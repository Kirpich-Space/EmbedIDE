//! Input traces exercise the modal parser without platform key events or GUI.
use embedide_editor::vim::{
    Action, Dispatch, ExCommand, IdeAction, InputError, InsertPosition, Key, Keymap, Mode, Motion,
    Operator, SelectionKind, VimInput, MAX_COMMAND_BYTES,
};
use embedide_editor::TextBuffer;

fn trace(input: &mut VimInput, text: &str) -> Vec<Action> {
    text.chars()
        .filter_map(|key| match input.feed(Key::Char(key)).unwrap() {
            Dispatch::Action(action) => Some(action),
            Dispatch::Consumed => None,
            Dispatch::Unhandled => panic!("unhandled key: {key:?}"),
        })
        .collect()
}

fn action(input: &mut VimInput, key: Key) -> Action {
    match input.feed(key).unwrap() {
        Dispatch::Action(action) => action,
        other => panic!("expected action, got {other:?}"),
    }
}

#[test]
fn counted_motions_and_word_navigation() {
    let mut input = VimInput::default();
    let actions = trace(&mut input, "5j2hklwbe0$");
    let expected = [
        (Motion::Down, 5),
        (Motion::Left, 2),
        (Motion::Up, 1),
        (Motion::Right, 1),
        (Motion::WordForward, 1),
        (Motion::WordBackward, 1),
        (Motion::WordEnd, 1),
        (Motion::LineStart, 1),
        (Motion::LineEnd, 1),
    ];
    assert_eq!(
        actions,
        expected.map(|(motion, count)| Action::Move { motion, count })
    );
}

#[test]
fn goto_preserves_one_based_line_counts() {
    let mut input = VimInput::default();
    assert_eq!(
        trace(&mut input, "gg12ggG4G"),
        [
            Action::Move {
                motion: Motion::Line(1),
                count: 1
            },
            Action::Move {
                motion: Motion::Line(12),
                count: 1
            },
            Action::Move {
                motion: Motion::LastLine,
                count: 1
            },
            Action::Move {
                motion: Motion::Line(4),
                count: 1
            },
        ]
    );
}

#[test]
fn operators_multiply_counts_and_change_enters_insert() {
    let mut input = VimInput::default();
    assert_eq!(
        trace(&mut input, "2d3d4yy2d3w"),
        [
            Action::Lines {
                operator: Operator::Delete,
                count: 6
            },
            Action::Lines {
                operator: Operator::Yank,
                count: 4
            },
            Action::Operate {
                operator: Operator::Delete,
                motion: Motion::WordForward,
                count: 6
            },
        ]
    );
    assert_eq!(
        trace(&mut input, "2c3iw"),
        [Action::InnerWord {
            operator: Operator::Change,
            count: 6
        },]
    );
    assert_eq!(input.mode(), Mode::Insert);
    assert_eq!(action(&mut input, Key::Escape), Action::FinishInsert);
    assert_eq!(input.mode(), Mode::Normal);
}

#[test]
fn delete_to_column_zero_is_not_a_repeat_prefix() {
    let mut input = VimInput::default();
    assert_eq!(
        trace(&mut input, "d0"),
        [Action::Operate {
            operator: Operator::Delete,
            motion: Motion::LineStart,
            count: 1
        },]
    );
    assert_eq!(
        trace(&mut input, "10j"),
        [Action::Move {
            motion: Motion::Down,
            count: 10
        }]
    );
}

#[test]
fn bad_prefix_and_escape_do_not_leak_operator_or_count() {
    let mut input = VimInput::default();
    assert!(trace(&mut input, "3diz").is_empty());
    assert_eq!(
        trace(&mut input, "j"),
        [Action::Move {
            motion: Motion::Down,
            count: 1
        }]
    );
    trace(&mut input, "7d");
    input.feed(Key::Escape).unwrap();
    assert!(trace(&mut input, "d").is_empty());
    assert_eq!(
        trace(&mut input, "d"),
        [Action::Lines {
            operator: Operator::Delete,
            count: 1
        }]
    );
}

#[test]
fn count_overflow_is_an_error_and_parser_recovers() {
    let mut input = VimInput::default();
    trace(&mut input, "100000");
    assert_eq!(input.feed(Key::Char('1')), Err(InputError::CountTooLarge));
    assert_eq!(
        trace(&mut input, "j"),
        [Action::Move {
            motion: Motion::Down,
            count: 1
        }]
    );
    trace(&mut input, "1001d1000");
    assert_eq!(input.feed(Key::Char('d')), Err(InputError::CountTooLarge));
    assert_eq!(input.mode(), Mode::Normal);
    assert_eq!(
        trace(&mut input, "j"),
        [Action::Move {
            motion: Motion::Down,
            count: 1
        }]
    );
}

#[test]
fn insert_mode_preserves_spaces_keys_and_committed_unicode() {
    let mut input = VimInput::default();
    assert_eq!(
        trace(&mut input, "i"),
        [Action::BeginInsert {
            position: InsertPosition::Before,
            repeat: 1
        }]
    );
    assert_eq!(
        trace(&mut input, " dd j"),
        [
            Action::InsertText(" ".into()),
            Action::InsertText("d".into()),
            Action::InsertText("d".into()),
            Action::InsertText(" ".into()),
            Action::InsertText("j".into()),
        ]
    );
    assert_eq!(
        action(&mut input, Key::Text("я🦀".into())),
        Action::InsertText("я🦀".into())
    );
    assert_eq!(action(&mut input, Key::Enter), Action::InsertNewline);
    assert_eq!(action(&mut input, Key::Tab), Action::InsertTab);
    assert_eq!(action(&mut input, Key::Backspace), Action::Backspace);
    assert_eq!(action(&mut input, Key::Delete), Action::DeleteForward(1));
}

#[test]
fn normal_paste_text_never_executes_modal_commands() {
    let mut input = VimInput::default();
    assert_eq!(
        input.feed(Key::Text("dd:wq".into())),
        Ok(Dispatch::Consumed)
    );
    assert_eq!(input.mode(), Mode::Normal);
    assert_eq!(
        trace(&mut input, "j"),
        [Action::Move {
            motion: Motion::Down,
            count: 1
        }]
    );
}

#[test]
fn insert_placements_and_counts_are_explicit() {
    for (key, position) in [
        ('i', InsertPosition::Before),
        ('a', InsertPosition::After),
        ('I', InsertPosition::LineStart),
        ('A', InsertPosition::LineEnd),
        ('o', InsertPosition::LineBelow),
        ('O', InsertPosition::LineAbove),
    ] {
        let mut input = VimInput::default();
        trace(&mut input, "3");
        assert_eq!(
            action(&mut input, Key::Char(key)),
            Action::BeginInsert {
                position,
                repeat: 3
            }
        );
        assert_eq!(input.mode(), Mode::Insert);
    }
}

#[test]
fn visual_mode_extends_motions_and_applies_operator() {
    let mut input = VimInput::default();
    assert_eq!(
        trace(&mut input, "v2j"),
        [
            Action::BeginVisual(SelectionKind::Character),
            Action::Move {
                motion: Motion::Down,
                count: 2
            },
        ]
    );
    assert_eq!(input.mode(), Mode::Visual);
    assert_eq!(
        trace(&mut input, "V"),
        [Action::BeginVisual(SelectionKind::Line)]
    );
    assert_eq!(
        trace(&mut input, "y"),
        [Action::VisualOperator(Operator::Yank)]
    );
    assert_eq!(input.mode(), Mode::Normal);
    assert_eq!(
        trace(&mut input, "vc"),
        [
            Action::BeginVisual(SelectionKind::Character),
            Action::VisualOperator(Operator::Change),
        ]
    );
    assert_eq!(input.mode(), Mode::Insert);
}

#[test]
fn visual_toggle_and_escape_clear_selection() {
    let mut input = VimInput::default();
    assert_eq!(
        trace(&mut input, "vv"),
        [
            Action::BeginVisual(SelectionKind::Character),
            Action::EndVisual
        ]
    );
    trace(&mut input, "V");
    assert_eq!(action(&mut input, Key::Escape), Action::EndVisual);
    assert_eq!(input.mode(), Mode::Normal);
}

#[test]
fn write_quit_and_writequit_are_requests_not_io() {
    for (text, expected) in [
        ("w", ExCommand::Write),
        ("q", ExCommand::Quit),
        ("q!", ExCommand::ForceQuit),
        ("wq", ExCommand::WriteQuit),
    ] {
        let mut input = VimInput::default();
        trace(&mut input, ":");
        assert_eq!(input.command_prompt(), Some(':'));
        assert_eq!(input.mode(), Mode::Command);
        assert!(trace(&mut input, text).is_empty());
        assert_eq!(input.command_text(), Some(text));
        assert_eq!(action(&mut input, Key::Enter), Action::Ex(expected));
        assert_eq!(input.mode(), Mode::Normal);
        assert_eq!(input.command_text(), None);
    }
}

#[test]
fn unknown_commands_and_shell_escapes_are_rejected() {
    for text in ["!make", "w other.c", "wq!", "source settings", ""] {
        let mut input = VimInput::default();
        trace(&mut input, ":");
        trace(&mut input, text);
        assert_eq!(
            input.feed(Key::Enter),
            Err(InputError::UnknownCommand(text.into()))
        );
        assert_eq!(input.mode(), Mode::Normal);
    }
}

#[test]
fn search_prompts_preserve_text_and_visual_context() {
    let mut input = VimInput::default();
    trace(&mut input, "v/я🦀");
    assert_eq!(input.command_prompt(), Some('/'));
    input.feed(Key::Backspace).unwrap();
    assert_eq!(input.command_text(), Some("я"));
    assert_eq!(
        action(&mut input, Key::Enter),
        Action::Search {
            query: "я".into(),
            backward: false
        }
    );
    assert_eq!(input.mode(), Mode::Visual);
    trace(&mut input, "?foo");
    input.feed(Key::Escape).unwrap();
    assert_eq!(input.mode(), Mode::Visual);
    assert_eq!(input.command_text(), None);
    input.feed(Key::Escape).unwrap();
    trace(&mut input, "?bar");
    assert_eq!(
        action(&mut input, Key::Enter),
        Action::Search {
            query: "bar".into(),
            backward: true
        }
    );
    assert_eq!(
        trace(&mut input, "2nN"),
        [
            Action::RepeatSearch {
                opposite: false,
                count: 2
            },
            Action::RepeatSearch {
                opposite: true,
                count: 1
            },
        ]
    );
}

#[test]
fn prompt_length_limit_is_byte_based_and_rejection_is_atomic() {
    let mut input = VimInput::default();
    trace(&mut input, "/");
    let text = "я".repeat(MAX_COMMAND_BYTES / 2);
    input.feed(Key::Text(text.clone())).unwrap();
    assert_eq!(input.feed(Key::Char('x')), Err(InputError::CommandTooLong));
    assert_eq!(input.command_text(), Some(text.as_str()));
    input.feed(Key::Backspace).unwrap();
    assert_eq!(input.command_text().unwrap().len(), MAX_COMMAND_BYTES - 2);
    input.feed(Key::Char('x')).unwrap();
}

#[test]
fn leader_chords_are_normal_only_and_configurable() {
    let mut keymap = Keymap::default();
    keymap.set_leader(',');
    keymap.bind_leader('r', IdeAction::Build);
    assert_eq!(keymap.unbind_leader('b'), Some(IdeAction::Build));
    let mut input = VimInput::with_keymap(keymap);
    assert_eq!(trace(&mut input, ",r"), [Action::Ide(IdeAction::Build)]);
    assert!(trace(&mut input, ",b").is_empty());
    assert_eq!(
        trace(&mut input, "i,r"),
        [
            Action::BeginInsert {
                position: InsertPosition::Before,
                repeat: 1
            },
            Action::InsertText(",".into()),
            Action::InsertText("r".into()),
        ]
    );
}

#[test]
fn default_leader_bindings_and_unknown_suffix() {
    let mut input = VimInput::default();
    assert_eq!(
        trace(&mut input, " e b d t s f"),
        [
            Action::Ide(IdeAction::ProjectExplorer),
            Action::Ide(IdeAction::Build),
            Action::Ide(IdeAction::Debug),
            Action::Ide(IdeAction::Terminal),
            Action::Ide(IdeAction::SerialMonitor),
            Action::Ide(IdeAction::FindFile),
        ]
    );
    assert!(trace(&mut input, " i").is_empty());
    assert_eq!(input.mode(), Mode::Normal);
}

#[test]
fn remaps_apply_once_without_altering_insert_prompt_or_leader_suffix() {
    let mut keymap = Keymap::default();
    keymap.remap('H', 'h');
    keymap.remap('h', 'l');
    keymap.remap('b', 'j');
    let mut input = VimInput::with_keymap(keymap);
    assert_eq!(
        trace(&mut input, "Hh b"),
        [
            Action::Move {
                motion: Motion::Left,
                count: 1
            },
            Action::Move {
                motion: Motion::Right,
                count: 1
            },
            Action::Ide(IdeAction::Build),
        ]
    );
    trace(&mut input, "/Hb");
    assert_eq!(input.command_text(), Some("Hb"));
    input.feed(Key::Escape).unwrap();
    trace(&mut input, "i");
    assert_eq!(
        trace(&mut input, "Hh"),
        [
            Action::InsertText("H".into()),
            Action::InsertText("h".into())
        ]
    );
}

#[test]
fn undo_redo_and_system_control_keys_do_not_conflict() {
    let mut input = VimInput::default();
    assert_eq!(trace(&mut input, "2u"), [Action::Undo(2)]);
    trace(&mut input, "3");
    assert_eq!(action(&mut input, Key::Ctrl('r')), Action::Redo(3));
    trace(&mut input, "4d");
    assert_eq!(input.feed(Key::Ctrl('s')), Ok(Dispatch::Unhandled));
    assert_eq!(
        trace(&mut input, "j"),
        [Action::Move {
            motion: Motion::Down,
            count: 1
        }]
    );
    trace(&mut input, "i");
    assert_eq!(input.feed(Key::Ctrl('r')), Ok(Dispatch::Unhandled));
    assert_eq!(input.mode(), Mode::Insert);
}

#[test]
fn paste_register_commands_retain_direction_and_count() {
    let mut input = VimInput::default();
    assert_eq!(
        trace(&mut input, "2pP"),
        [
            Action::Paste {
                before: false,
                count: 2
            },
            Action::Paste {
                before: true,
                count: 1
            },
        ]
    );
}

#[test]
fn reset_clears_failed_change_and_prompt_state() {
    let mut input = VimInput::default();
    trace(&mut input, "ciw");
    assert_eq!(input.mode(), Mode::Insert);
    input.reset();
    trace(&mut input, ":w");
    input.reset();
    assert_eq!(input.mode(), Mode::Normal);
    assert_eq!(input.command_text(), None);
    assert_eq!(
        trace(&mut input, "j"),
        [Action::Move {
            motion: Motion::Down,
            count: 1
        }]
    );
}

#[test]
fn arrow_keys_remain_motions_in_insert_without_becoming_text() {
    let mut input = VimInput::default();
    trace(&mut input, "i");
    assert_eq!(
        action(&mut input, Key::Left),
        Action::Move {
            motion: Motion::Left,
            count: 1
        }
    );
    assert_eq!(input.mode(), Mode::Insert);
    assert_eq!(
        action(&mut input, Key::Home),
        Action::Move {
            motion: Motion::LineStart,
            count: 1
        }
    );
}

#[test]
fn insert_trace_drives_real_buffer_and_grouped_undo() {
    let mut input = VimInput::default();
    let mut buffer = TextBuffer::new(String::new());
    let keys = [
        Key::Char('i'),
        Key::Text("я".into()),
        Key::Text("🦀".into()),
        Key::Escape,
        Key::Char('u'),
        Key::Ctrl('r'),
    ];
    for key in keys {
        match action(&mut input, key) {
            Action::BeginInsert {
                position: InsertPosition::Before,
                repeat: 1,
            } => buffer.begin_edit_group(),
            Action::InsertText(text) => {
                let end = buffer.byte_len();
                buffer.replace(end..end, &text).unwrap();
            }
            Action::FinishInsert => buffer.end_edit_group(),
            Action::Undo(1) => {
                assert!(buffer.undo());
                assert!(buffer.is_empty());
                assert!(!buffer.can_undo());
            }
            Action::Redo(1) => {
                assert!(buffer.redo());
            }
            other => panic!("unexpected action in trace: {other:?}"),
        }
    }
    assert_eq!(buffer.contents(), "я🦀");
}

//! Typed action interpreter. Every edit goes through EditorState::replace.
use crate::{
    document::SearchEntry,
    indent,
    motion::{self, next_unit, previous_unit},
    search,
    session::Effect,
    vim::{Action, ExCommand, InsertPosition, Motion, Operator, SelectionKind},
    EditorError, EditorState, Register, Selection,
};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

fn repeated(text: &str, count: u32) -> Result<String, EditorError> {
    let len = text
        .len()
        .checked_mul(count as usize)
        .ok_or(EditorError::EditTooLarge)?;
    if len > 64 * 1024 * 1024 {
        return Err(EditorError::EditTooLarge);
    }
    Ok(text.repeat(count as usize))
}

fn selected(state: &EditorState) -> Result<(Range<usize>, SelectionKind), EditorError> {
    let selection = state
        .selection
        .ok_or(EditorError::Unsupported("no active Visual selection"))?;
    let start = selection.anchor.min(selection.head);
    let end = selection.anchor.max(selection.head);
    let range = if selection.kind == SelectionKind::Line {
        state
            .lines
            .lines(state.lines.row(start), state.lines.row(end), &state.buffer)
    } else {
        start..next_unit(&state.buffer, end)?
    };
    Ok((range, selection.kind))
}

fn apply_range(
    state: &mut EditorState,
    operator: Operator,
    raw: Range<usize>,
    kind: SelectionKind,
    visual: bool,
) -> Result<(), EditorError> {
    let text = state.buffer.slice(raw.clone())?;
    if raw.is_empty() && kind == SelectionKind::Character && operator != Operator::Change {
        state.selection = None;
        return Ok(());
    }
    if operator == Operator::Yank {
        state.unnamed = Some(Register { text, kind });
        if visual {
            state.cursor = raw.start;
        }
        state.selection = None;
        state.preferred = None;
        return Ok(());
    }
    let mut range = raw.clone();
    if kind == SelectionKind::Line {
        if operator == Operator::Change && text.ends_with('\n') {
            range.end -= if text.ends_with("\r\n") { 2 } else { 1 };
        } else if operator == Operator::Delete
            && range.end == state.buffer.byte_len()
            && range.start > 0
            && (range.is_empty() || state.buffer.byte_at(range.end - 1) != Some(b'\n'))
        {
            range.start = previous_unit(&state.buffer, range.start)?;
        }
    }
    if range.is_empty() && operator == Operator::Delete {
        state.selection = None;
        return Ok(());
    }
    let changing = operator == Operator::Change;
    if changing {
        state.begin_group();
    }
    let cursor = range.start;
    state.replace(range, "", cursor, changing)?;
    state.unnamed = Some(Register { text, kind });
    state.selection = None;
    Ok(())
}

/// Shift one line by a level; the cursor keeps its place in the text (or
/// moves to the end of the new indentation when it was inside it).
fn shift_row(
    state: &mut EditorState,
    row: usize,
    dedent: bool,
    insert: bool,
) -> Result<(), EditorError> {
    let (range, whitespace, blank) = indent::line_indent(state, row)?;
    if blank && !dedent && !insert {
        return Ok(()); // like Vim, empty lines are not indented by line shifts
    }
    let new = indent::shifted(&state.indent, &whitespace, dedent);
    if new == whitespace {
        return Ok(());
    }
    let cursor = if state.lines.row(state.cursor) != row {
        state.cursor
    } else if state.cursor <= range.end {
        range.start + new.len()
    } else {
        state.cursor - whitespace.len() + new.len()
    };
    state.replace(range, &new, cursor, insert)
}

/// Shift lines `first..=last` as one undo step; the cursor goes to the
/// first non-blank character of `first`.
fn shift_rows(
    state: &mut EditorState,
    first: usize,
    last: usize,
    dedent: bool,
) -> Result<(), EditorError> {
    state.begin_group();
    let result = (first..=last)
        .rev()
        .try_for_each(|row| shift_row(state, row, dedent, false));
    state.finish_group();
    result?;
    let target = state.lines.first_nonblank(first, &state.buffer)?;
    state.cursor = state.lines.clamp(target, false, &state.buffer)?;
    state.preferred = None;
    Ok(())
}

fn restore_history(state: &mut EditorState, count: u32, redo: bool) -> Result<(), EditorError> {
    for _ in 0..count {
        if !(if redo {
            state.buffer.can_redo()
        } else {
            state.buffer.can_undo()
        }) {
            break;
        }
        state.restore(redo)?;
    }
    Ok(())
}

fn paste(state: &mut EditorState, before: bool, count: u32) -> Result<(), EditorError> {
    let register = state.unnamed.as_ref().ok_or(EditorError::EmptyRegister)?;
    let row = state.lines.row(state.cursor);
    let (offset, text, cursor) = if register.kind == SelectionKind::Character {
        let text = repeated(&register.text, count)?;
        let end = state.lines.end(row, &state.buffer);
        let offset = if before || state.cursor == end {
            state.cursor
        } else {
            next_unit(&state.buffer, state.cursor)?
        };
        let cursor = if let Some((index, _)) = text.grapheme_indices(true).next_back() {
            offset + index
        } else {
            offset
        };
        (offset, text, cursor)
    } else {
        let body = register
            .text
            .strip_suffix("\r\n")
            .or_else(|| register.text.strip_suffix('\n'))
            .unwrap_or(&register.text);
        let mut unit = String::from(body);
        unit.push_str(state.eol);
        let mut payload = repeated(&unit, count)?;
        payload.truncate(payload.len() - state.eol.len());
        if state.buffer.is_empty() {
            if body.is_empty() || register.text.ends_with('\n') {
                payload.push_str(state.eol);
            }
            (0, payload, 0)
        } else if before {
            payload.push_str(state.eol);
            let start = state.lines.start(row);
            (start, payload, start)
        } else if row + 1 < state.lines.count() {
            payload.push_str(state.eol);
            let start = state.lines.start(row + 1);
            (start, payload, start)
        } else {
            let end = state.buffer.byte_len();
            payload.insert_str(0, state.eol);
            (end, payload, end + state.eol.len())
        }
    };
    state.replace(offset..offset, &text, cursor, false)
}

fn begin_insert(
    state: &mut EditorState,
    position: InsertPosition,
    repeat: u32,
) -> Result<(), EditorError> {
    if repeat != 1 {
        return Err(EditorError::Unsupported(
            "counted Insert sessions are not implemented; repeat paste or use a single Insert",
        ));
    }
    let row = state.lines.row(state.cursor);
    let end = state.lines.end(row, &state.buffer);
    let target = match position {
        InsertPosition::Before => state.cursor,
        InsertPosition::After => {
            if state.cursor < end {
                next_unit(&state.buffer, state.cursor)?
            } else {
                end
            }
        }
        InsertPosition::LineStart => state.lines.first_nonblank(row, &state.buffer)?,
        InsertPosition::LineEnd => end,
        InsertPosition::LineAbove => state.lines.start(row),
        InsertPosition::LineBelow => {
            if row + 1 < state.lines.count() {
                state.lines.start(row + 1)
            } else {
                end
            }
        }
    };
    state.begin_group();
    if position == InsertPosition::LineBelow {
        // `o` is Enter at the end of the line: same indentation rules.
        state.cursor = end;
        let (range, text, cursor) = indent::newline(state)?;
        let at = range.start;
        state.replace(range, &text, at + cursor, true)?;
    } else if position == InsertPosition::LineAbove {
        // `O` keeps the indentation of the line it opens above.
        let (_, whitespace, _) = indent::line_indent(state, row)?;
        let text = format!("{whitespace}{}", state.eol);
        state.replace(target..target, &text, target + whitespace.len(), true)?;
    } else {
        state.cursor = target;
    }
    state.preferred = None;
    Ok(())
}

fn run_search(
    state: &mut EditorState,
    query: String,
    backward: bool,
    count: u32,
    remember: bool,
) -> Result<(), EditorError> {
    let pattern = if query.is_empty() {
        state
            .searches
            .back()
            .ok_or(EditorError::NoSearch)?
            .pattern
            .clone()
    } else {
        query
    };
    let offset = search::find(&state.buffer, &pattern, state.cursor, backward, count)?;
    state.cursor = state.lines.clamp(offset, false, &state.buffer)?;
    state.preferred = None;
    if let Some(selection) = &mut state.selection {
        selection.head = state.cursor;
    }
    if remember {
        state.searches.push_back(SearchEntry { pattern, backward });
        if state.searches.len() > 32 {
            state.searches.pop_front();
        }
    }
    Ok(())
}

pub(crate) fn execute(
    state: &mut EditorState,
    action: Action,
    was_insert: bool,
) -> Result<Effect, EditorError> {
    match action {
        Action::Move { motion, count } => {
            let (cursor, preferred) = motion::resolve(
                &state.buffer,
                &state.lines,
                state.cursor,
                state.preferred,
                motion,
                count,
                was_insert,
            )?;
            if was_insert {
                state.finish_group();
            }
            state.cursor = cursor;
            state.preferred = preferred;
            if let Some(selection) = &mut state.selection {
                selection.head = cursor;
            }
        }
        Action::BeginInsert { position, repeat } => begin_insert(state, position, repeat)?,
        Action::FinishInsert => {
            let start = state.lines.start(state.lines.row(state.cursor));
            if state.cursor > start {
                state.cursor = previous_unit(&state.buffer, state.cursor)?;
            }
            state.cursor = state.lines.clamp(state.cursor, false, &state.buffer)?;
            state.finish_group();
            state.preferred = None;
        }
        Action::InsertText(text) => {
            state.begin_group();
            state.replace(
                state.cursor..state.cursor,
                &text,
                state.cursor + text.len(),
                true,
            )?;
        }
        Action::InsertNewline => {
            state.begin_group();
            let (range, text, cursor) = indent::newline(state)?;
            let at = range.start;
            state.replace(range, &text, at + cursor, true)?;
        }
        Action::InsertTab => {
            state.begin_group();
            let row = state.lines.row(state.cursor);
            let (range, whitespace, _) = indent::line_indent(state, row)?;
            if state.cursor <= range.end {
                let new = indent::shifted(&state.indent, &whitespace, false);
                let cursor = range.start + new.len();
                state.replace(range, &new, cursor, true)?;
            } else {
                let text = indent::tab_text(state)?;
                let cursor = state.cursor;
                state.replace(cursor..cursor, &text, cursor + text.len(), true)?;
            }
        }
        Action::Dedent => {
            state.begin_group();
            let row = state.lines.row(state.cursor);
            shift_row(state, row, true, true)?;
        }
        Action::TypeCloser(closer) => {
            state.begin_group();
            if let Some((range, whitespace)) = indent::closer(state, closer)? {
                let cursor = range.start + whitespace.len();
                state.replace(range, &whitespace, cursor, true)?;
            }
            let text = closer.to_string();
            let cursor = state.cursor;
            state.replace(cursor..cursor, &text, cursor + text.len(), true)?;
        }
        Action::ShiftLines { dedent, count } => {
            let row = state.lines.row(state.cursor);
            let last = row
                .saturating_add(count.max(1) as usize - 1)
                .min(state.lines.count() - 1);
            shift_rows(state, row, last, dedent)?;
        }
        Action::ShiftSelection { dedent } => {
            let selection = state
                .selection
                .ok_or(EditorError::Unsupported("no active Visual selection"))?;
            let first = state.lines.row(selection.anchor.min(selection.head));
            let last = state.lines.row(selection.anchor.max(selection.head));
            state.selection = None;
            shift_rows(state, first, last, dedent)?;
        }
        Action::Backspace => {
            if state.cursor > 0 {
                state.begin_group();
                let start = previous_unit(&state.buffer, state.cursor)?;
                state.replace(start..state.cursor, "", start, true)?;
            }
        }
        Action::DeleteForward(count) => {
            let mut end = state.cursor;
            let limit = if was_insert {
                state.buffer.byte_len()
            } else {
                state.lines.end(state.lines.row(end), &state.buffer)
            };
            for _ in 0..count {
                if end >= limit {
                    break;
                }
                end = next_unit(&state.buffer, end)?.min(limit);
            }
            if was_insert {
                state.begin_group();
                state.replace(state.cursor..end, "", state.cursor, true)?;
            } else {
                let start = state.cursor;
                apply_range(
                    state,
                    Operator::Delete,
                    start..end,
                    SelectionKind::Character,
                    false,
                )?;
            }
        }
        Action::Lines { operator, count } => {
            let row = state.lines.row(state.cursor);
            let last = row
                .saturating_add(count.saturating_sub(1) as usize)
                .min(state.lines.count() - 1);
            let range = state.lines.lines(row, last, &state.buffer);
            apply_range(state, operator, range, SelectionKind::Line, false)?;
        }
        Action::Operate {
            operator,
            motion,
            count,
        } => {
            if operator == Operator::Change
                && motion == Motion::WordForward
                && state
                    .buffer
                    .char_at(state.cursor)?
                    .is_some_and(|ch| !ch.is_whitespace())
            {
                let word = motion::inner_word(&state.buffer, state.cursor, count)?;
                let start = state.cursor;
                apply_range(
                    state,
                    operator,
                    start..word.end,
                    SelectionKind::Character,
                    false,
                )?;
                return Ok(Effect::Applied);
            }
            let target = if motion == Motion::WordForward {
                motion::operator_word_forward(&state.buffer, &state.lines, state.cursor, count)?
            } else {
                motion::resolve(
                    &state.buffer,
                    &state.lines,
                    state.cursor,
                    None,
                    motion,
                    count,
                    true,
                )?
                .0
            };
            if matches!(
                motion,
                Motion::Up | Motion::Down | Motion::Line(_) | Motion::LastLine
            ) {
                let a = state.lines.row(state.cursor);
                let b = state.lines.row(target);
                let range = state.lines.lines(a.min(b), a.max(b), &state.buffer);
                apply_range(state, operator, range, SelectionKind::Line, false)?;
            } else {
                let start = state.cursor.min(target);
                let mut end = state.cursor.max(target);
                if matches!(motion, Motion::WordEnd) {
                    end = next_unit(&state.buffer, end)?;
                }
                // LineEnd resolve uses an insertion endpoint already excluding EOL.
                apply_range(state, operator, start..end, SelectionKind::Character, false)?;
            }
        }
        Action::InnerWord { operator, count } => {
            let range = motion::inner_word(&state.buffer, state.cursor, count)?;
            apply_range(state, operator, range, SelectionKind::Character, false)?;
        }
        Action::BeginVisual(kind) => {
            let anchor = state
                .selection
                .map(|selection| selection.anchor)
                .unwrap_or(state.cursor);
            state.selection = Some(Selection {
                anchor,
                head: state.cursor,
                kind,
            });
        }
        Action::EndVisual => state.selection = None,
        Action::VisualOperator(operator) => {
            let (range, kind) = selected(state)?;
            apply_range(state, operator, range, kind, true)?;
        }
        Action::Paste { before, count } => paste(state, before, count)?,
        Action::Undo(count) => restore_history(state, count, false)?,
        Action::Redo(count) => restore_history(state, count, true)?,
        Action::Search { query, backward } => run_search(state, query, backward, 1, true)?,
        Action::RepeatSearch { opposite, count } => {
            let previous = state.searches.back().ok_or(EditorError::NoSearch)?.clone();
            run_search(
                state,
                previous.pattern,
                previous.backward ^ opposite,
                count,
                false,
            )?;
        }
        Action::Ex(ExCommand::Write) => return Ok(Effect::Saved(state.save()?)),
        Action::Ex(ExCommand::Host(command)) => return Ok(Effect::Command(command)),
        Action::Ex(command) => {
            if command == ExCommand::WriteQuit {
                state.save()?;
            }
            if command != ExCommand::ForceQuit && state.is_modified() {
                return Err(EditorError::UnsavedChanges);
            }
            state.closed = true;
            return Ok(Effect::Closed);
        }
        Action::Ide(action) => return Ok(Effect::Ide(action)),
    }
    Ok(Effect::Applied)
}

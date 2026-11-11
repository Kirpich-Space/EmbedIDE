//! Automatic indentation through the real parser and executor.
use embedide_editor::{indent::IndentStyle, vim::Key, Editor};
use std::path::PathBuf;

/// An unsaved document bound to `name` (the extension selects the language);
/// nothing is written to disk.
fn doc(name: &str, text: &str) -> Editor {
    let dir = std::env::temp_dir().join(format!("embed-indent-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path: PathBuf = dir.join(name);
    let mut editor = Editor::create(&path).unwrap();
    editor.replace(0..0, text).unwrap();
    editor.set_cursor(0).unwrap();
    editor
}

/// Type in Insert mode: `\n` is Enter, `\t` Tab, `\x08` Shift+Tab; others are typed keys.
fn type_text(editor: &mut Editor, text: &str) {
    for ch in text.chars() {
        let key = match ch {
            '\n' => Key::Enter,
            '\t' => Key::Tab,
            '\x08' => Key::BackTab,
            c => Key::Char(c),
        };
        editor.feed(key).unwrap();
    }
}

fn text(editor: &Editor) -> String {
    editor.state().buffer().contents()
}

/// Open Insert mode at the end of the document.
fn append(editor: &mut Editor) {
    editor.feed(Key::Char('G')).unwrap();
    editor.feed(Key::Char('A')).unwrap();
}

#[test]
fn enter_keeps_the_current_indentation() {
    let mut e = doc("a.c", "    int x = 1;");
    append(&mut e);
    type_text(&mut e, "\nint y;");
    assert_eq!(text(&e), "    int x = 1;\n    int y;");
}

#[test]
fn enter_after_an_opening_brace_indents_one_level_per_language() {
    for (name, line, expected) in [
        ("m.rs", "fn main() {", "fn main() {\n    x"),
        ("m.c", "int main(void) {", "int main(void) {\n    x"),
        ("m.cpp", "class A {", "class A {\n    x"),
        (
            "m.zig",
            "pub fn main() void {",
            "pub fn main() void {\n    x",
        ),
        ("m.rs", "let v = vec![", "let v = vec![\n    x"),
        ("m.c", "call(", "call(\n    x"),
        ("m.py", "def f():", "def f():\n    x"),
        ("m.sh", "if true; then", "if true; then\n    x"),
        ("m.S", "reset:", "reset:\n    x"),
    ] {
        let mut e = doc(name, line);
        append(&mut e);
        type_text(&mut e, "\nx");
        assert_eq!(text(&e), expected, "{name}");
    }
    // Languages without block rules keep the indentation only.
    let mut e = doc("m.toml", "[package]");
    append(&mut e);
    type_text(&mut e, "\nx");
    assert_eq!(text(&e), "[package]\nx");
    // A colon does not open a block in C, and braces do not in Python.
    let mut e = doc("m.c", "  case 1:");
    append(&mut e);
    type_text(&mut e, "\nx");
    assert_eq!(text(&e), "  case 1:\n  x");
}

#[test]
fn enter_between_braces_opens_an_indented_line_and_moves_the_closer() {
    let mut e = doc("m.rs", "fn main() {}");
    e.feed(Key::Char('$')).unwrap(); // on the closing brace
    e.feed(Key::Char('i')).unwrap();
    type_text(&mut e, "\nlet a = 1;");
    assert_eq!(text(&e), "fn main() {\n    let a = 1;\n}");
    // The example from the task: Enter inside an indented block body.
    let mut e = doc("m.rs", "fn main() {\n    // comment\n}");
    e.feed(Key::Char('j')).unwrap();
    e.feed(Key::Char('A')).unwrap();
    type_text(&mut e, "\nrun();");
    assert_eq!(text(&e), "fn main() {\n    // comment\n    run();\n}");
}

#[test]
fn braces_in_strings_and_comments_do_not_change_indentation() {
    for line in [
        "let s = \"{\";",
        "x = 1; // {",
        "/* { */ y = 2;",
        "char c = '{';",
    ] {
        let mut e = doc("m.rs", line);
        append(&mut e);
        type_text(&mut e, "\nz");
        assert_eq!(text(&e), format!("{line}\nz"), "{line}");
    }
    // Inside a block comment Enter keeps the indentation; `}` is not aligned.
    let mut e = doc("m.c", "/*\n   text {");
    append(&mut e);
    type_text(&mut e, "\n}");
    assert_eq!(text(&e), "/*\n   text {\n   }");
}

#[test]
fn closing_delimiters_align_with_their_opener() {
    let mut e = doc("m.c", "void f(void) {\n    if (x) {\n        y();");
    append(&mut e);
    type_text(&mut e, "\n}\n}");
    assert_eq!(
        text(&e),
        "void f(void) {\n    if (x) {\n        y();\n    }\n}"
    );
    // Nested brackets are matched by depth, and `)` / `]` align too.
    let mut e = doc("m.rs", "let v = [\n    (1, 2),\n    3,");
    append(&mut e);
    type_text(&mut e, "\n]");
    assert_eq!(text(&e), "let v = [\n    (1, 2),\n    3,\n]");
    // A closer after other text on the line is inserted unchanged.
    let mut e = doc("m.rs", "fn f() {\n    g(");
    append(&mut e);
    type_text(&mut e, "x)");
    assert_eq!(text(&e), "fn f() {\n    g(x)");
    // Assembly and TOML have no closer rules.
    let mut e = doc("m.toml", "a = [\n    1,\n    ");
    append(&mut e);
    type_text(&mut e, "]");
    assert_eq!(text(&e), "a = [\n    1,\n    ]");
}

#[test]
fn enter_on_a_blank_auto_indented_line_leaves_no_trailing_spaces() {
    let mut e = doc("m.rs", "fn f() {");
    append(&mut e);
    type_text(&mut e, "\n\nx");
    assert_eq!(text(&e), "fn f() {\n\n    x");
}

#[test]
fn tab_and_shift_tab_change_indentation_in_insert_mode() {
    let mut e = doc("m.c", "x;");
    e.feed(Key::Char('i')).unwrap();
    type_text(&mut e, "\t");
    assert_eq!(text(&e), "    x;");
    type_text(&mut e, "\t");
    assert_eq!(text(&e), "        x;");
    type_text(&mut e, "\x08");
    assert_eq!(text(&e), "    x;");
    // After text, Tab inserts spaces to the next stop; Shift+Tab still dedents the line.
    e.feed(Key::End).unwrap();
    type_text(&mut e, "\tz");
    assert_eq!(text(&e), "    x;  z");
    type_text(&mut e, "\x08");
    assert_eq!(text(&e), "x;  z");
    type_text(&mut e, "\x08");
    assert_eq!(text(&e), "x;  z", "no indentation left");
    // Partial levels snap to the grid.
    let mut e = doc("m.c", "  y;");
    e.feed(Key::Char('I')).unwrap();
    type_text(&mut e, "\t");
    assert_eq!(text(&e), "    y;");
}

#[test]
fn width_and_tabs_are_configurable() {
    let mut e = doc("m.rs", "fn f() {");
    e.set_indent(IndentStyle::new(2, false, true));
    append(&mut e);
    type_text(&mut e, "\nx\t");
    assert_eq!(text(&e), "fn f() {\n  x ");
    let mut e = doc("m.c", "int f() {");
    e.set_indent(IndentStyle::new(8, true, true));
    append(&mut e);
    type_text(&mut e, "\nreturn 0;\n}");
    assert_eq!(text(&e), "int f() {\n\treturn 0;\n}");
    // Auto-indent off: Enter keeps indentation, no language rules.
    let mut e = doc("m.rs", "    fn f() {");
    e.set_indent(IndentStyle::new(4, false, false));
    append(&mut e);
    type_text(&mut e, "\nx\n}");
    assert_eq!(text(&e), "    fn f() {\n    x\n    }");
}

#[test]
fn detection_adopts_the_file_convention_and_makefiles_use_tabs() {
    let spaces = IndentStyle::default();
    let tabbed = "int f() {\n\treturn 0;\n}\n";
    assert!(spaces.detect(None, tabbed).tabs);
    assert!(!spaces.detect(None, "a\n  b\n  c\n").tabs);
    let configured_tabs = IndentStyle::new(4, true, true);
    assert!(
        configured_tabs.detect(None, "flat\ntext\n").tabs,
        "no evidence keeps config"
    );
    assert!(
        spaces
            .detect(Some(std::path::Path::new("x/Makefile")), "")
            .tabs
    );
    assert!(
        spaces
            .detect(Some(std::path::Path::new("rules.mk")), "")
            .tabs
    );
}

#[test]
fn normal_and_visual_tab_shift_whole_lines_as_one_undo_step() {
    let mut e = doc("m.rs", "a\n\nb\nc");
    e.feed(Key::Char('3')).unwrap();
    e.feed(Key::Tab).unwrap();
    assert_eq!(text(&e), "    a\n\n    b\nc", "blank lines stay empty");
    e.feed(Key::Char('u')).unwrap();
    assert_eq!(text(&e), "a\n\nb\nc", "one undo step for all lines");
    e.feed(Key::Ctrl('r')).unwrap();
    assert_eq!(text(&e), "    a\n\n    b\nc");
    // Visual line selection of b..c, Shift+Tab then Tab twice.
    e.feed(Key::Char('j')).unwrap();
    e.feed(Key::Char('j')).unwrap();
    e.feed(Key::Char('V')).unwrap();
    e.feed(Key::Char('j')).unwrap();
    e.feed(Key::Tab).unwrap();
    assert_eq!(text(&e), "    a\n\n        b\n    c");
    assert!(e.state().selection().is_none());
    // The cursor is on the first shifted line (b); Shift+Tab in Normal mode shifts it back.
    e.feed(Key::BackTab).unwrap();
    assert_eq!(text(&e), "    a\n\n    b\n    c");
    e.feed(Key::Char('u')).unwrap();
    assert_eq!(text(&e), "    a\n\n        b\n    c");
}

#[test]
fn indentation_typed_in_insert_mode_undoes_with_the_insert() {
    let mut e = doc("m.rs", "fn f() {}");
    e.feed(Key::Char('$')).unwrap();
    e.feed(Key::Char('i')).unwrap();
    type_text(&mut e, "\nx();");
    e.feed(Key::Escape).unwrap();
    assert_eq!(text(&e), "fn f() {\n    x();\n}");
    e.feed(Key::Char('u')).unwrap();
    assert_eq!(text(&e), "fn f() {}");
    e.feed(Key::Ctrl('r')).unwrap();
    assert_eq!(text(&e), "fn f() {\n    x();\n}");
}

#[test]
fn pasted_text_is_inserted_literally() {
    let mut e = doc("m.rs", "fn f() {");
    append(&mut e);
    e.feed(Key::Text("\n}\n  {\n".into())).unwrap();
    assert_eq!(text(&e), "fn f() {\n}\n  {\n");
}

#[test]
fn unicode_lines_keep_valid_boundaries() {
    let mut e = doc("m.rs", "fn 猫() { // ключ {");
    append(&mut e);
    type_text(&mut e, "\nlet ж = \"{\";\n}");
    assert_eq!(text(&e), "fn 猫() { // ключ {\n    let ж = \"{\";\n}");
}

#[test]
fn open_line_below_and_above_use_indentation_rules() {
    let mut e = doc("m.rs", "fn f() {\n    a();\n}");
    e.feed(Key::Char('o')).unwrap();
    type_text(&mut e, "first();");
    e.feed(Key::Escape).unwrap();
    assert_eq!(text(&e), "fn f() {\n    first();\n    a();\n}");
    e.feed(Key::Char('j')).unwrap();
    e.feed(Key::Char('O')).unwrap();
    type_text(&mut e, "second();");
    e.feed(Key::Escape).unwrap();
    assert_eq!(
        text(&e),
        "fn f() {\n    first();\n    second();\n    a();\n}"
    );
}

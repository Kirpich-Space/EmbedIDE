//! Syntax highlighting: detection, per-language categories, multi-line state, Unicode, incremental cache.
use embedide_editor::{
    syntax::{highlight_line, Highlight, Highlighter, Language, LineState},
    vim::Key,
    Editor,
};
use std::path::Path;

/// Every highlighted token of `text` as (source text, category), line by line.
fn tokens(language: Language, text: &str) -> Vec<(String, Highlight)> {
    let mut state = LineState::Normal;
    let mut out = Vec::new();
    for line in text.split('\n') {
        let mut spans = Vec::new();
        state = highlight_line(language, line, &state, &mut spans);
        let mut last_end = 0;
        for span in spans {
            assert!(span.range.start >= last_end, "spans overlap in {line:?}");
            assert!(
                line.is_char_boundary(span.range.start) && line.is_char_boundary(span.range.end)
            );
            last_end = span.range.end;
            out.push((line[span.range].to_string(), span.kind));
        }
    }
    out
}

fn has(tokens: &[(String, Highlight)], text: &str, kind: Highlight) -> bool {
    tokens.iter().any(|(t, k)| t == text && *k == kind)
}

#[track_caller]
fn expect(language: Language, text: &str, expected: &[(&str, Highlight)]) {
    let found = tokens(language, text);
    for (token, kind) in expected {
        assert!(
            has(&found, token, *kind),
            "{language:?}: expected {token:?} as {kind:?}; got {found:?}"
        );
    }
}

use Highlight::{
    Comment, Constant, Function, Keyword, Number, Operator, Preprocessor, String as Str, Type,
    Variable,
};

#[test]
fn language_is_detected_from_extension_file_name_and_shebang() {
    let cases = [
        ("main.c", Language::C),
        ("drv.h", Language::C),
        ("main.cpp", Language::Cpp),
        ("x.hpp", Language::Cpp),
        ("main.rs", Language::Rust),
        ("main.zig", Language::Zig),
        ("startup.s", Language::Asm),
        ("boot.S", Language::Asm),
        ("simulation.f90", Language::Fortran),
        ("OLD.F", Language::Fortran),
        ("launch.sh", Language::Shell),
        ("script.py", Language::Python),
        ("app.js", Language::JavaScript),
        ("app.ts", Language::TypeScript),
        ("Cargo.toml", Language::Toml),
        ("config.json", Language::Json),
        ("Cargo.lock", Language::Toml),
        (".bashrc", Language::Shell),
        ("PKGBUILD", Language::Shell),
        ("notes.txt", Language::Plain),
    ];
    for (name, language) in cases {
        assert_eq!(
            Language::detect(Some(Path::new(name)), ""),
            language,
            "{name}"
        );
    }
    for (line, language) in [
        ("#!/bin/sh", Language::Shell),
        ("#!/usr/bin/env bash", Language::Shell),
        ("#!/usr/bin/env -S python3 -u", Language::Python),
        ("#!/usr/bin/python3.12", Language::Python),
        ("#!/usr/bin/env node", Language::JavaScript),
        ("#!/usr/bin/perl", Language::Plain),
        ("echo hi", Language::Plain),
    ] {
        assert_eq!(
            Language::detect(Some(Path::new("run")), line),
            language,
            "{line}"
        );
        assert_eq!(Language::detect(None, line), language, "{line}");
    }
    // The extension wins over a misleading shebang.
    assert_eq!(
        Language::detect(Some(Path::new("a.py")), "#!/bin/sh"),
        Language::Python
    );
}

#[test]
fn bash_script_is_not_rendered_as_plain_text() {
    let script = r#"#!/usr/bin/env bash
# Launch the firmware build
set -euo pipefail
BUILD_DIR="${1:-build}"
export PATH="$HOME/.local/bin:$PATH"
if [[ -d "$BUILD_DIR" ]]; then
    echo "Building in $BUILD_DIR" >&2
fi
for f in src/*.c; do gcc -c "$f" -o "${f%.c}.o"; done
count=$(ls | wc -l)
exit 0"#;
    let found = tokens(Language::Shell, script);
    expect(
        Language::Shell,
        script,
        &[
            ("#!/usr/bin/env bash", Preprocessor),
            ("# Launch the firmware build", Comment),
            ("set", Function),
            ("BUILD_DIR", Variable),
            ("${1:-build}", Variable),
            ("$HOME", Variable),
            ("if", Keyword),
            ("then", Keyword),
            ("fi", Keyword),
            ("for", Keyword),
            ("done", Keyword),
            ("echo", Function),
            ("gcc", Function),
            ("ls", Function),
            ("wc", Function),
            ("\"Building in ", Str),
            ("2", Number),
            ("0", Number),
        ],
    );
    let kinds: std::collections::BTreeSet<_> =
        found.iter().map(|(_, k)| format!("{k:?}")).collect();
    assert!(kinds.len() >= 7, "only {kinds:?}");
}

#[test]
fn shell_strings_comments_and_heredocs_span_lines() {
    let script = "cat <<-EOF\n\tbody $x # not a comment\n\tEOF\necho 'multi\nline' # tail\nurl=a#b";
    let found = tokens(Language::Shell, script);
    assert!(has(&found, "\tbody $x # not a comment", Str), "{found:?}");
    assert!(has(&found, "\tEOF", Str));
    assert!(has(&found, "line'", Str));
    assert!(has(&found, "# tail", Comment));
    assert!(
        !found.iter().any(|(t, k)| *k == Comment && t.contains("#b")),
        "{found:?}"
    );
}

#[test]
fn c_and_cpp_categories() {
    expect(
        Language::C,
        "#include <stdio.h>\n#define LED_PIN 13 /* on-board */\nstatic uint32_t counter = 0x1F;\nint main(void) {\n    char c = '\\n';\n    printf(\"%d\\n\", counter); // done\n    return NULL;\n}",
        &[
            ("#include", Preprocessor),
            ("<stdio.h>", Str),
            ("#define", Preprocessor),
            ("LED_PIN", Constant),
            ("13", Number),
            ("/* on-board */", Comment),
            ("static", Keyword),
            ("uint32_t", Type),
            ("0x1F", Number),
            ("int", Type),
            ("main", Function),
            ("'\\n'", Str),
            ("printf", Function),
            ("\"%d\\n\"", Str),
            ("// done", Comment),
            ("NULL", Constant),
            ("=", Operator),
        ],
    );
    expect(
        Language::Cpp,
        "namespace demo { class Engine final { public: virtual ~Engine() = default; }; }\nauto p = nullptr; std::vector<int> v{1'000};",
        &[
            ("namespace", Keyword),
            ("class", Keyword),
            ("Engine", Type),
            ("virtual", Keyword),
            ("nullptr", Constant),
            ("vector", Type),
            ("1'000", Number),
        ],
    );
}

#[test]
fn block_comments_continue_across_lines() {
    let found = tokens(Language::C, "a /* one\ntwo\nthree */ b = 1;");
    assert!(has(&found, "/* one", Comment));
    assert!(has(&found, "two", Comment));
    assert!(has(&found, "three */", Comment));
    assert!(has(&found, "1", Number));
    // Rust comments nest.
    let found = tokens(Language::Rust, "/* a /* b */ still\n*/ fn x() {}");
    assert!(has(&found, "/* a /* b */ still", Comment), "{found:?}");
    assert!(has(&found, "fn", Keyword));
}

#[test]
fn rust_categories() {
    expect(
        Language::Rust,
        "#[derive(Debug)]\nstruct Point<'a> { name: &'a str, x: f64 }\nfn main() {\n    let s = r#\"raw \"q\"\"#;\n    println!(\"{}\", MAX_SIZE);\n    let v: Vec<u8> = Vec::new(); let c = 'x';\n}",
        &[
            ("#[derive(Debug)]", Preprocessor),
            ("struct", Keyword),
            ("Point", Type),
            ("'a", Type),
            ("str", Type),
            ("f64", Type),
            ("fn", Keyword),
            ("main", Function),
            ("r#\"raw \"q\"\"#", Str),
            ("println!", Function),
            ("MAX_SIZE", Constant),
            ("Vec", Type),
            ("new", Function),
            ("'x'", Str),
        ],
    );
    let found = tokens(Language::Rust, "let s = \"multi\nline\"; 1..10");
    assert!(has(&found, "line\"", Str), "{found:?}");
    assert!(
        has(&found, "1", Number) && has(&found, "10", Number),
        "{found:?}"
    );
}

#[test]
fn zig_categories() {
    expect(
        Language::Zig,
        "const std = @import(\"std\");\npub fn main() !void {\n    const x: u32 = 42;\n    const s = \\\\multiline\n    ;\n}",
        &[
            ("const", Keyword),
            ("@import", Function),
            ("\"std\"", Str),
            ("pub", Keyword),
            ("main", Function),
            ("void", Type),
            ("u32", Type),
            ("42", Number),
        ],
    );
    let found = tokens(Language::Zig, "    \\\\ line string");
    assert!(has(&found, "\\\\ line string", Str), "{found:?}");
}

#[test]
fn assembly_categories() {
    expect(
        Language::Asm,
        "    .section .text\n    .global _start\n_start:             @ ARM entry\n    ldr  r0, =0x20001000\n    mov  sp, r0\n    bl   main       // call\n    movq $0x10, %rax  # x86\n#define STACK 0x100\n    add x0, x1, #4 ; nasm",
        &[
            (".section", Preprocessor),
            (".global", Preprocessor),
            ("_start:", Function),
            ("@ ARM entry", Comment),
            ("ldr", Keyword),
            ("r0", Variable),
            ("0x20001000", Number),
            ("sp", Variable),
            ("bl", Keyword),
            ("// call", Comment),
            ("$0x10", Number),
            ("%rax", Variable),
            ("# x86", Comment),
            ("#define", Preprocessor),
            ("STACK", Constant),
            ("x0", Variable),
            ("#4", Number),
            ("; nasm", Comment),
        ],
    );
}

#[test]
fn fortran_categories() {
    expect(
        Language::Fortran,
        "program simulation\n  implicit none\n  integer, parameter :: n = 100\n  real(8) :: x = 1.5d0\n  ! comment\n  IF (x > 0.0 .and. .TRUE.) THEN\n     call step(x, 'it''s')\n  end if\nend program simulation",
        &[
            ("program", Keyword),
            ("simulation", Function),
            ("implicit", Keyword),
            ("integer", Type),
            ("parameter", Keyword),
            ("100", Number),
            ("real", Type),
            ("1.5d0", Number),
            ("! comment", Comment),
            ("IF", Keyword),
            (".and.", Operator),
            (".TRUE.", Constant),
            ("THEN", Keyword),
            ("call", Keyword),
            ("step", Function),
            ("'it''s'", Str),
        ],
    );
}

#[test]
fn python_categories_and_triple_quoted_strings() {
    expect(
        Language::Python,
        "@dataclass\nclass Sensor:\n    def read(self, value: int) -> float:\n        \"\"\"Doc\n        string\"\"\"\n        return float(value) * 1.5  # scale\nprint(f\"value {x}\", None, rb'\\x00')",
        &[
            ("@dataclass", Preprocessor),
            ("class", Keyword),
            ("Sensor", Type),
            ("def", Keyword),
            ("read", Function),
            ("self", Variable),
            ("int", Type),
            ("\"\"\"Doc", Str),
            ("        string\"\"\"", Str),
            ("return", Keyword),
            ("1.5", Number),
            ("# scale", Comment),
            ("print", Function),
            ("f\"value {x}\"", Str),
            ("None", Constant),
            ("rb'\\x00'", Str),
        ],
    );
    let found = tokens(Language::Python, "x = f'''a\nb''' + r\"\"\"c\nd\"\"\"");
    assert!(has(&found, "b'''", Str), "{found:?}");
    assert!(has(&found, "d\"\"\"", Str), "{found:?}");
}

#[test]
fn javascript_and_typescript_categories() {
    expect(
        Language::TypeScript,
        "interface Config { port: number }\nconst app = createServer(`port\n${port}`);\nexport function start(cfg: Config): void { console.log('x', true, undefined); }",
        &[
            ("interface", Keyword),
            ("Config", Type),
            ("number", Type),
            ("const", Keyword),
            ("createServer", Function),
            ("`port", Str),
            ("${port}`", Str),
            ("export", Keyword),
            ("function", Keyword),
            ("start", Function),
            ("log", Function),
            ("'x'", Str),
            ("true", Constant),
            ("undefined", Constant),
        ],
    );
    expect(
        Language::JavaScript,
        "const $el = document.querySelector(\"#app\"); // ui\nclass View extends Base {}",
        &[
            ("const", Keyword),
            ("querySelector", Function),
            ("\"#app\"", Str),
            ("// ui", Comment),
            ("class", Keyword),
            ("View", Type),
            ("extends", Keyword),
        ],
    );
}

#[test]
fn toml_and_json_categories() {
    expect(
        Language::Toml,
        "[package]\nname = \"demo\" # comment\nedition = 2021\n[[bin]]\nserde = { version = \"1.0\", features = [\"derive\"] }\ndate = 1979-05-27T07:32:00Z\nok = true\ndesc = '''\nmulti\n'''",
        &[
            ("[package]", Type),
            ("name", Variable),
            ("=", Operator),
            ("\"demo\"", Str),
            ("# comment", Comment),
            ("2021", Number),
            ("[[bin]]", Type),
            ("version", Variable),
            ("features", Variable),
            ("\"derive\"", Str),
            ("1979-05-27T07:32:00Z", Constant),
            ("true", Constant),
            ("multi", Str),
        ],
    );
    expect(
        Language::Json,
        "{\"name\": \"demo\", \"port\": 8080, \"debug\": true,\n \"tags\": [null, -1.5e3]}",
        &[
            ("\"name\"", Variable),
            ("\"demo\"", Str),
            ("8080", Number),
            ("true", Constant),
            ("null", Constant),
            ("1.5e3", Number),
        ],
    );
}

#[test]
fn unicode_text_keeps_spans_on_char_boundaries() {
    for language in [
        Language::C,
        Language::Rust,
        Language::Python,
        Language::Shell,
        Language::Toml,
        Language::Asm,
        Language::Fortran,
    ] {
        // `tokens` asserts char boundaries and non-overlap for every span.
        let found = tokens(
            language,
            "имя = \"строка 猫\" # комментарий\nlet 변수 = 'é'; // ü\n$ПЕРЕМЕННАЯ 🙂 x",
        );
        assert!(!found.is_empty(), "{language:?}");
    }
    let found = tokens(Language::Rust, "let имя = \"строка\"; // коммент");
    assert!(has(&found, "\"строка\"", Str));
    assert!(has(&found, "// коммент", Comment));
}

#[test]
fn plain_text_and_unknown_files_have_no_spans() {
    assert!(tokens(Language::Plain, "int main() { return 0; }").is_empty());
}

fn line_kinds(highlighter: &mut Highlighter, editor: &Editor, line: usize) -> Vec<Highlight> {
    highlighter
        .spans(editor.state(), line)
        .into_iter()
        .map(|s| s.kind)
        .collect()
}

#[test]
fn highlighter_tracks_edits_and_undo_incrementally() {
    let dir = std::env::temp_dir().join(format!("embedide-syntax-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.c");
    std::fs::write(&path, "int a;\nint b;\nint c;\n").unwrap();
    let mut editor = Editor::open(&path).unwrap();
    let mut highlighter = Highlighter::default();
    assert_eq!(line_kinds(&mut highlighter, &editor, 2), vec![Type]);
    assert_eq!(highlighter.language(), Language::C);
    // Opening a block comment on line 0 turns the following lines into comments.
    editor.replace(0..0, "/* ").unwrap();
    assert_eq!(line_kinds(&mut highlighter, &editor, 2), vec![Comment]);
    assert_eq!(line_kinds(&mut highlighter, &editor, 1), vec![Comment]);
    // Closing it on line 1 restores line 2.
    let line1 = editor.state().line_range(1).unwrap();
    editor.replace(line1.end..line1.end, " */").unwrap();
    assert_eq!(line_kinds(&mut highlighter, &editor, 2), vec![Type]);
    // Undo through the engine (Vim `u`) is seen as a new revision.
    editor.feed(Key::Escape).unwrap();
    editor.feed(Key::Char('u')).unwrap();
    assert_eq!(line_kinds(&mut highlighter, &editor, 2), vec![Comment]);
    editor.feed(Key::Char('u')).unwrap();
    assert_eq!(line_kinds(&mut highlighter, &editor, 2), vec![Type]);
    // The file on disk is never touched by highlighting.
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "int a;\nint b;\nint c;\n"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn highlighter_handles_large_documents_and_scrolling() {
    let mut text = String::new();
    for i in 0..20_000 {
        text.push_str(&format!("let v{i} = {i}; // line {i}\n"));
    }
    let dir = std::env::temp_dir().join(format!("embedide-syntax-big-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("big.rs");
    std::fs::write(&path, &text).unwrap();
    let mut editor = Editor::open(&path).unwrap();
    let mut highlighter = Highlighter::default();
    let started = std::time::Instant::now();
    assert_eq!(
        line_kinds(&mut highlighter, &editor, 19_999),
        vec![Keyword, Operator, Number, Comment]
    );
    // Typing near the end revalidates by hash and re-lexes only from the edit.
    for _ in 0..50 {
        let range = editor.state().line_range(19_990).unwrap();
        editor.replace(range.start..range.start, " ").unwrap();
        assert_eq!(
            line_kinds(&mut highlighter, &editor, 19_995),
            vec![Keyword, Operator, Number, Comment]
        );
    }
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "highlighting took {:?}",
        started.elapsed()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn r_and_modern_fortran_are_detected_and_highlighted() {
    assert_eq!(
        Language::detect(Some(Path::new("analysis.R")), ""),
        Language::R
    );
    assert_eq!(
        Language::detect(Some(Path::new("report.Rmd")), ""),
        Language::R
    );
    assert_eq!(
        Language::detect(None, "#!/usr/bin/env Rscript"),
        Language::R
    );
    assert_eq!(Language::R.id(), "r");

    let r_found = tokens(
        Language::R,
        "f <- function(x) { if (is.na(x)) return(NULL); mean(x) } # comment",
    );
    assert!(has(&r_found, "function", Keyword));
    assert!(has(&r_found, "if", Keyword));
    assert!(has(&r_found, "return", Keyword));
    assert!(has(&r_found, "NULL", Constant));
    assert!(has(&r_found, "mean", Function));
    assert!(has(&r_found, "# comment", Comment));

    let f_found = tokens(
        Language::Fortran,
        "submodule (parent:anc) child\n  use, intrinsic :: iso_fortran_env, only: real64, output_unit\n  real(real64), contiguous :: x(:)\nend submodule child",
    );
    assert!(has(&f_found, "submodule", Keyword));
    assert!(has(&f_found, "contiguous", Keyword));
    assert!(has(&f_found, "real64", Type));
    assert!(has(&f_found, "output_unit", Constant));
}

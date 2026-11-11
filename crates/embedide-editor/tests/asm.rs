//! Assembly dialects: detection, dialect-specific lexing, labels, branches.
use embedide_editor::{
    asm::{labels, Dialect},
    syntax::{highlight_line_in, Highlight, Language, LineState},
};
use std::path::Path;

fn kinds(dialect: Dialect, line: &str) -> Vec<(String, Highlight)> {
    let mut spans = Vec::new();
    highlight_line_in(
        Language::Asm,
        Some(dialect),
        line,
        &LineState::Normal,
        &mut spans,
    );
    spans
        .into_iter()
        .map(|s| (line[s.range].to_string(), s.kind))
        .collect()
}

fn has(dialect: Dialect, line: &str, text: &str, kind: Highlight) -> bool {
    kinds(dialect, line)
        .iter()
        .any(|(t, k)| t == text && *k == kind)
}

#[test]
fn dialects_are_detected_from_extension_and_content() {
    fn p(s: &str) -> Option<&Path> {
        Some(Path::new(s))
    }
    assert_eq!(
        Dialect::detect(p("boot.asm"), "bits 64\nsection .text\n"),
        Dialect::Nasm
    );
    assert_eq!(Dialect::detect(p("x.inc"), "mov eax, 1"), Dialect::Nasm);
    assert_eq!(
        Dialect::detect(p("a.s"), ".text\nmovq $1, %rax\n"),
        Dialect::GasAtt
    );
    assert_eq!(
        Dialect::detect(p("a.s"), ""),
        Dialect::GasAtt,
        "GNU as default"
    );
    assert_eq!(
        Dialect::detect(p("a.S"), ".intel_syntax noprefix\nmov rax, 1\n"),
        Dialect::GasIntel
    );
    assert_eq!(
        Dialect::detect(
            p("start.S"),
            ".global _start\n_start:\n  mov x0, #0\n  mov x8, #93\n  svc #0\n"
        ),
        Dialect::Aarch64
    );
    assert_eq!(
        Dialect::detect(
            p("vec.s"),
            ".syntax unified\n.thumb\nreset:\n  ldr r0, =1\n  bx lr\n"
        ),
        Dialect::Arm
    );
    assert_eq!(
        Dialect::detect(p("m.s"), "_start:\n  li a7, 93\n  li a0, 0\n  ecall\n"),
        Dialect::RiscV
    );
    assert_eq!(Dialect::parse("gas-intel"), Some(Dialect::GasIntel));
}

#[test]
fn comment_characters_follow_the_dialect() {
    // NASM: `;` comments; `#` is not one.
    assert!(has(
        Dialect::Nasm,
        "mov eax, 1 ; set",
        "; set",
        Highlight::Comment
    ));
    assert!(has(
        Dialect::Nasm,
        "%define N 4",
        "%define",
        Highlight::Preprocessor
    ));
    // GNU as x86: `#` comments, `;` separates two statements, `@function` is not a comment.
    assert!(has(
        Dialect::GasAtt,
        "movq $1, %rax # one",
        "# one",
        Highlight::Comment
    ));
    let two = kinds(Dialect::GasAtt, "nop; ret");
    assert!(
        two.contains(&("nop".into(), Highlight::Keyword))
            && two.contains(&("ret".into(), Highlight::Branch)),
        "{two:?}"
    );
    assert!(!kinds(Dialect::GasAtt, ".type main, @function")
        .iter()
        .any(|(_, k)| *k == Highlight::Comment));
    assert!(has(
        Dialect::GasAtt,
        "movq $1, %rax",
        "%rax",
        Highlight::Variable
    ));
    // ARM: `@` comments, `#` immediates.
    assert!(has(
        Dialect::Arm,
        "mov r0, #1 @ one",
        "@ one",
        Highlight::Comment
    ));
    assert!(!kinds(Dialect::Arm, "mov r0, #1")
        .iter()
        .any(|(_, k)| *k == Highlight::Comment));
    // AArch64: `//` comments, `#` immediates.
    assert!(has(
        Dialect::Aarch64,
        "mov x0, #0 // exit code",
        "// exit code",
        Highlight::Comment
    ));
    assert!(has(
        Dialect::Aarch64,
        "mov x0, #0",
        "x0",
        Highlight::Variable
    ));
    // RISC-V: `#` comments.
    assert!(has(
        Dialect::RiscV,
        "li a0, 0 # status",
        "# status",
        Highlight::Comment
    ));
}

#[test]
fn branches_and_their_targets_stand_out() {
    assert!(has(
        Dialect::Nasm,
        "    jne .loop",
        "jne",
        Highlight::Branch
    ));
    assert!(has(
        Dialect::Nasm,
        "    jne .loop",
        ".loop",
        Highlight::Function
    ));
    assert!(has(
        Dialect::Nasm,
        "    call print",
        "print",
        Highlight::Function
    ));
    assert!(has(
        Dialect::Nasm,
        "    mov eax, ebx",
        "mov",
        Highlight::Keyword
    ));
    assert!(has(
        Dialect::GasAtt,
        "    callq puts",
        "callq",
        Highlight::Branch
    ));
    assert!(has(
        Dialect::Aarch64,
        "    b.ne loop",
        "b.ne",
        Highlight::Branch
    ));
    assert!(has(
        Dialect::Aarch64,
        "    cbz x0, done",
        "done",
        Highlight::Function
    ));
    assert!(has(Dialect::Arm, "    bne loop", "bne", Highlight::Branch));
    assert!(has(
        Dialect::RiscV,
        "    beqz a0, done",
        "beqz",
        Highlight::Branch
    ));
    assert!(!Dialect::Arm.is_branch("bic"));
    assert!(Dialect::GasAtt.is_branch("jmp"));
}

#[test]
fn labels_are_found_outside_comments() {
    let text = "section .text\nglobal _start\n_start:\n.loop:  dec ecx ; x: not a label\n    jnz .loop\n1: ret\n; fake: no\n";
    let found: Vec<(String, usize)> = labels(text, Dialect::Nasm)
        .into_iter()
        .map(|l| (l.name, l.line))
        .collect();
    assert_eq!(
        found,
        [("_start".into(), 2), (".loop".into(), 3), ("1".into(), 5)]
    );
    let gas = "main: # entry\n  movq $0, %rax # rax: zero\nend:\n";
    let names: Vec<String> = labels(gas, Dialect::GasAtt)
        .into_iter()
        .map(|l| l.name)
        .collect();
    assert_eq!(names, ["main", "end"]);
}

#[test]
fn completion_sets_match_the_dialect() {
    assert!(
        Dialect::Nasm.mnemonics().contains(&"mov") && Dialect::GasAtt.mnemonics().contains(&"movq")
    );
    assert!(
        Dialect::Nasm.directives().contains(&"section")
            && Dialect::GasAtt.directives().contains(&".globl")
    );
    assert!(Dialect::Aarch64.registers().contains(&"x29".to_string()));
    assert!(Dialect::RiscV.registers().contains(&"a7".to_string()));
    assert!(Dialect::GasAtt.registers().contains(&"r10d".to_string()));
    assert!(!Dialect::Arm.registers().contains(&"rax".to_string()));
}

#[test]
fn the_example_project_files_are_detected_correctly() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/asm");
    for (file, dialect) in [
        ("hello-nasm.asm", Dialect::Nasm),
        ("hello-att.s", Dialect::GasAtt),
        ("hello-intel.s", Dialect::GasIntel),
        ("hello-aarch64.s", Dialect::Aarch64),
        ("hello-riscv.s", Dialect::RiscV),
    ] {
        let path = dir.join(file);
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(Dialect::detect(Some(&path), &text), dialect, "{file}");
    }
}

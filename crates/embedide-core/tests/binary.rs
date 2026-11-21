//! Binary documents, ELF inspection and disassembly on disposable fixtures.
use embedide_core::{
    binary::{looks_binary, parse_offset, parse_pattern, BinaryFile},
    disasm, elf,
};
use std::{
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "embed bin ж {}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 300 KiB of every byte value, so pages (64 KiB) and NULs are crossed.
fn pattern_bytes() -> Vec<u8> {
    (0..300 * 1024).map(|i| (i * 7 % 256) as u8).collect()
}

#[test]
fn reading_is_paged_and_never_changes_the_file() {
    let d = Dir::new();
    let data = pattern_bytes();
    let path = d.file("blob.bin", &data);
    let mut f = BinaryFile::open(&path).unwrap();
    assert_eq!(f.len(), data.len() as u64);
    assert_eq!(f.read(0, 16).unwrap(), data[..16]);
    let edge = 64 * 1024 - 3;
    assert_eq!(
        f.read(edge as u64, 10).unwrap(),
        data[edge..edge + 10],
        "across a page"
    );
    assert_eq!(
        f.read(data.len() as u64 - 4, 100).unwrap(),
        data[data.len() - 4..],
        "short at the end"
    );
    assert!(f.read(data.len() as u64 + 5, 4).unwrap().is_empty());
    drop(f);
    assert_eq!(std::fs::read(&path).unwrap(), data);
    assert!(looks_binary(&path));
    assert!(!looks_binary(&d.file("t.txt", "plain ж text\n".as_bytes())));
    assert!(looks_binary(&d.file("bad.txt", b"bad \xff\xfe utf-8")));
}

#[test]
fn edits_undo_redo_and_save_byte_for_byte() {
    let d = Dir::new();
    let data = pattern_bytes();
    let path = d.file("fw.bin", &data);
    let mut f = BinaryFile::open(&path).unwrap();
    f.set(0, 0xAA).unwrap();
    f.set(100_000, 0xBB).unwrap();
    f.set(100_000, 0xCC).unwrap();
    f.set(5, data[5]).unwrap(); // same value: not a change
    assert_eq!(f.changed(), 2);
    assert_eq!(f.byte(100_000).unwrap(), Some(0xCC));
    assert_eq!(
        std::fs::read(&path).unwrap(),
        data,
        "nothing written before save"
    );
    assert_eq!(f.undo().unwrap(), Some(100_000));
    assert_eq!(f.byte(100_000).unwrap(), Some(0xBB));
    assert_eq!(f.undo().unwrap(), Some(100_000));
    assert_eq!(f.byte(100_000).unwrap(), Some(data[100_000]));
    assert!(!f.is_edited(100_000));
    assert_eq!(f.redo().unwrap(), Some(100_000));
    assert!(f.set(data.len() as u64, 1).is_err(), "the file never grows");
    f.save().unwrap();
    assert!(!f.is_modified());
    let mut expected = data.clone();
    expected[0] = 0xAA;
    expected[100_000] = 0xBB;
    let saved = std::fs::read(&path).unwrap();
    assert_eq!(saved.len(), data.len());
    assert_eq!(saved, expected, "only the edited bytes differ");
    let leftovers: Vec<_> = std::fs::read_dir(&d.0)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().contains(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "temporary file left behind");
    // Reopen: the edits are in the file now.
    let mut again = BinaryFile::open(&path).unwrap();
    assert_eq!(again.read(0, 1).unwrap(), [0xAA]);
}

#[cfg(unix)]
#[test]
fn saving_refuses_when_the_file_changed_and_keeps_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let d = Dir::new();
    let path = d.file("tool", &[1, 2, 3, 4]);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o751)).unwrap();
    let mut f = BinaryFile::open(&path).unwrap();
    f.set(1, 9).unwrap();
    f.save().unwrap();
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o751, "permissions preserved");
    f.set(2, 8).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(&path, [7, 7, 7, 7, 7]).unwrap(); // someone else rewrote it
    let error = f.save().unwrap_err().to_string();
    assert!(error.contains("changed on disk"), "{error}");
    assert_eq!(
        std::fs::read(&path).unwrap(),
        [7, 7, 7, 7, 7],
        "the other change is kept"
    );
}

#[test]
fn search_finds_hex_and_text_across_pages_and_wraps() {
    let d = Dir::new();
    let mut data = vec![0u8; 200 * 1024];
    data[65_534..65_537].copy_from_slice(b"ELF"); // straddles the first page boundary
    data[150_000..150_003].copy_from_slice(&[0xde, 0xad, 0xbe]);
    let path = d.file("img", &data);
    let mut f = BinaryFile::open(&path).unwrap();
    assert_eq!(
        f.find(&parse_pattern("\"ELF\"").unwrap(), 0).unwrap(),
        Some(65_534)
    );
    assert_eq!(
        f.find(&parse_pattern("de ad be").unwrap(), 70_000).unwrap(),
        Some(150_000)
    );
    assert_eq!(
        f.find(&parse_pattern("0xdeadbe").unwrap(), 150_001)
            .unwrap(),
        Some(150_000),
        "wraps around"
    );
    f.set(10, 0x7f).unwrap();
    assert_eq!(f.find(&[0x7f], 0).unwrap(), Some(10), "edits are searched");
    assert_eq!(f.find(b"absent", 0).unwrap(), None);
    assert_eq!(parse_pattern("\"a\\x00\\n\"").unwrap(), [b'a', 0, b'\n']);
    assert!(parse_pattern("7g").is_err() && parse_pattern("abc").is_err());
    assert_eq!(parse_offset("0x1f00").unwrap(), 0x1f00);
    assert_eq!(parse_offset("1f00h").unwrap(), 0x1f00);
    assert_eq!(parse_offset("7936").unwrap(), 7936);
    assert!(parse_offset("zz").is_err());
}

fn have(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// A small C program built with GCC (skipped without gcc).
fn gcc_binary(d: &Dir) -> Option<PathBuf> {
    if !have("gcc") {
        return None;
    }
    let src = d.file(
        "main.c",
        b"int helper(int x) { return x * 3; }\nint main(void) { return helper(2) - 6; }\n",
    );
    let out = d.0.join("prog");
    let ok = Command::new("gcc")
        .args(["-O0", "-g", "-o"])
        .arg(&out)
        .arg(&src)
        .status()
        .unwrap()
        .success();
    ok.then_some(out)
}

#[test]
fn elf64_executables_show_headers_sections_and_symbols() {
    let d = Dir::new();
    let Some(path) = gcc_binary(&d) else { return };
    let e = elf::read(&path).unwrap();
    assert_eq!((e.class, e.little), (64, true));
    assert!(
        e.machine == "x86-64" || e.machine == "AArch64",
        "{}",
        e.machine
    );
    assert!(e.kind.starts_with("DYN") || e.kind.starts_with("EXEC"));
    assert!(e.entry != 0);
    assert!(e
        .segments
        .iter()
        .any(|s| s.kind == "LOAD" && s.flags.contains('X')));
    let text = e.sections.iter().find(|s| s.name == ".text").unwrap();
    assert!(text.executable() && text.size > 0);
    let main = e.symbol("main").unwrap();
    assert_eq!(main.kind, "FUNC");
    assert_eq!(
        e.section_at(main.value).map(|s| s.name.as_str()),
        Some(".text")
    );
    assert!(e.file_offset(main.value).is_some());
    assert!(e.symbol("helper").is_some());
    // Inspection never changes the file.
    let before = std::fs::read(&path).unwrap();
    let _ = elf::read(&path).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn elf32_and_big_endian_objects_parse() {
    if !have("llvm-mc") {
        return;
    }
    let d = Dir::new();
    let src = d.file("t.s", b".text\n.globl start\nstart:\n nop\n");
    for (triple, class, little, machine) in [
        ("i386-linux-gnu", 32, true, "x86 (i386)"),
        ("powerpc64-linux-gnu", 64, false, "PowerPC64"),
        ("aarch64-linux-gnu", 64, true, "AArch64"),
        ("riscv64-linux-gnu", 64, true, "RISC-V"),
    ] {
        let obj = d.0.join(format!("{triple}.o"));
        let ok = Command::new("llvm-mc")
            .arg(format!("-triple={triple}"))
            .args(["-filetype=obj", "-o"])
            .arg(&obj)
            .arg(&src)
            .status()
            .unwrap()
            .success();
        if !ok {
            continue; // target not built into this LLVM
        }
        let e = elf::read(&obj).unwrap();
        assert_eq!(
            (e.class, e.little, e.machine.as_str()),
            (class, little, machine),
            "{triple}"
        );
        assert!(e.kind.starts_with("REL"));
        assert!(
            e.symbols.iter().any(|s| s.name == "start"),
            "{triple}: {:?}",
            e.symbols
        );
    }
}

#[test]
fn malformed_and_truncated_files_never_panic() {
    assert!(elf::parse(b"").is_err());
    assert!(elf::parse(b"MZ\x90\x00").is_err());
    assert!(elf::parse(b"\x7fELF\x09\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00").is_err());
    let d = Dir::new();
    let Some(path) = gcc_binary(&d) else { return };
    let data = std::fs::read(&path).unwrap();
    for cut in (0..data.len()).step_by(97) {
        let _ = elf::parse(&data[..cut]); // errors or partial tables, never a panic
    }
    // Garbage header fields: absurd table offsets and counts.
    let mut bad = data.clone();
    bad[0x28..0x30].copy_from_slice(&u64::MAX.to_le_bytes()); // e_shoff
    bad[0x3c..0x3e].copy_from_slice(&u16::MAX.to_le_bytes()); // e_shnum
    let e = elf::parse(&bad).unwrap();
    assert!(
        e.sections.is_empty() && !e.notes.is_empty(),
        "{:?}",
        e.notes
    );
}

#[test]
fn disassembly_lists_symbols_and_instructions_from_objdump() {
    let sample = "\nprog:     file format elf64-x86-64\n\nDisassembly of section .text:\n\n0000000000001129 <main>:\n    1129:\t55                   \tpush   %rbp\n    112a:\t48 89 e5             \tmov    %rsp,%rbp\n    112d:\tc3                   \tret\n";
    let lines = disasm::parse(sample);
    assert_eq!(lines[0].kind, disasm::Kind::Section);
    assert_eq!(
        (lines[1].kind, lines[1].addr, lines[1].text.as_str()),
        (disasm::Kind::Symbol, Some(0x1129), "main")
    );
    assert_eq!(
        (
            lines[3].addr,
            lines[3].bytes.as_str(),
            lines[3].text.as_str()
        ),
        (Some(0x112a), "48 89 e5", "mov    %rsp,%rbp")
    );
    // llvm-objdump puts a tab between mnemonic and operands.
    let llvm = disasm::parse("  401000: 48 c7 c0 01 00 00 00         \tmovq\t$0x1, %rax\n");
    assert_eq!(
        (llvm[0].bytes.as_str(), llvm[0].text.as_str()),
        ("48 c7 c0 01 00 00 00", "movq $0x1, %rax")
    );
    let d = Dir::new();
    let Some(path) = gcc_binary(&d) else { return };
    let e = elf::read(&path).unwrap();
    let Ok((program, args)) = disasm::command(&e, &path, true, None, None) else {
        return;
    };
    let listing = disasm::disassemble(&program, &args).unwrap();
    let main = listing
        .lines
        .iter()
        .position(|l| l.kind == disasm::Kind::Symbol && l.text == "main")
        .unwrap();
    assert_eq!(
        listing.lines[main].addr,
        Some(e.symbol("main").unwrap().value),
        "objdump and the ELF reader agree"
    );
    assert!(listing.lines[main + 1..]
        .iter()
        .take(3)
        .all(|l| l.kind == disasm::Kind::Instruction && !l.bytes.is_empty()));
    assert!(listing.lines.iter().any(|l| l.text.starts_with("push")
        || l.text.starts_with("endbr64")
        || l.text.starts_with("stp")));
    assert!(
        listing.tool.contains("-d"),
        "tool label preserves flags: {}",
        listing.tool
    );
}

#[test]
fn elf_overflow_offsets_short_disasm_args_and_utf8_pattern_escapes_never_panic() {
    // Crafted ELF with extreme addresses/offsets must never overflow in section_at/file_offset.
    let mut e = elf::Elf {
        class: 64,
        little: true,
        kind: "EXEC".into(),
        machine: "x86-64".into(),
        machine_id: 62,
        osabi: 0,
        entry: 0,
        segments: vec![elf::Segment {
            kind: "LOAD".into(),
            offset: u64::MAX - 2,
            vaddr: u64::MAX - 10,
            filesz: 100,
            memsz: 100,
            flags: "R-X".into(),
        }],
        sections: vec![elf::Section {
            name: ".text".into(),
            kind: "PROGBITS".into(),
            flags: "AX".into(),
            addr: u64::MAX - 4,
            offset: 0,
            size: 100,
        }],
        symbols: Vec::new(),
        notes: Vec::new(),
    };
    assert!(e.section_at(u64::MAX - 1).is_some());
    assert_eq!(e.file_offset(u64::MAX - 1), None);
    e.segments[0].offset = 0x100;
    assert_eq!(e.file_offset(u64::MAX - 10), Some(0x100));

    // disassemble with fewer than 2 arguments must never underflow/panic.
    let listing = disasm::disassemble(std::path::Path::new("true"), &[]).unwrap();
    assert_eq!(listing.tool, "true");

    // Non-ASCII escaped character in binary text pattern keeps full UTF-8 bytes.
    assert_eq!(
        embedide_core::binary::parse_pattern(r#""\ж""#).unwrap(),
        "ж".as_bytes()
    );
}

#[test]
fn builtin_disassembler_decodes_x86_64_elf_in_att_and_intel_without_external_tools() {
    let d = Dir::new();
    let Some(path) = gcc_binary(&d) else { return };
    let e = elf::read(&path).unwrap();
    if !disasm::supports_builtin(&e) {
        return;
    }
    let att = disasm::disassemble_builtin(&e, &path, false).unwrap();
    assert!(att.tool.contains("built-in"));
    let main_idx = att
        .lines
        .iter()
        .position(|l| l.kind == disasm::Kind::Symbol && l.text == "main")
        .expect("main symbol");
    assert_eq!(
        att.lines[main_idx].addr,
        Some(e.symbol("main").unwrap().value)
    );
    let att_body: Vec<&disasm::Line> = att.lines[main_idx + 1..].iter().take(6).collect();
    assert!(att_body
        .iter()
        .all(|l| l.kind == disasm::Kind::Instruction && !l.bytes.is_empty()));
    assert!(
        att_body.iter().any(|l| l.text.contains('%')),
        "AT&T syntax uses % registers: {att_body:?}"
    );

    let intel = disasm::disassemble_builtin(&e, &path, true).unwrap();
    assert!(intel.tool.contains("intel"));
    let intel_main = intel
        .lines
        .iter()
        .position(|l| l.kind == disasm::Kind::Symbol && l.text == "main")
        .expect("main symbol in intel");
    assert_eq!(intel.lines[intel_main].addr, att.lines[main_idx].addr);
    let intel_body: Vec<&disasm::Line> = intel.lines[intel_main + 1..].iter().take(6).collect();
    assert!(
        intel_body.iter().all(|l| !l.text.contains('%')),
        "Intel syntax omits % registers: {intel_body:?}"
    );

    // ET_REL (.o) object file: symbol at offset 0 must still be emitted and scoped to its section.
    let src = d.0.join("obj.c");
    let obj = d.0.join("obj.o");
    std::fs::write(&src, "int first_fn(int x) { return x + 1; }\n").unwrap();
    let status = Command::new("gcc")
        .args(["-O0", "-c", "-o"])
        .arg(&obj)
        .arg(&src)
        .status();
    if status.is_ok_and(|s| s.success()) {
        let rel = elf::read(&obj).unwrap();
        let listing = disasm::disassemble_builtin(&rel, &obj, true).unwrap();
        let sym = listing
            .lines
            .iter()
            .find(|l| l.kind == disasm::Kind::Symbol && l.text == "first_fn")
            .expect("first_fn at offset 0 in ET_REL");
        assert_eq!(sym.addr, Some(0));
    }

    // Crafted ELF whose section address wraps past u64::MAX must never panic.
    let raw = d.0.join("wrap.bin");
    std::fs::write(&raw, [0x90u8; 16]).unwrap();
    let crafted = elf::Elf {
        class: 64,
        little: true,
        kind: "EXEC".into(),
        machine: "x86-64".into(),
        machine_id: 62,
        osabi: 0,
        entry: 0,
        segments: Vec::new(),
        sections: vec![elf::Section {
            name: ".text".into(),
            kind: "PROGBITS".into(),
            flags: "AX".into(),
            addr: u64::MAX - 4,
            offset: 0,
            size: 16,
        }],
        symbols: Vec::new(),
        notes: Vec::new(),
    };
    let wrapped = disasm::disassemble_builtin(&crafted, &raw, true).unwrap();
    assert!(wrapped
        .lines
        .iter()
        .any(|l| l.kind == disasm::Kind::Instruction && l.text == "nop"));
}

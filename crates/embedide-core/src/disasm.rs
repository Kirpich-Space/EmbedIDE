//! Disassembly through an installed disassembler: GNU `objdump` for x86
//! binaries, `llvm-objdump` for other architectures (it reads the machine
//! from the ELF header), each falling back to the other. The binary is only
//! read, never executed. Output is parsed into addresses, instruction bytes
//! and text, with symbol and section headers; source mappings are never
//! invented (stripped binaries simply have no symbols).
use crate::{elf::Elf, Error, Result};
use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

/// Output accepted from the disassembler.
const MAX_OUTPUT: usize = 64 * 1024 * 1024;
/// Lines kept.
const MAX_LINES: usize = 1_000_000;
/// Time allowed.
const TIMEOUT: Duration = Duration::from_secs(120);

/// What a line of the listing is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `Disassembly of section .text:`.
    Section,
    /// `0000000000401136 <main>:`.
    Symbol,
    /// An instruction.
    Instruction,
}

/// One listing line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// Line kind.
    pub kind: Kind,
    /// Address (instructions and symbols).
    pub addr: Option<u64>,
    /// Instruction bytes as printed (`48 89 e5`).
    pub bytes: String,
    /// Mnemonic and operands, symbol name or section name.
    pub text: String,
}

/// A parsed listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    /// Disassembler command used.
    pub tool: String,
    /// Lines in address order.
    pub lines: Vec<Line>,
    /// The listing was cut at the size limit.
    pub truncated: bool,
}

fn on_path(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

/// The disassembler and arguments for this binary; `intel` selects Intel
/// syntax on x86. `objdump`/`llvm_objdump` override the programs.
pub fn command(
    elf: &Elf,
    path: &Path,
    intel: bool,
    objdump: Option<&str>,
    llvm: Option<&str>,
) -> Result<(PathBuf, Vec<String>)> {
    let x86 = matches!(elf.machine_id, 3 | 62);
    let gnu = objdump.map(PathBuf::from).or_else(|| on_path("objdump"));
    let llvm = llvm.map(PathBuf::from).or_else(|| on_path("llvm-objdump"));
    let order = if x86 { [gnu, llvm] } else { [llvm, gnu] };
    let program = order.into_iter().flatten().next().ok_or_else(|| {
        Error("No disassembler found: install binutils (objdump) or LLVM (llvm-objdump)".into())
    })?;
    let is_llvm = program
        .file_name()
        .is_some_and(|n| n.to_string_lossy().contains("llvm"));
    let mut args = vec!["-d".to_string()];
    if is_llvm {
        if x86 && intel {
            args.push("--x86-asm-syntax=intel".into());
        }
    } else {
        args.push("-w".into());
        if x86 && intel {
            args.extend(["-M".into(), "intel".into()]);
        }
    }
    args.push("--".into());
    args.push(path.display().to_string());
    Ok((program, args))
}

/// Run the disassembler (bounded in time and size) and parse its output.
pub fn disassemble(program: &Path, args: &[String]) -> Result<Listing> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error(format!("Cannot run {}: {e}", program.display())))?;
    let mut out = child.stdout.take().expect("piped");
    let mut err = child.stderr.take().expect("piped");
    let reader = thread::spawn(move || {
        let mut data = Vec::new();
        let _ = (&mut out)
            .take(MAX_OUTPUT as u64 + 1)
            .read_to_end(&mut data);
        data
    });
    let err_reader = thread::spawn(move || {
        let mut data = Vec::new();
        let _ = (&mut err).take(16 * 1024).read_to_end(&mut data);
        data
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() < TIMEOUT => thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error(format!(
                    "{} took too long; stopped",
                    program.display()
                )));
            }
        }
    };
    let data = reader.join().unwrap_or_default();
    let stderr = String::from_utf8_lossy(&err_reader.join().unwrap_or_default())
        .trim()
        .to_string();
    if !status.success() {
        return Err(Error(format!(
            "{} failed: {}",
            program.display(),
            if stderr.is_empty() {
                "no message".into()
            } else {
                stderr
            }
        )));
    }
    let truncated = data.len() > MAX_OUTPUT;
    let text = String::from_utf8_lossy(&data[..data.len().min(MAX_OUTPUT)]);
    let mut lines = parse(&text);
    let truncated = truncated || lines.len() > MAX_LINES;
    lines.truncate(MAX_LINES);
    Ok(Listing {
        tool: format!("{} {}", program.display(), args[..args.len() - 2].join(" ")),
        lines,
        truncated,
    })
}

/// Parse GNU objdump / llvm-objdump `-d` output.
pub fn parse(text: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    for raw in text.lines() {
        let line = raw.trim_end();
        if let Some(name) = line
            .strip_prefix("Disassembly of section ")
            .and_then(|l| l.strip_suffix(':'))
        {
            lines.push(Line {
                kind: Kind::Section,
                addr: None,
                bytes: String::new(),
                text: name.to_string(),
            });
            continue;
        }
        // `0000000000401136 <main>:`
        if let Some((addr, rest)) = line.split_once(" <") {
            if let (Ok(addr), Some(name)) = (
                u64::from_str_radix(addr.trim(), 16),
                rest.strip_suffix(">:"),
            ) {
                lines.push(Line {
                    kind: Kind::Symbol,
                    addr: Some(addr),
                    bytes: String::new(),
                    text: name.to_string(),
                });
                continue;
            }
        }
        // `  401136:\t55                   \tpush   %rbp`
        let trimmed = line.trim_start();
        let Some((addr, rest)) = trimmed.split_once(':') else {
            continue;
        };
        let Ok(addr) = u64::from_str_radix(addr, 16) else {
            continue;
        };
        let mut fields = rest.split('\t').map(str::trim).filter(|f| !f.is_empty());
        let Some(first) = fields.next() else { continue };
        let is_bytes = |f: &str| {
            f.split(' ')
                .filter(|w| !w.is_empty())
                .all(|w| w.len() == 2 && w.bytes().all(|b| b.is_ascii_hexdigit()))
        };
        let (bytes, text) = if is_bytes(first) {
            (
                first.split_whitespace().collect::<Vec<_>>().join(" "),
                fields.collect::<Vec<_>>().join(" "),
            )
        } else {
            (
                String::new(),
                std::iter::once(first)
                    .chain(fields)
                    .collect::<Vec<_>>()
                    .join(" "),
            )
        };
        lines.push(Line {
            kind: Kind::Instruction,
            addr: Some(addr),
            bytes,
            text,
        });
    }
    lines
}

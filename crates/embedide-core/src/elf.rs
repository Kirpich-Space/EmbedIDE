//! A small, bounds-checked ELF reader (ELF32/ELF64, little and big endian):
//! file header, program headers, section headers with names, and symbols
//! from `.symtab` / `.dynsym`. Nothing is executed or mapped; every offset
//! and count from the file is checked, so truncated or malformed files give
//! an error (or partial tables with notes), never a panic.
//!
//! A full library (e.g. `object`) was not needed for this read-only view;
//! the reader is ~300 lines with no dependencies.
use crate::{Error, Result};
use std::{
    cell::RefCell,
    collections::HashMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

/// Headers and tables larger than this are not read.
const MAX_TABLE: u64 = 64 * 1024 * 1024;
/// Symbols listed at most.
const MAX_SYMBOLS: usize = 200_000;

/// One program header (segment).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// `LOAD`, `DYNAMIC`, `INTERP`, … or the number.
    pub kind: String,
    /// File offset.
    pub offset: u64,
    /// Virtual address.
    pub vaddr: u64,
    /// Bytes in the file.
    pub filesz: u64,
    /// Bytes in memory.
    pub memsz: u64,
    /// `R`, `W`, `X` flags.
    pub flags: String,
}

/// One section header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// Name from the section-name string table.
    pub name: String,
    /// `PROGBITS`, `NOBITS`, `SYMTAB`, … or the number.
    pub kind: String,
    /// `A` (alloc), `W` (write), `X` (exec) flags.
    pub flags: String,
    /// Virtual address.
    pub addr: u64,
    /// File offset.
    pub offset: u64,
    /// Size in bytes.
    pub size: u64,
}

impl Section {
    /// Contains executable code.
    pub fn executable(&self) -> bool {
        self.flags.contains('X')
    }
}

/// One symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    /// Name (may be empty).
    pub name: String,
    /// Value (usually an address).
    pub value: u64,
    /// Size in bytes.
    pub size: u64,
    /// `FUNC`, `OBJECT`, `SECTION`, `FILE`, `NOTYPE`, …
    pub kind: String,
    /// `LOCAL`, `GLOBAL`, `WEAK`.
    pub bind: String,
    /// Index of the section it is defined in (0: undefined).
    pub section: u16,
    /// From `.dynsym` (otherwise `.symtab`).
    pub dynamic: bool,
}

/// What the inspector shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Elf {
    /// 32 or 64.
    pub class: u8,
    /// Little endian.
    pub little: bool,
    /// `EXEC`, `DYN`, `REL`, `CORE` or the number.
    pub kind: String,
    /// Machine name (`x86-64`, `AArch64`, `RISC-V`, `ARM`, …).
    pub machine: String,
    /// Raw `e_machine`.
    pub machine_id: u16,
    /// OS/ABI byte.
    pub osabi: u8,
    /// Entry point address.
    pub entry: u64,
    /// Program headers.
    pub segments: Vec<Segment>,
    /// Section headers.
    pub sections: Vec<Section>,
    /// Symbols (both tables), sorted by address.
    pub symbols: Vec<Symbol>,
    /// Problems found while reading (truncated tables and the like).
    pub notes: Vec<String>,
}

/// Machine name for `e_machine`.
pub fn machine_name(id: u16) -> String {
    match id {
        3 => "x86 (i386)".into(),
        8 => "MIPS".into(),
        20 => "PowerPC".into(),
        21 => "PowerPC64".into(),
        40 => "ARM".into(),
        50 => "IA-64".into(),
        62 => "x86-64".into(),
        83 => "AVR".into(),
        105 => "MSP430".into(),
        183 => "AArch64".into(),
        243 => "RISC-V".into(),
        258 => "LoongArch".into(),
        n => format!("machine {n}"),
    }
}

/// Random access to the file's bytes.
trait Source {
    /// Total length.
    fn len(&self) -> u64;
    /// Fill `buf` from `at`; false when out of range or unreadable.
    fn read_at(&self, at: u64, buf: &mut [u8]) -> bool;
}

struct Slice<'a>(&'a [u8]);

impl Source for Slice<'_> {
    fn len(&self) -> u64 {
        self.0.len() as u64
    }
    fn read_at(&self, at: u64, buf: &mut [u8]) -> bool {
        let Some(at) = usize::try_from(at).ok() else {
            return false;
        };
        match at
            .checked_add(buf.len())
            .and_then(|end| self.0.get(at..end))
        {
            Some(src) => {
                buf.copy_from_slice(src);
                true
            }
            None => false,
        }
    }
}

/// A file read in 64 KiB pages on demand (at most 1024 pages cached), so
/// inspecting a large binary reads only its headers, tables and strings.
struct FileSource {
    file: RefCell<File>,
    len: u64,
    pages: RefCell<HashMap<u64, Vec<u8>>>,
}

impl FileSource {
    const PAGE: u64 = 64 * 1024;
    fn page(&self, index: u64) -> Option<Vec<u8>> {
        if let Some(p) = self.pages.borrow().get(&index) {
            return Some(p.clone());
        }
        let start = index * Self::PAGE;
        let size = Self::PAGE.min(self.len.checked_sub(start)?) as usize;
        let mut data = vec![0; size];
        let mut file = self.file.borrow_mut();
        file.seek(SeekFrom::Start(start)).ok()?;
        file.read_exact(&mut data).ok()?;
        let mut pages = self.pages.borrow_mut();
        if pages.len() >= 1024 {
            pages.clear();
        }
        pages.insert(index, data.clone());
        Some(data)
    }
}

impl Source for FileSource {
    fn len(&self) -> u64 {
        self.len
    }
    fn read_at(&self, at: u64, buf: &mut [u8]) -> bool {
        if at
            .checked_add(buf.len() as u64)
            .is_none_or(|end| end > self.len)
        {
            return false;
        }
        let mut done = 0;
        while done < buf.len() {
            let pos = at + done as u64;
            let Some(page) = self.page(pos / Self::PAGE) else {
                return false;
            };
            let from = (pos % Self::PAGE) as usize;
            let n = (page.len() - from).min(buf.len() - done);
            buf[done..done + n].copy_from_slice(&page[from..from + n]);
            done += n;
        }
        true
    }
}

struct Reader<'a> {
    src: &'a dyn Source,
    little: bool,
}

impl Reader<'_> {
    fn array<const N: usize>(&self, at: u64) -> Option<[u8; N]> {
        let mut buf = [0; N];
        self.src.read_at(at, &mut buf).then_some(buf)
    }
    fn u8(&self, at: u64) -> Option<u8> {
        self.array::<1>(at).map(|b| b[0])
    }
    fn u16(&self, at: u64) -> Option<u16> {
        let b = self.array::<2>(at)?;
        Some(if self.little {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        })
    }
    fn u32(&self, at: u64) -> Option<u32> {
        let b = self.array::<4>(at)?;
        Some(if self.little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    }
    fn u64(&self, at: u64) -> Option<u64> {
        let b = self.array::<8>(at)?;
        Some(if self.little {
            u64::from_le_bytes(b)
        } else {
            u64::from_be_bytes(b)
        })
    }
    /// A 32-bit field in ELF32, 64-bit in ELF64.
    fn word(&self, at: u64, wide: bool) -> Option<u64> {
        if wide {
            self.u64(at)
        } else {
            self.u32(at).map(u64::from)
        }
    }
    /// A NUL-terminated string (at most 4096 bytes).
    fn cstr(&self, at: u64) -> String {
        let mut out = Vec::new();
        let mut pos = at;
        while out.len() < 4096 {
            match self.u8(pos) {
                Some(0) | None => break,
                Some(b) => out.push(b),
            }
            pos += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    }
}

fn flags_rwx(r: bool, w: bool, x: bool) -> String {
    [(r, 'R'), (w, 'W'), (x, 'X')]
        .iter()
        .map(|&(on, c)| if on { c } else { '-' })
        .collect()
}

/// Read and parse an ELF file; only headers, tables and strings are read.
pub fn read(path: &Path) -> Result<Elf> {
    let file =
        File::open(path).map_err(|e| Error(format!("Cannot read {}: {e}", path.display())))?;
    let len = file
        .metadata()
        .map_err(|e| Error(format!("Cannot read {}: {e}", path.display())))?
        .len();
    parse_source(&FileSource {
        file: RefCell::new(file),
        len,
        pages: RefCell::new(HashMap::new()),
    })
}

/// Parse ELF bytes held in memory.
pub fn parse(data: &[u8]) -> Result<Elf> {
    parse_source(&Slice(data))
}

fn parse_source(data: &dyn Source) -> Result<Elf> {
    let mut ident = [0u8; 16];
    if !data.read_at(0, &mut ident) || &ident[..4] != b"\x7fELF" {
        return Err(Error("Not an ELF file (no \\x7fELF magic)".into()));
    }
    let wide = match ident[4] {
        1 => false,
        2 => true,
        c => return Err(Error(format!("Unknown ELF class {c}"))),
    };
    let little = match ident[5] {
        1 => true,
        2 => false,
        e => return Err(Error(format!("Unknown ELF data encoding {e}"))),
    };
    let r = Reader { src: data, little };
    let truncated = || Error("Truncated ELF header".into());
    let kind = match r.u16(16).ok_or_else(truncated)? {
        1 => "REL (relocatable)".to_string(),
        2 => "EXEC (executable)".to_string(),
        3 => "DYN (shared object / PIE)".to_string(),
        4 => "CORE".to_string(),
        n => format!("type {n}"),
    };
    let machine_id = r.u16(18).ok_or_else(truncated)?;
    let (entry, phoff, shoff, flags_at) = if wide {
        (r.u64(24), r.u64(32), r.u64(40), 48)
    } else {
        (
            r.u32(24).map(u64::from),
            r.u32(28).map(u64::from),
            r.u32(32).map(u64::from),
            36,
        )
    };
    let (entry, phoff, shoff) = (
        entry.ok_or_else(truncated)?,
        phoff.ok_or_else(truncated)?,
        shoff.ok_or_else(truncated)?,
    );
    let field = |n: u64| r.u16(flags_at + 4 + 2 * n).ok_or_else(truncated);
    let (phentsize, phnum, shentsize, shnum, shstrndx) =
        (field(1)?, field(2)?, field(3)?, field(4)?, field(5)?);
    let mut notes = Vec::new();
    let mut table = |what: &str, off: u64, entsize: u16, num: u16, min: u16| -> Vec<u64> {
        if num == 0 {
            return Vec::new();
        }
        if entsize < min {
            notes.push(format!("{what}: entry size {entsize} is too small"));
            return Vec::new();
        }
        let total = u64::from(entsize) * u64::from(num);
        if total > MAX_TABLE || off.checked_add(total).is_none_or(|end| end > data.len()) {
            notes.push(format!("{what} table is truncated or out of range"));
            return Vec::new();
        }
        (0..u64::from(num))
            .map(|i| off + i * u64::from(entsize))
            .collect()
    };
    let ph = table(
        "Program header",
        phoff,
        phentsize,
        phnum,
        if wide { 56 } else { 32 },
    );
    let sh = table(
        "Section header",
        shoff,
        shentsize,
        shnum,
        if wide { 64 } else { 40 },
    );
    let segments = ph
        .iter()
        .filter_map(|&at| {
            let p_type = r.u32(at)?;
            let (offset, vaddr, filesz, memsz, flags) = if wide {
                (
                    r.u64(at + 8)?,
                    r.u64(at + 16)?,
                    r.u64(at + 32)?,
                    r.u64(at + 40)?,
                    r.u32(at + 4)?,
                )
            } else {
                (
                    u64::from(r.u32(at + 4)?),
                    u64::from(r.u32(at + 8)?),
                    u64::from(r.u32(at + 16)?),
                    u64::from(r.u32(at + 20)?),
                    r.u32(at + 24)?,
                )
            };
            let kind = match p_type {
                0 => "NULL".into(),
                1 => "LOAD".into(),
                2 => "DYNAMIC".into(),
                3 => "INTERP".into(),
                4 => "NOTE".into(),
                6 => "PHDR".into(),
                7 => "TLS".into(),
                0x6474_e550 => "GNU_EH_FRAME".into(),
                0x6474_e551 => "GNU_STACK".into(),
                0x6474_e552 => "GNU_RELRO".into(),
                0x6474_e553 => "GNU_PROPERTY".into(),
                n => format!("0x{n:x}"),
            };
            Some(Segment {
                kind,
                offset,
                vaddr,
                filesz,
                memsz,
                flags: flags_rwx(flags & 4 != 0, flags & 2 != 0, flags & 1 != 0),
            })
        })
        .collect();
    struct Raw {
        name: u32,
        kind: u32,
        flags: u64,
        addr: u64,
        offset: u64,
        size: u64,
        link: u32,
        entsize: u64,
    }
    let raws: Vec<Raw> = sh
        .iter()
        .filter_map(|&at| {
            Some(Raw {
                name: r.u32(at)?,
                kind: r.u32(at + 4)?,
                flags: r.word(at + 8, wide)?,
                addr: r.word(at + if wide { 16 } else { 12 }, wide)?,
                offset: r.word(at + if wide { 24 } else { 16 }, wide)?,
                size: r.word(at + if wide { 32 } else { 20 }, wide)?,
                link: r.u32(at + if wide { 40 } else { 24 })?,
                entsize: r.word(at + if wide { 56 } else { 36 }, wide)?,
            })
        })
        .collect();
    let names = raws.get(usize::from(shstrndx)).map(|s| s.offset);
    if names.is_none() && !raws.is_empty() {
        notes.push("No section-name string table".into());
    }
    let sections: Vec<Section> = raws
        .iter()
        .map(|s| Section {
            name: names
                .map(|base| r.cstr(base + u64::from(s.name)))
                .unwrap_or_default(),
            kind: match s.kind {
                0 => "NULL".into(),
                1 => "PROGBITS".into(),
                2 => "SYMTAB".into(),
                3 => "STRTAB".into(),
                4 => "RELA".into(),
                5 => "HASH".into(),
                6 => "DYNAMIC".into(),
                7 => "NOTE".into(),
                8 => "NOBITS".into(),
                9 => "REL".into(),
                11 => "DYNSYM".into(),
                14 => "INIT_ARRAY".into(),
                15 => "FINI_ARRAY".into(),
                0x6fff_fff6 => "GNU_HASH".into(),
                0x6fff_fffe => "VERNEED".into(),
                0x6fff_ffff => "VERSYM".into(),
                n => format!("0x{n:x}"),
            },
            flags: [
                (s.flags & 2 != 0, 'A'),
                (s.flags & 1 != 0, 'W'),
                (s.flags & 4 != 0, 'X'),
            ]
            .iter()
            .filter(|(on, _)| *on)
            .map(|&(_, c)| c)
            .collect(),
            addr: s.addr,
            offset: s.offset,
            size: s.size,
        })
        .collect();
    let mut symbols = Vec::new();
    for s in raws.iter().filter(|s| s.kind == 2 || s.kind == 11) {
        let entsize = if wide { 24 } else { 16 };
        if s.entsize != 0 && s.entsize < entsize {
            notes.push("Symbol table with a bad entry size skipped".into());
            continue;
        }
        let Some(strtab) = raws.get(s.link as usize).map(|t| t.offset) else {
            notes.push("Symbol table without a string table skipped".into());
            continue;
        };
        if s.size > MAX_TABLE
            || s.offset
                .checked_add(s.size)
                .is_none_or(|end| end > data.len())
        {
            notes.push("Symbol table is truncated".into());
            continue;
        }
        let count = s.size / entsize;
        for i in 1..count {
            if symbols.len() >= MAX_SYMBOLS {
                notes.push(format!("Only the first {MAX_SYMBOLS} symbols are listed"));
                break;
            }
            let at = s.offset + i * entsize;
            let parsed = if wide {
                (|| {
                    Some((
                        r.u32(at)?,
                        r.u8(at + 4)?,
                        r.u16(at + 6)?,
                        r.u64(at + 8)?,
                        r.u64(at + 16)?,
                    ))
                })()
            } else {
                (|| {
                    Some((
                        r.u32(at)?,
                        r.u8(at + 12)?,
                        r.u16(at + 14)?,
                        u64::from(r.u32(at + 4)?),
                        u64::from(r.u32(at + 8)?),
                    ))
                })()
            };
            let Some((name, info, shndx, value, size)) = parsed else {
                break;
            };
            symbols.push(Symbol {
                name: r.cstr(strtab + u64::from(name)),
                value,
                size,
                kind: match info & 0xf {
                    0 => "NOTYPE",
                    1 => "OBJECT",
                    2 => "FUNC",
                    3 => "SECTION",
                    4 => "FILE",
                    6 => "TLS",
                    10 => "IFUNC",
                    _ => "OTHER",
                }
                .into(),
                bind: match info >> 4 {
                    0 => "LOCAL",
                    1 => "GLOBAL",
                    2 => "WEAK",
                    _ => "OTHER",
                }
                .into(),
                section: shndx,
                dynamic: s.kind == 11,
            });
        }
    }
    symbols.sort_by(|a, b| a.value.cmp(&b.value).then_with(|| a.name.cmp(&b.name)));
    Ok(Elf {
        class: if wide { 64 } else { 32 },
        little,
        kind,
        machine: machine_name(machine_id),
        machine_id,
        osabi: ident[7],
        entry,
        segments,
        sections,
        symbols,
        notes,
    })
}

impl Elf {
    /// The section containing virtual address `addr`.
    pub fn section_at(&self, addr: u64) -> Option<&Section> {
        self.sections
            .iter()
            .find(|s| s.addr != 0 && addr >= s.addr && addr < s.addr + s.size.max(1))
    }
    /// File offset of a virtual address, through the program headers.
    pub fn file_offset(&self, addr: u64) -> Option<u64> {
        self.segments
            .iter()
            .find(|s| s.kind == "LOAD" && addr >= s.vaddr && addr < s.vaddr + s.filesz)
            .map(|s| s.offset + (addr - s.vaddr))
    }
    /// A function or object symbol by name.
    pub fn symbol(&self, name: &str) -> Option<&Symbol> {
        self.symbols
            .iter()
            .find(|s| s.name == name && s.section != 0)
    }
}

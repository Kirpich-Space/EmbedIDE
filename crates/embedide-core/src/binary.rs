//! Binary documents: arbitrary bytes, never decoded as text.
//!
//! The file is read on demand in bounded pages (one page cached), so a
//! multi-gigabyte image costs a few kilobytes of memory. Edits are an
//! overlay of replaced bytes (`offset → value`); the file on disk is not
//! touched until [`BinaryFile::save`], which refuses to run when the file
//! changed since it was opened, writes a complete copy with the edits next
//! to the original, flushes it, and renames it over the original. Bytes
//! are only replaced, never inserted or removed, so the length cannot
//! change and untouched bytes are copied unchanged.
//!
//! Atomicity: on POSIX filesystems the rename is atomic (readers see the
//! old or the new file, never a mix). Permissions are copied to the new
//! file; ownership, extended attributes, ACLs and hard links are not
//! preserved (the path then names a new inode).
use crate::{Error, Result};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::SystemTime,
};

/// Bytes read per page.
const PAGE: usize = 64 * 1024;
/// Largest single read requested by a view.
const MAX_READ: usize = 1024 * 1024;
/// Undo steps kept.
const HISTORY: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Change {
    offset: u64,
    /// Overlay value before the change (None: the byte on disk).
    before: Option<u8>,
    after: u8,
}

/// An open binary file.
#[derive(Debug)]
pub struct BinaryFile {
    path: PathBuf,
    file: File,
    len: u64,
    modified_at: Option<SystemTime>,
    page: Option<(u64, Vec<u8>)>,
    edits: BTreeMap<u64, u8>,
    undo: Vec<Change>,
    redo: Vec<Change>,
}

fn io_error(path: &Path, what: &str, e: io::Error) -> Error {
    let hint = match e.kind() {
        io::ErrorKind::PermissionDenied => " (permission denied)",
        io::ErrorKind::NotFound => " (no such file)",
        _ => "",
    };
    Error(format!("Cannot {what} {}: {e}{hint}", path.display()))
}

impl BinaryFile {
    /// Open read-only; nothing is read until bytes are requested.
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path).map_err(|e| io_error(path, "open", e))?;
        let meta = file.metadata().map_err(|e| io_error(path, "inspect", e))?;
        if !meta.is_file() {
            return Err(Error(format!("{} is not a regular file", path.display())));
        }
        Ok(Self {
            path: path.to_path_buf(),
            file,
            len: meta.len(),
            modified_at: meta.modified().ok(),
            page: None,
            edits: BTreeMap::new(),
            undo: Vec::new(),
            redo: Vec::new(),
        })
    }

    /// Path of the file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Length in bytes (fixed: edits replace bytes only).
    pub fn len(&self) -> u64 {
        self.len
    }

    /// Whether the file is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Changed bytes not saved yet.
    pub fn changed(&self) -> usize {
        self.edits.len()
    }

    /// Unsaved changes exist.
    pub fn is_modified(&self) -> bool {
        !self.edits.is_empty()
    }

    /// Whether the byte at `offset` differs from the file on disk.
    pub fn is_edited(&self, offset: u64) -> bool {
        self.edits.contains_key(&offset)
    }

    fn disk(&mut self, offset: u64, len: usize) -> Result<Vec<u8>> {
        let len = len.min(self.len.saturating_sub(offset) as usize);
        let cached = self.page.as_ref().filter(|(start, data)| {
            offset >= *start && offset + len as u64 <= start + data.len() as u64
        });
        if let Some((start, data)) = cached {
            let from = (offset - start) as usize;
            return Ok(data[from..from + len].to_vec());
        }
        let start = offset - offset % PAGE as u64;
        let want = ((offset + len as u64 - start) as usize)
            .max(PAGE)
            .min((self.len - start) as usize);
        let mut data = vec![0; want];
        self.file
            .seek(SeekFrom::Start(start))
            .and_then(|_| self.file.read_exact(&mut data))
            .map_err(|e| io_error(&self.path, "read", e))?;
        let from = (offset - start) as usize;
        let out = data[from..from + len].to_vec();
        if data.len() <= 4 * PAGE {
            self.page = Some((start, data));
        }
        Ok(out)
    }

    /// Up to `len` bytes (≤ 1 MiB) at `offset`, with unsaved edits applied.
    pub fn read(&mut self, offset: u64, len: usize) -> Result<Vec<u8>> {
        if offset >= self.len {
            return Ok(Vec::new());
        }
        let mut bytes = self.disk(offset, len.min(MAX_READ))?;
        let end = offset + bytes.len() as u64;
        for (&at, &value) in self.edits.range(offset..end) {
            bytes[(at - offset) as usize] = value;
        }
        Ok(bytes)
    }

    /// The byte at `offset`, with edits applied.
    pub fn byte(&mut self, offset: u64) -> Result<Option<u8>> {
        Ok(self.read(offset, 1)?.first().copied())
    }

    /// Replace one byte (an undo step). Offsets past the end are refused:
    /// the file never grows or shrinks.
    pub fn set(&mut self, offset: u64, value: u8) -> Result<()> {
        if offset >= self.len {
            return Err(Error(format!(
                "Offset 0x{offset:x} is past the end (0x{:x} bytes); binary editing never changes the length",
                self.len
            )));
        }
        let before = self.edits.get(&offset).copied();
        let current = match before {
            Some(v) => v,
            None => self.disk(offset, 1)?[0],
        };
        if current == value {
            return Ok(());
        }
        self.apply(offset, Some(value))?;
        self.undo.push(Change {
            offset,
            before,
            after: value,
        });
        if self.undo.len() > HISTORY {
            self.undo.remove(0);
        }
        self.redo.clear();
        Ok(())
    }

    fn apply(&mut self, offset: u64, value: Option<u8>) -> Result<()> {
        let original = self.disk(offset, 1)?[0];
        match value {
            Some(v) if v != original => {
                self.edits.insert(offset, v);
            }
            _ => {
                self.edits.remove(&offset);
            }
        }
        Ok(())
    }

    /// Undo the last change; returns its offset.
    pub fn undo(&mut self) -> Result<Option<u64>> {
        let Some(change) = self.undo.pop() else {
            return Ok(None);
        };
        self.apply(change.offset, change.before)?;
        self.redo.push(change);
        Ok(Some(change.offset))
    }

    /// Redo the last undone change; returns its offset.
    pub fn redo(&mut self) -> Result<Option<u64>> {
        let Some(change) = self.redo.pop() else {
            return Ok(None);
        };
        self.apply(change.offset, Some(change.after))?;
        self.undo.push(change);
        Ok(Some(change.offset))
    }

    /// First occurrence of `pattern` at or after `from` (wrapping to the
    /// start), with edits applied. Streams the file in pages.
    pub fn find(&mut self, pattern: &[u8], from: u64) -> Result<Option<u64>> {
        if pattern.is_empty() || pattern.len() > PAGE || self.len < pattern.len() as u64 {
            return Ok(None);
        }
        let last = self.len - pattern.len() as u64;
        for (start, end) in [(from.min(last + 1), last + 1), (0, from.min(last + 1))] {
            let mut at = start;
            while at < end {
                let chunk = self.read(at, PAGE + pattern.len() - 1)?;
                let limit =
                    ((end - at) as usize).min(chunk.len().saturating_sub(pattern.len() - 1));
                if let Some(i) = chunk
                    .windows(pattern.len())
                    .take(limit)
                    .position(|w| w == pattern)
                {
                    return Ok(Some(at + i as u64));
                }
                at += PAGE as u64;
            }
        }
        Ok(None)
    }

    /// Write the edits to the file. Refuses when the file changed on disk
    /// since it was opened (size or modification time).
    pub fn save(&mut self) -> Result<()> {
        if self.edits.is_empty() {
            return Ok(());
        }
        let meta = fs::metadata(&self.path).map_err(|e| io_error(&self.path, "inspect", e))?;
        if meta.len() != self.len || meta.modified().ok() != self.modified_at {
            return Err(Error(format!(
                "{} changed on disk since it was opened; nothing was written (reopen it)",
                self.path.display()
            )));
        }
        if meta.permissions().readonly() {
            return Err(Error(format!(
                "{} is read-only; nothing was written",
                self.path.display()
            )));
        }
        let dir = self
            .path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .to_path_buf();
        let name = self
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let temp = dir.join(format!(".{name}.embedide-{}.tmp", std::process::id()));
        let result = self.write_copy(&temp, &meta);
        if let Err(e) = result {
            let _ = fs::remove_file(&temp);
            return Err(e);
        }
        fs::rename(&temp, &self.path).map_err(|e| {
            let _ = fs::remove_file(&temp);
            io_error(&self.path, "replace", e)
        })?;
        if let Ok(d) = File::open(&dir) {
            let _ = d.sync_all();
        }
        let reopened = Self::open(&self.path)?;
        self.file = reopened.file;
        self.modified_at = reopened.modified_at;
        self.page = None;
        self.edits.clear();
        self.undo.clear();
        self.redo.clear();
        Ok(())
    }

    fn write_copy(&mut self, temp: &Path, meta: &fs::Metadata) -> Result<()> {
        let mut out = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(temp)
            .map_err(|e| io_error(temp, "create", e))?;
        let mut offset = 0;
        while offset < self.len {
            let chunk = self.read(offset, PAGE)?;
            out.write_all(&chunk)
                .map_err(|e| io_error(temp, "write", e))?;
            offset += chunk.len() as u64;
        }
        out.sync_all().map_err(|e| io_error(temp, "flush", e))?;
        let written = out
            .metadata()
            .map_err(|e| io_error(temp, "inspect", e))?
            .len();
        if written != self.len {
            return Err(Error(format!(
                "Wrote {written} of {} bytes to {}; the original is unchanged",
                self.len,
                temp.display()
            )));
        }
        fs::set_permissions(temp, meta.permissions())
            .map_err(|e| io_error(temp, "set permissions on", e))?;
        Ok(())
    }
}

/// Parse a byte pattern: hexadecimal (`7f 45 4c`, `7f454c`, `0x7f`) or, in
/// double quotes, text (`"ELF"`, with `\n`, `\t`, `\0`, `\xNN` escapes).
pub fn parse_pattern(text: &str) -> Result<Vec<u8>> {
    let t = text.trim();
    if let Some(inner) = t.strip_prefix('"') {
        let inner = inner.strip_suffix('"').unwrap_or(inner);
        let mut out = Vec::new();
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c != '\\' {
                let mut buf = [0; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                continue;
            }
            match chars.next() {
                Some('n') => out.push(b'\n'),
                Some('t') => out.push(b'\t'),
                Some('r') => out.push(b'\r'),
                Some('0') => out.push(0),
                Some('x') => {
                    let hex: String = chars.by_ref().take(2).collect();
                    out.push(
                        u8::from_str_radix(&hex, 16)
                            .map_err(|_| Error(format!("Bad escape \\x{hex}")))?,
                    );
                }
                Some(c) => out.push(c as u8),
                None => out.push(b'\\'),
            }
        }
        return Ok(out);
    }
    let digits: String = t
        .split_whitespace()
        .map(|w| w.trim_start_matches("0x").trim_start_matches("0X"))
        .collect();
    if digits.is_empty() || digits.len() % 2 != 0 || !digits.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(Error(
            "Search for hex bytes (7f 45 4c) or text in quotes (\"ELF\")".into(),
        ));
    }
    Ok((0..digits.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&digits[i..i + 2], 16).expect("hex checked"))
        .collect())
}

/// Parse an offset: `0x1f00`, `1f00h` (hex) or decimal.
pub fn parse_offset(text: &str) -> Result<u64> {
    let t = text.trim().replace('_', "");
    let parsed = if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16)
    } else if let Some(hex) = t.strip_suffix('h') {
        u64::from_str_radix(hex, 16)
    } else {
        t.parse()
    };
    parsed.map_err(|_| Error(format!("Not an offset: {text} (use 0x1f00 or 7936)")))
}

/// Whether a file looks binary: a NUL byte or invalid UTF-8 in its first
/// 8 KiB (a cut multi-byte character at the end does not count).
pub fn looks_binary(path: &Path) -> bool {
    let mut head = vec![0; 8192];
    let Ok(n) = File::open(path).and_then(|mut f| f.read(&mut head)) else {
        return false;
    };
    let head = &head[..n];
    if head.contains(&0) {
        return true;
    }
    match std::str::from_utf8(head) {
        Ok(_) => false,
        Err(e) => e.error_len().is_some(),
    }
}

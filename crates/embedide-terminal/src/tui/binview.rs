//! Binary view: HEX viewer/editor, ELF inspector and disassembly, in the
//! editor area. Opening never modifies the file; editing is an explicit
//! mode (`i`) and only `w` (confirmed by the workspace) writes.
use super::layout::{self, Row};
use super::theme::Role;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use embedide_core::{binary, disasm, elf};
use embedide_editor::syntax::Highlight;
use std::{path::Path, sync::mpsc, thread};

/// Which view of the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Offsets, bytes and ASCII.
    Hex,
    /// ELF headers, segments, sections, symbols.
    Elf,
    /// Disassembly listing.
    Disasm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ElfTab {
    Header,
    Segments,
    Sections,
    Symbols,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Prompt {
    Goto,
    Search,
}

enum Disasm {
    None,
    Running(mpsc::Receiver<Result<disasm::Listing, String>>),
    Ready(disasm::Listing),
    Failed(String),
}

/// What the workspace should do after a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Handled here.
    Done,
    /// Not a binary-view key (global shortcuts go on).
    Ignored,
    /// Close the view (the workspace confirms when there are changes).
    Close,
    /// Save the edits (the workspace confirms first).
    Save,
}

/// Settings from `native.binary`.
#[derive(Debug, Clone)]
pub struct Settings {
    /// Bytes per hex row (8, 16 or 32).
    pub bytes_per_row: usize,
    /// Editing may be switched on with `i`.
    pub allow_edit: bool,
    /// Intel syntax for x86 disassembly.
    pub intel: bool,
    /// Program overrides.
    pub objdump: Option<String>,
    /// Program overrides.
    pub llvm_objdump: Option<String>,
}

/// An open binary.
pub struct BinaryView {
    /// The document.
    pub file: binary::BinaryFile,
    /// Current view.
    pub mode: Mode,
    /// Byte under the cursor.
    pub cursor: u64,
    top: u64,
    per_row: usize,
    /// Edit mode (hex digits replace bytes).
    pub editing: bool,
    nibble: Option<u8>,
    settings: Settings,
    elf: Option<Result<elf::Elf, String>>,
    tab: ElfTab,
    elf_sel: usize,
    elf_top: usize,
    disasm: Disasm,
    dis_sel: usize,
    dis_top: usize,
    prompt: Option<(Prompt, String)>,
    last_search: Option<(Mode, String)>,
    /// Last message for the status bar.
    pub message: String,
    height: usize,
    /// Address to show once the disassembly is ready.
    pending: Option<u64>,
}

const HELP: &str = "HEX: arrows/hjkl move · g offset · / search (7f 45 or \"text\") · n next · i edit · u undo · Ctrl+R redo · w save · +/- width · Tab/e/d ELF, disassembly · q close";

impl BinaryView {
    /// Open read-only in HEX mode.
    pub fn open(path: &Path, settings: Settings) -> Result<Self, String> {
        let file = binary::BinaryFile::open(path).map_err(|e| e.0)?;
        let per_row = match settings.bytes_per_row {
            8 | 16 | 32 => settings.bytes_per_row,
            _ => 16,
        };
        Ok(Self {
            file,
            mode: Mode::Hex,
            cursor: 0,
            top: 0,
            per_row,
            editing: false,
            nibble: None,
            settings,
            elf: None,
            tab: ElfTab::Header,
            elf_sel: 0,
            elf_top: 0,
            disasm: Disasm::None,
            dis_sel: 0,
            dis_top: 0,
            prompt: None,
            last_search: None,
            message: HELP.into(),
            height: 20,
            pending: None,
        })
    }

    /// Path of the file.
    pub fn path(&self) -> &Path {
        self.file.path()
    }

    /// Drop cached ELF/disassembly state after saving patched bytes to disk.
    pub fn invalidate_analysis(&mut self) {
        self.elf = None;
        self.disasm = Disasm::None;
    }

    fn elf(&mut self) -> Result<&elf::Elf, String> {
        if self.elf.is_none() {
            self.elf = Some(elf::read(self.file.path()).map_err(|e| e.0));
        }
        self.elf
            .as_ref()
            .expect("set")
            .as_ref()
            .map_err(Clone::clone)
    }

    /// Switch view; disassembly starts in the background on first use.
    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
        self.prompt = None;
        if mode == Mode::Disasm && matches!(self.disasm, Disasm::None | Disasm::Failed(_)) {
            self.start_disasm();
        }
        if mode == Mode::Elf {
            if let Err(e) = self.elf() {
                self.message = format!("Not an ELF file: {e}");
            }
        }
    }

    fn start_disasm(&mut self) {
        let path = self.file.path().to_path_buf();
        let intel = self.settings.intel;
        let has_override = self.settings.objdump.is_some() || self.settings.llvm_objdump.is_some();
        match self.elf().cloned() {
            Ok(elf) if disasm::supports_builtin(&elf) && !has_override => {
                let (tx, rx) = mpsc::channel();
                thread::spawn(move || {
                    let _ =
                        tx.send(disasm::disassemble_builtin(&elf, &path, intel).map_err(|e| e.0));
                });
                self.disasm = Disasm::Running(rx);
                self.message = "Disassembling…".into();
            }
            Ok(elf) => match disasm::command(
                &elf,
                &path,
                intel,
                self.settings.objdump.as_deref(),
                self.settings.llvm_objdump.as_deref(),
            ) {
                Ok((program, args)) => {
                    let (tx, rx) = mpsc::channel();
                    thread::spawn(move || {
                        let _ = tx.send(disasm::disassemble(&program, &args).map_err(|e| e.0));
                    });
                    self.disasm = Disasm::Running(rx);
                    self.message = "Disassembling…".into();
                }
                Err(e) => self.disasm = Disasm::Failed(e.0),
            },
            Err(e) => self.disasm = Disasm::Failed(e),
        }
    }

    /// Poll the background disassembler. Returns whether anything changed.
    pub fn tick(&mut self) -> bool {
        let Disasm::Running(rx) = &self.disasm else {
            return false;
        };
        match rx.try_recv() {
            Ok(Ok(listing)) => {
                self.message = format!(
                    "{} lines from {}{}",
                    listing.lines.len(),
                    listing.tool,
                    if listing.truncated {
                        " (cut at the size limit)"
                    } else {
                        ""
                    }
                );
                self.disasm = Disasm::Ready(listing);
                if let Some(addr) = self.pending.take() {
                    self.goto_address(addr);
                }
                true
            }
            Ok(Err(e)) => {
                self.disasm = Disasm::Failed(e);
                true
            }
            Err(mpsc::TryRecvError::Empty) => false,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.disasm = Disasm::Failed("disassembler stopped".into());
                true
            }
        }
    }

    fn move_to(&mut self, offset: u64) {
        self.cursor = offset.min(self.file.len().saturating_sub(1));
        self.nibble = None;
    }

    /// Jump the hex cursor to a file offset (and show the hex view).
    pub fn goto_offset(&mut self, offset: u64) {
        self.mode = Mode::Hex;
        self.move_to(offset);
        let row = self.cursor / self.per_row as u64;
        self.top = row.saturating_sub(2);
    }

    /// Jump the disassembly to the first line at or after `addr`.
    pub fn goto_address(&mut self, addr: u64) -> bool {
        if matches!(self.disasm, Disasm::None) {
            self.start_disasm();
        }
        self.mode = Mode::Disasm;
        if let Disasm::Ready(listing) = &self.disasm {
            if let Some(i) = listing
                .lines
                .iter()
                .position(|l| l.addr.is_some_and(|a| a >= addr))
            {
                self.dis_sel = i;
                self.dis_top = i.saturating_sub(3);
                return true;
            }
        }
        false
    }

    fn search(&mut self, text: &str, from_next: bool) {
        match self.mode {
            Mode::Hex => match binary::parse_pattern(text) {
                Ok(pattern) => {
                    let from = self.cursor + u64::from(from_next);
                    match self.file.find(&pattern, from) {
                        Ok(Some(at)) => {
                            self.goto_offset(at);
                            self.message = format!("Found at 0x{at:x}");
                        }
                        Ok(None) => self.message = format!("Not found: {text}"),
                        Err(e) => self.message = e.0,
                    }
                }
                Err(e) => self.message = e.0,
            },
            Mode::Disasm => {
                let Disasm::Ready(listing) = &self.disasm else {
                    return;
                };
                let needle = text.to_lowercase();
                let start = self.dis_sel + usize::from(from_next);
                let n = listing.lines.len();
                let hit = (0..n).map(|i| (start + i) % n.max(1)).find(|&i| {
                    let l = &listing.lines[i];
                    l.text.to_lowercase().contains(&needle)
                        || l.addr
                            .is_some_and(|a| format!("{a:x}") == needle.trim_start_matches("0x"))
                });
                match hit {
                    Some(i) => {
                        self.dis_sel = i;
                        self.dis_top = i.saturating_sub(3);
                        self.message = format!("{text}: line {}", i + 1);
                    }
                    None => self.message = format!("Not found: {text}"),
                }
            }
            Mode::Elf => {
                let tab = self.tab;
                let Ok(e) = self.elf() else { return };
                let needle = text.to_lowercase();
                let names: Vec<String> = match tab {
                    ElfTab::Sections => e.sections.iter().map(|s| s.name.clone()).collect(),
                    _ => e.symbols.iter().map(|s| s.name.clone()).collect(),
                };
                if self.tab != ElfTab::Sections {
                    self.tab = ElfTab::Symbols;
                }
                let start = self.elf_sel + usize::from(from_next);
                let n = names.len();
                let order = || (0..n).map(|i| (start + i) % n.max(1));
                // An exact name first (`main`, not `__libc_start_main`), then a substring.
                let exact = order().find(|&i| names[i].to_lowercase() == needle);
                match exact.or_else(|| order().find(|&i| names[i].to_lowercase().contains(&needle)))
                {
                    Some(i) => {
                        self.elf_sel = i;
                        self.elf_top = i.saturating_sub(3);
                    }
                    None => self.message = format!("Not found: {text}"),
                }
            }
        }
    }

    fn submit_prompt(&mut self, kind: Prompt, text: String) {
        match (kind, self.mode) {
            (Prompt::Search, mode) => {
                self.last_search = Some((mode, text.clone()));
                self.search(&text, false);
            }
            (Prompt::Goto, Mode::Disasm) => {
                match binary::parse_offset(&format!("0x{}", text.trim().trim_start_matches("0x"))) {
                    Ok(addr) => {
                        if !self.goto_address(addr) {
                            self.message = format!("No instruction at or after 0x{addr:x}");
                        }
                    }
                    Err(e) => self.message = e.0,
                }
            }
            (Prompt::Goto, _) => match binary::parse_offset(&text) {
                Ok(offset) if offset < self.file.len() => self.goto_offset(offset),
                Ok(offset) => {
                    self.message = format!(
                        "0x{offset:x} is past the end (0x{:x} bytes)",
                        self.file.len()
                    )
                }
                Err(e) => self.message = e.0,
            },
        }
    }

    /// Handle a key.
    pub fn key(&mut self, key: KeyEvent) -> Outcome {
        if let Some((kind, mut text)) = self.prompt.take() {
            match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.submit_prompt(kind, text),
                KeyCode::Backspace => {
                    text.pop();
                    self.prompt = Some((kind, text));
                }
                KeyCode::Char(c)
                    if !key.modifiers.contains(KeyModifiers::CONTROL) && text.len() < 256 =>
                {
                    text.push(c);
                    self.prompt = Some((kind, text));
                }
                _ => self.prompt = Some((kind, text)),
            }
            return Outcome::Done;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return match key.code {
                KeyCode::Char('r') => {
                    self.redo();
                    Outcome::Done
                }
                KeyCode::Char('s') => Outcome::Save,
                _ => Outcome::Ignored,
            };
        }
        if self.editing && self.mode == Mode::Hex {
            if let KeyCode::Char(c) = key.code {
                if let Some(digit) = c.to_digit(16) {
                    self.type_nibble(digit as u8);
                    return Outcome::Done;
                }
            }
            if key.code == KeyCode::Esc {
                self.editing = false;
                self.nibble = None;
                self.message = "Navigation mode (w saves, u undoes)".into();
                return Outcome::Done;
            }
        }
        let page = self.height.saturating_sub(3).max(1);
        match (self.mode, key.code) {
            (_, KeyCode::Tab) => self.set_mode(match self.mode {
                Mode::Hex => Mode::Elf,
                Mode::Elf => Mode::Disasm,
                Mode::Disasm => Mode::Hex,
            }),
            (_, KeyCode::Char('x')) if !self.editing => self.set_mode(Mode::Hex),
            (_, KeyCode::Char('e')) if !self.editing => self.set_mode(Mode::Elf),
            (_, KeyCode::Char('d')) if !self.editing => self.set_mode(Mode::Disasm),
            (_, KeyCode::Char('q')) if !self.editing => return Outcome::Close,
            (_, KeyCode::Char('w')) if !self.editing => return Outcome::Save,
            (_, KeyCode::Char('/')) => self.prompt = Some((Prompt::Search, String::new())),
            (_, KeyCode::Char('g')) if !self.editing => {
                self.prompt = Some((Prompt::Goto, String::new()))
            }
            (_, KeyCode::Char('n')) if !self.editing => {
                if let Some((_, text)) = self.last_search.clone() {
                    self.search(&text, true);
                }
            }
            (_, KeyCode::Char('u')) if !self.editing => self.undo(),
            (Mode::Hex, code) => self.hex_key(code, page),
            (Mode::Elf, code) => self.elf_key(code, page),
            (Mode::Disasm, code) => self.disasm_key(code, page),
        }
        Outcome::Done
    }

    /// Two hex digits make one byte (one undo step); the first digit is
    /// only shown (`A_`) until the second arrives.
    fn type_nibble(&mut self, digit: u8) {
        let Some(high) = self.nibble.take() else {
            self.nibble = Some(digit);
            return;
        };
        if let Err(e) = self.file.set(self.cursor, (high << 4) | digit) {
            self.message = e.0;
            return;
        }
        self.cursor = (self.cursor + 1).min(self.file.len().saturating_sub(1));
    }

    fn undo(&mut self) {
        match self.file.undo() {
            Ok(Some(at)) => {
                self.goto_offset(at);
                self.message = format!("Undone at 0x{at:x}; {} bytes changed", self.file.changed());
            }
            Ok(None) => self.message = "Nothing to undo".into(),
            Err(e) => self.message = e.0,
        }
    }

    fn redo(&mut self) {
        match self.file.redo() {
            Ok(Some(at)) => {
                self.goto_offset(at);
                self.message = format!("Redone at 0x{at:x}");
            }
            Ok(None) => self.message = "Nothing to redo".into(),
            Err(e) => self.message = e.0,
        }
    }

    fn hex_key(&mut self, code: KeyCode, page: usize) {
        let row = self.per_row as u64;
        let len = self.file.len();
        match code {
            KeyCode::Left | KeyCode::Char('h') => self.move_to(self.cursor.saturating_sub(1)),
            KeyCode::Right | KeyCode::Char('l') => self.move_to(self.cursor + 1),
            KeyCode::Up | KeyCode::Char('k') => self.move_to(self.cursor.saturating_sub(row)),
            KeyCode::Down | KeyCode::Char('j') => {
                if self.cursor + row < len {
                    self.move_to(self.cursor + row)
                }
            }
            KeyCode::PageUp => self.move_to(self.cursor.saturating_sub(row * page as u64)),
            KeyCode::PageDown => {
                self.move_to((self.cursor + row * page as u64).min(len.saturating_sub(1)))
            }
            KeyCode::Home | KeyCode::Char('0') => self.move_to(self.cursor - self.cursor % row),
            KeyCode::End | KeyCode::Char('$') => {
                self.move_to(self.cursor - self.cursor % row + row - 1)
            }
            KeyCode::Char('G') => self.move_to(len.saturating_sub(1)),
            KeyCode::Char('+') => self.per_row = (self.per_row * 2).min(32),
            KeyCode::Char('-') => self.per_row = (self.per_row / 2).max(8),
            KeyCode::Char('i') if self.settings.allow_edit => {
                self.editing = true;
                self.message = "EDIT: type hex digits to replace bytes · Esc navigation · nothing is written until w".into();
            }
            KeyCode::Char('i') => {
                self.message = "Editing is disabled (native.binary.allow_edit = false)".into()
            }
            KeyCode::Esc => {}
            _ => {}
        }
    }

    fn elf_rows_len(&mut self) -> usize {
        let tab = self.tab;
        match self.elf() {
            Ok(e) => match tab {
                ElfTab::Header => 0,
                ElfTab::Segments => e.segments.len(),
                ElfTab::Sections => e.sections.len(),
                ElfTab::Symbols => e.symbols.len(),
            },
            Err(_) => 0,
        }
    }

    fn elf_key(&mut self, code: KeyCode, page: usize) {
        let len = self.elf_rows_len();
        match code {
            KeyCode::Char('1') => self.tab = ElfTab::Header,
            KeyCode::Char('2') => self.tab = ElfTab::Segments,
            KeyCode::Char('3') => self.tab = ElfTab::Sections,
            KeyCode::Char('4') => self.tab = ElfTab::Symbols,
            KeyCode::Up | KeyCode::Char('k') => self.elf_sel = self.elf_sel.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => {
                self.elf_sel = (self.elf_sel + 1).min(len.saturating_sub(1))
            }
            KeyCode::PageUp => self.elf_sel = self.elf_sel.saturating_sub(page),
            KeyCode::PageDown => self.elf_sel = (self.elf_sel + page).min(len.saturating_sub(1)),
            KeyCode::Enter => self.elf_enter(),
            _ => {}
        }
        if matches!(code, KeyCode::Char('1'..='4')) {
            self.elf_sel = 0;
            self.elf_top = 0;
        }
    }

    fn elf_enter(&mut self) {
        let (tab, sel) = (self.tab, self.elf_sel);
        let Ok(e) = self.elf().cloned() else { return };
        match tab {
            ElfTab::Segments => {
                if let Some(s) = e.segments.get(sel) {
                    self.goto_offset(s.offset);
                }
            }
            ElfTab::Sections => {
                if let Some(s) = e.sections.get(sel) {
                    if s.executable() && s.addr != 0 {
                        self.set_mode(Mode::Disasm);
                        self.pending_address(s.addr);
                    } else if s.kind != "NOBITS" {
                        self.goto_offset(s.offset);
                    } else {
                        self.message = format!("{} has no bytes in the file (NOBITS)", s.name);
                    }
                }
            }
            ElfTab::Symbols => {
                if let Some(sym) = e.symbols.get(sel) {
                    let code = e
                        .section_at(sym.value)
                        .is_some_and(elf::Section::executable);
                    if code {
                        self.set_mode(Mode::Disasm);
                        self.pending_address(sym.value);
                    } else if let Some(offset) = e.file_offset(sym.value) {
                        self.goto_offset(offset);
                    } else {
                        self.message = format!("{} has no file bytes", sym.name);
                    }
                }
            }
            ElfTab::Header => self.goto_offset(0),
        }
    }

    /// Jump once the disassembly is ready.
    fn pending_address(&mut self, addr: u64) {
        if !self.goto_address(addr) {
            if matches!(self.disasm, Disasm::Running(_)) {
                self.pending = Some(addr);
                self.message = format!("Disassembling… then 0x{addr:x}");
            } else {
                self.message = format!("No instruction at 0x{addr:x}");
            }
        }
    }

    fn disasm_key(&mut self, code: KeyCode, page: usize) {
        let len = match &self.disasm {
            Disasm::Ready(l) => l.lines.len(),
            _ => 0,
        };
        match code {
            KeyCode::Up | KeyCode::Char('k') => self.dis_sel = self.dis_sel.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => {
                self.dis_sel = (self.dis_sel + 1).min(len.saturating_sub(1))
            }
            KeyCode::PageUp => self.dis_sel = self.dis_sel.saturating_sub(page),
            KeyCode::PageDown => self.dis_sel = (self.dis_sel + page).min(len.saturating_sub(1)),
            KeyCode::Char('G') => self.dis_sel = len.saturating_sub(1),
            KeyCode::Char(c @ ('[' | ']')) => {
                if let Disasm::Ready(l) = &self.disasm {
                    let is_section = |i: &usize| l.lines[*i].kind == disasm::Kind::Section;
                    let next = if c == ']' {
                        (self.dis_sel + 1..len).find(is_section)
                    } else {
                        (0..self.dis_sel).rev().find(is_section)
                    };
                    if let Some(i) = next {
                        self.dis_sel = i;
                        self.dis_top = i;
                    }
                }
            }
            KeyCode::Char('m') => {
                self.last_search = Some((Mode::Disasm, "<main>".into()));
                if let Disasm::Ready(l) = &self.disasm {
                    match l
                        .lines
                        .iter()
                        .position(|x| x.kind == disasm::Kind::Symbol && x.text == "main")
                    {
                        Some(i) => {
                            self.dis_sel = i;
                            self.dis_top = i.saturating_sub(1);
                            self.message = "main".into();
                        }
                        None => self.message = "No symbol main (stripped binary?)".into(),
                    }
                }
            }
            KeyCode::Char('s') => {
                self.settings.intel = !self.settings.intel;
                self.message = format!(
                    "{} syntax",
                    if self.settings.intel { "Intel" } else { "AT&T" }
                );
                self.start_disasm();
            }
            KeyCode::Enter => {
                let addr = match &self.disasm {
                    Disasm::Ready(l) => l.lines.get(self.dis_sel).and_then(|x| x.addr),
                    _ => None,
                };
                if let (Some(addr), Ok(e)) = (addr, self.elf().cloned()) {
                    match e.file_offset(addr) {
                        Some(offset) => self.goto_offset(offset),
                        None => self.message = format!("0x{addr:x} is not backed by file bytes"),
                    }
                }
            }
            _ => {}
        }
    }

    /// One status line (mode, size, cursor, changes).
    pub fn status(&self) -> String {
        let name = self
            .file
            .path()
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mode = match (self.mode, self.editing) {
            (Mode::Hex, true) => "HEX EDIT",
            (Mode::Hex, false) => "HEX",
            (Mode::Elf, _) => "ELF",
            (Mode::Disasm, _) => "DISASM",
        };
        let changed = if self.file.is_modified() {
            format!(" [+{} bytes changed]", self.file.changed())
        } else {
            String::new()
        };
        format!(
            "{mode} | {name}{changed} | {} bytes | offset 0x{:x} | {}",
            self.file.len(),
            self.cursor,
            self.message
        )
    }

    /// Render `width` × `height` rows.
    pub fn rows(&mut self, width: usize, height: usize) -> Vec<Row> {
        self.height = height;
        let mut rows = match self.mode {
            Mode::Hex => self.hex_rows(width, height.saturating_sub(1)),
            Mode::Elf => self.elf_rows(width, height.saturating_sub(1)),
            Mode::Disasm => self.disasm_rows(width, height.saturating_sub(1)),
        };
        rows.resize_with(height.saturating_sub(1), Row::default);
        let footer = match &self.prompt {
            Some((Prompt::Goto, t)) if self.mode == Mode::Disasm => format!("Go to address (hex): {t}_"),
            Some((Prompt::Goto, t)) => format!("Go to offset (0x1f00 or decimal): {t}_"),
            Some((Prompt::Search, t)) if self.mode == Mode::Hex => format!("Search bytes (7f 45 4c or \"text\"): {t}_"),
            Some((Prompt::Search, t)) => format!("Search: {t}_"),
            None => match (self.mode, self.editing) {
                (Mode::Hex, true) => "EDIT: hex digits replace bytes · arrows move · Esc navigation · w save (asks) · u undo".into(),
                (Mode::Hex, false) => HELP.into(),
                (Mode::Elf, _) => "ELF: 1 header · 2 segments · 3 sections · 4 symbols · j/k select · Enter open · / find · Tab next view · q close".into(),
                (Mode::Disasm, _) => "DISASM: j/k · g address · / search · n next · m main · [ ] sections · s Intel/AT&T · Enter → hex · q close".into(),
            },
        };
        rows.push(layout::label(&footer, width, true));
        rows
    }

    fn hex_rows(&mut self, width: usize, height: usize) -> Vec<Row> {
        let per = self.per_row as u64;
        let total_rows = self.file.len().div_ceil(per).max(1);
        let body = height.saturating_sub(1).max(1) as u64;
        let row = self.cursor / per;
        if row < self.top {
            self.top = row;
        }
        if row >= self.top + body {
            self.top = row + 1 - body;
        }
        self.top = self.top.min(total_rows.saturating_sub(1));
        let wide = self.file.len() > u64::from(u32::MAX);
        let off_w = if wide { 16 } else { 8 };
        let hex_w = self.per_row * 3 + self.per_row / 8;
        let mut out = vec![layout::label(
            &format!("{:<off_w$}  {:<hex_w$} ASCII", "OFFSET", "HEX BYTES"),
            width,
            false,
        )];
        let bytes = self
            .file
            .read(self.top * per, (body * per) as usize)
            .unwrap_or_default();
        for (i, chunk) in bytes.chunks(self.per_row).enumerate() {
            let base = (self.top + i as u64) * per;
            let mut r = Row::default();
            r.push(&format!("{base:0off_w$x}  "), false);
            for (j, b) in chunk.iter().enumerate() {
                let at = base + j as u64;
                let edited = self.file.is_edited(at);
                let style = edited.then_some(Role::Accent.into());
                let text = match self.nibble {
                    Some(high) if at == self.cursor => format!("{high:X}_"),
                    _ => format!("{b:02X}"),
                };
                r.push_styled(&text, at == self.cursor, style);
                r.push(if (j + 1) % 8 == 0 { "  " } else { " " }, false);
            }
            let used: usize = chunk.len() * 3 + chunk.len() / 8;
            if used < hex_w + 1 {
                r.push(&" ".repeat(hex_w + 1 - used), false);
            }
            for (j, b) in chunk.iter().enumerate() {
                let at = base + j as u64;
                let c = if (0x20..0x7f).contains(b) {
                    *b as char
                } else {
                    '.'
                };
                r.push_styled(
                    &c.to_string(),
                    at == self.cursor,
                    self.file.is_edited(at).then_some(Role::Accent.into()),
                );
            }
            out.push(clip(r, width));
        }
        if self.file.is_empty() {
            out.push(layout::label("(empty file)", width, false));
        }
        out
    }

    fn elf_rows(&mut self, width: usize, height: usize) -> Vec<Row> {
        let tab = self.tab;
        let e = match self.elf() {
            Ok(e) => e.clone(),
            Err(err) => {
                return vec![layout::label(
                    &format!("Not an ELF file: {err}"),
                    width,
                    false,
                )]
            }
        };
        let mut out = vec![layout::label(
            &format!(
                "ELF{} {} | {} | {} | entry 0x{:x}   [1] header  [2] segments ({})  [3] sections ({})  [4] symbols ({})  Enter opens · / find",
                e.class,
                if e.little { "LE" } else { "BE" },
                e.machine,
                e.kind,
                e.entry,
                e.segments.len(),
                e.sections.len(),
                e.symbols.len()
            ),
            width,
            false,
        )];
        let lines: Vec<String> = match tab {
            ElfTab::Header => {
                let mut v = vec![
                    format!("Class:        ELF{} ({}-bit)", e.class, e.class),
                    format!("Endianness:   {}", if e.little { "little" } else { "big" }),
                    format!("Machine:      {} (e_machine {})", e.machine, e.machine_id),
                    format!("Type:         {}", e.kind),
                    format!("OS/ABI:       {}", e.osabi),
                    format!("Entry point:  0x{:x}", e.entry),
                    format!("Segments:     {}", e.segments.len()),
                    format!("Sections:     {}", e.sections.len()),
                    format!("Symbols:      {}", e.symbols.len()),
                ];
                v.extend(e.notes.iter().map(|n| format!("Note: {n}")));
                v
            }
            ElfTab::Segments => std::iter::once(format!(
                "{:<14} {:>10} {:>18} {:>10} {:>10} FLAGS",
                "TYPE", "OFFSET", "VADDR", "FILESZ", "MEMSZ"
            ))
            .chain(e.segments.iter().map(|s| {
                format!(
                    "{:<14} {:>#10x} {:>#18x} {:>#10x} {:>#10x} {}",
                    s.kind, s.offset, s.vaddr, s.filesz, s.memsz, s.flags
                )
            }))
            .collect(),
            ElfTab::Sections => std::iter::once(format!(
                "{:<24} {:<12} {:<4} {:>18} {:>10} {:>10}",
                "NAME", "TYPE", "FLG", "ADDR", "OFFSET", "SIZE"
            ))
            .chain(e.sections.iter().map(|s| {
                format!(
                    "{:<24} {:<12} {:<4} {:>#18x} {:>#10x} {:>#10x}",
                    s.name, s.kind, s.flags, s.addr, s.offset, s.size
                )
            }))
            .collect(),
            ElfTab::Symbols => std::iter::once(format!(
                "{:>18} {:>8} {:<8} {:<7} {}",
                "VALUE", "SIZE", "TYPE", "BIND", "NAME"
            ))
            .chain(e.symbols.iter().map(|s| {
                format!(
                    "{:>#18x} {:>8} {:<8} {:<7} {}{}",
                    s.value,
                    s.size,
                    s.kind,
                    s.bind,
                    s.name,
                    if s.dynamic { " (dynamic)" } else { "" }
                )
            }))
            .collect(),
        };
        let header = usize::from(tab != ElfTab::Header);
        let body = height.saturating_sub(1 + header).max(1);
        if tab != ElfTab::Header {
            out.push(layout::label(&lines[0], width, false));
            let n = lines.len() - 1;
            self.elf_sel = self.elf_sel.min(n.saturating_sub(1));
            if self.elf_sel < self.elf_top {
                self.elf_top = self.elf_sel;
            }
            if self.elf_sel >= self.elf_top + body {
                self.elf_top = self.elf_sel + 1 - body;
            }
            for (i, line) in lines[1..].iter().enumerate().skip(self.elf_top).take(body) {
                out.push(layout::label(line, width, i == self.elf_sel));
            }
        } else {
            out.extend(
                lines
                    .iter()
                    .take(body)
                    .map(|l| layout::label(l, width, false)),
            );
        }
        out
    }

    fn disasm_rows(&mut self, width: usize, height: usize) -> Vec<Row> {
        let listing = match &self.disasm {
            Disasm::Ready(l) => l,
            Disasm::Running(_) => {
                return vec![layout::label(
                    "Disassembling… (the binary is read, never run)",
                    width,
                    false,
                )]
            }
            Disasm::Failed(e) => {
                return vec![layout::label(
                    &format!("Disassembly unavailable: {e}"),
                    width,
                    false,
                )]
            }
            Disasm::None => return vec![layout::label("Press d to disassemble", width, false)],
        };
        let mut out = vec![layout::label(
            "ADDRESS           BYTES                     INSTRUCTION   (g address · / search · m main · [ ] sections · s syntax · Enter hex)",
            width,
            false,
        )];
        let body = height.saturating_sub(1).max(1);
        let n = listing.lines.len();
        self.dis_sel = self.dis_sel.min(n.saturating_sub(1));
        if self.dis_sel < self.dis_top {
            self.dis_top = self.dis_sel;
        }
        if self.dis_sel >= self.dis_top + body {
            self.dis_top = self.dis_sel + 1 - body;
        }
        for (i, l) in listing
            .lines
            .iter()
            .enumerate()
            .skip(self.dis_top)
            .take(body)
        {
            let selected = i == self.dis_sel;
            let row = match l.kind {
                disasm::Kind::Section => {
                    let mut r = Row::default();
                    r.push_styled(
                        &format!("── section {} ──", l.text),
                        selected,
                        Some(Highlight::Preprocessor.into()),
                    );
                    r
                }
                disasm::Kind::Symbol => {
                    let mut r = Row::default();
                    r.push_styled(
                        &format!("{:016x} <{}>:", l.addr.unwrap_or(0), l.text),
                        selected,
                        Some(Highlight::Function.into()),
                    );
                    r
                }
                disasm::Kind::Instruction => {
                    let mut r = Row::default();
                    r.push(&format!("{:>16x}  ", l.addr.unwrap_or(0)), selected);
                    r.push_styled(
                        &format!("{:<24}  ", l.bytes),
                        selected,
                        Some(Highlight::Comment.into()),
                    );
                    let (mnemonic, operands) = l.text.split_once(' ').unwrap_or((&l.text, ""));
                    let branch = embedide_editor::asm::Dialect::GasAtt.is_branch(mnemonic)
                        || embedide_editor::asm::Dialect::Aarch64.is_branch(mnemonic)
                        || embedide_editor::asm::Dialect::RiscV.is_branch(mnemonic);
                    r.push_styled(
                        mnemonic,
                        selected,
                        Some(
                            if branch {
                                Highlight::Branch
                            } else {
                                Highlight::Keyword
                            }
                            .into(),
                        ),
                    );
                    r.push(&format!(" {operands}"), selected);
                    r
                }
            };
            out.push(clip(row, width));
        }
        out
    }
}

/// Cut a row to `width` cells (all content here is ASCII).
fn clip(row: Row, width: usize) -> Row {
    let mut out = Row::default();
    let mut used = 0;
    for run in row.runs {
        if used >= width {
            break;
        }
        let take: String = run.text.chars().take(width - used).collect();
        used += take.chars().count();
        out.push_attrs(&take, run.selected, run.style, run.cell);
    }
    out
}

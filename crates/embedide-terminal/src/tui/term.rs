//! Interactive terminal panel: the user's shell in a real pseudo-terminal,
//! rendered through a VT100/xterm screen model. Input goes to the shell
//! unchanged except for the workspace's panel keys (Alt+1/2/3).
//!
//! The screen model only draws; it never talks back. Interactive shells do:
//! fish 4 sends Primary Device Attributes (`CSI c`) and cursor position
//! (`CSI 6 n`) queries before every prompt and blocks until they are answered.
//! [`Replies`] answers those status queries the way an xterm-compatible
//! terminal does, after the preceding output reached the screen model, so the
//! reported cursor position is exact.
use super::layout::{self, CellStyle, Row};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::{Duration, Instant},
};

/// Lines of history kept for scrollback.
pub const SCROLLBACK_LINES: usize = 5000;
/// Output chunks buffered between the PTY reader and the screen model. When
/// full, the reader blocks, so a flooding program is slowed by the PTY
/// instead of growing memory without bound.
const PENDING_CHUNKS: usize = 64;
/// Longest a single `poll` spends feeding output, so the UI keeps drawing
/// and reading keys (Ctrl+C) while a program floods the terminal.
const POLL_BUDGET: Duration = Duration::from_millis(30);

/// One shell session.
pub struct TerminalPane {
    parser: vt100::Parser,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    output: Receiver<Vec<u8>>,
    replies: Replies,
    exited: Option<String>,
    size: (u16, u16),
    shell: String,
}

fn io_err(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}

/// The user's shell; never a fixed choice made by EmbedIDE.
pub fn default_shell() -> String {
    if cfg!(windows) {
        std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into())
    } else {
        std::env::var("SHELL")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "/bin/sh".into())
    }
}

impl TerminalPane {
    /// Start `shell` in `cwd` with extra environment and directories prepended to PATH.
    pub fn spawn(
        shell: &str,
        cwd: &Path,
        env: &BTreeMap<String, String>,
        path_dirs: &[PathBuf],
        rows: u16,
        cols: u16,
    ) -> io::Result<Self> {
        let (rows, cols) = (rows.max(2), cols.max(10));
        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(io_err)?;
        let mut cmd = CommandBuilder::new(shell);
        cmd.cwd(cwd);
        for (k, v) in env {
            cmd.env(k, v);
        }
        if !path_dirs.is_empty() {
            let current = std::env::var_os("PATH").unwrap_or_default();
            let joined: OsString = std::env::join_paths(
                path_dirs
                    .iter()
                    .cloned()
                    .chain(std::env::split_paths(&current)),
            )
            .map_err(io_err)?;
            cmd.env("PATH", joined);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("EMBEDIDE_PROJECT", cwd);
        let child = pair.slave.spawn_command(cmd).map_err(io_err)?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(io_err)?;
        let writer = pair.master.take_writer().map_err(io_err)?;
        let (tx, output) = mpsc::sync_channel(PENDING_CHUNKS);
        thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        Ok(Self {
            parser: vt100::Parser::new(rows, cols, SCROLLBACK_LINES),
            master: pair.master,
            writer,
            child,
            output,
            replies: Replies::default(),
            exited: None,
            size: (rows, cols),
            shell: shell.to_string(),
        })
    }

    /// Shell command line shown in the panel title.
    pub fn shell(&self) -> &str {
        &self.shell
    }

    /// Feed pending output to the screen model. Returns whether anything changed.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        let started = Instant::now();
        loop {
            if changed && started.elapsed() >= POLL_BUDGET {
                // More output is waiting; the next frame continues.
                return true;
            }
            match self.output.try_recv() {
                Ok(bytes) => {
                    self.process(&bytes);
                    changed = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if self.exited.is_none() {
                        let status = self.child.try_wait().ok().flatten();
                        self.exited = Some(
                            status
                                .map(|s| format!("exit {}", s.exit_code()))
                                .unwrap_or_else(|| "closed".into()),
                        );
                        changed = true;
                    }
                    break;
                }
            }
        }
        if self.exited.is_none() {
            if let Ok(Some(status)) = self.child.try_wait() {
                // Drain what the shell wrote before exiting
                while let Ok(bytes) = self.output.try_recv() {
                    self.process(&bytes);
                }
                self.exited = Some(format!("exit {}", status.exit_code()));
                changed = true;
            }
        }
        changed
    }

    /// Feed shell output to the screen model, answering status queries in order.
    /// A scrolled-back view stays on the same history lines (vt100 moves the
    /// offset as new lines arrive).
    fn process(&mut self, mut bytes: &[u8]) {
        while let Some((end, query)) = self.replies.scan(bytes) {
            self.parser.process(&bytes[..end]);
            let reply = query.reply(self.parser.screen());
            self.write(reply.as_bytes());
            bytes = &bytes[end..];
        }
        self.parser.process(bytes);
    }

    /// Lines the view is scrolled back from the live screen (0 = live).
    pub fn scrolled(&self) -> usize {
        self.parser.screen().scrollback()
    }

    /// Lines of history above the live screen.
    pub fn history(&mut self) -> usize {
        let current = self.scrolled();
        self.parser.screen_mut().set_scrollback(usize::MAX);
        let total = self.scrolled();
        self.parser.screen_mut().set_scrollback(current);
        total
    }

    /// Scroll the view `delta` lines back (positive) or forward (negative),
    /// clamped to the history. Full-screen programs (alternate screen) have
    /// no history: there the keys belong to the program.
    pub fn scroll(&mut self, delta: isize) {
        let target = self.scrolled().saturating_add_signed(delta);
        self.parser.screen_mut().set_scrollback(target);
    }

    /// Return to the live screen.
    pub fn scroll_to_live(&mut self) {
        self.parser.screen_mut().set_scrollback(0);
    }

    /// A full-screen program (less, htop, vim) owns the screen.
    pub fn alternate_screen(&self) -> bool {
        self.parser.screen().alternate_screen()
    }

    /// Visible rows of the panel body.
    pub fn page(&self) -> usize {
        usize::from(self.size.0)
    }

    /// Why the shell ended, if it did.
    pub fn exited(&self) -> Option<&str> {
        self.exited.as_deref()
    }

    /// Match the visible panel size.
    pub fn resize(&mut self, rows: u16, cols: u16) {
        let (rows, cols) = (rows.max(2), cols.max(10));
        if (rows, cols) == self.size {
            return;
        }
        self.size = (rows, cols);
        self.parser.screen_mut().set_size(rows, cols);
        let _ = self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
    }

    fn write(&mut self, bytes: &[u8]) {
        if self.exited.is_none() {
            let _ = self.writer.write_all(bytes);
            let _ = self.writer.flush();
        }
    }

    /// Send a key as the terminal byte sequence a shell expects.
    pub fn key(&mut self, key: KeyEvent) {
        let app_cursor = self.parser.screen().application_cursor();
        if let Some(seq) = encode_key(key, app_cursor) {
            self.write(&seq);
        }
    }

    /// Paste text (bracketed when the shell asked for it).
    pub fn paste(&mut self, text: &str) {
        if self.parser.screen().bracketed_paste() {
            let mut s = String::from("\x1b[200~");
            s.push_str(&text.replace("\x1b[201~", ""));
            s.push_str("\x1b[201~");
            self.write(s.as_bytes());
        } else {
            self.write(text.replace('\n', "\r").as_bytes());
        }
    }

    /// Visible screen rows, plus the cursor (column, row) within them.
    pub fn rows(&mut self, width: usize, height: usize) -> (Vec<Row>, Option<(u16, u16)>) {
        self.resize(height as u16, width as u16);
        let screen = self.parser.screen();
        let mut rows: Vec<Row> = (0..screen.size().0.min(height as u16))
            .map(|row| screen_row(screen, row, width as u16))
            .collect();
        rows.resize_with(height, Row::default);
        let cursor = (!screen.hide_cursor() && screen.scrollback() == 0).then(|| {
            let (r, c) = screen.cursor_position();
            (c, r)
        });
        (rows, cursor)
    }

    /// Text currently on screen (tests and accessibility).
    pub fn contents(&self) -> String {
        self.parser.screen().contents()
    }
}

/// One screen row with the colours and attributes the program set. Wide
/// characters occupy two cells; their continuation cell is skipped.
pub fn screen_row(screen: &vt100::Screen, row: u16, width: u16) -> Row {
    let mut out = Row::default();
    let mut col = 0;
    while col < width {
        let Some(cell) = screen.cell(row, col) else {
            break;
        };
        if cell.is_wide_continuation() {
            col += 1;
            continue;
        }
        let cells = if cell.is_wide() { 2 } else { 1 };
        let mut text: String = cell
            .contents()
            .chars()
            .filter(|&ch| !layout::unsafe_char(ch))
            .collect();
        if col + cells > width {
            text = " ".into();
        } else if text.is_empty() {
            text = " ".repeat(usize::from(cells));
        }
        let style = CellStyle {
            fg: color(cell.fgcolor()),
            bg: color(cell.bgcolor()),
            bold: cell.bold(),
            dim: cell.dim(),
            italic: cell.italic(),
            underline: cell.underline(),
            inverse: cell.inverse(),
        };
        out.push_attrs(
            &text,
            false,
            None,
            (style != CellStyle::default()).then_some(style),
        );
        col += cells;
    }
    out
}

fn color(color: vt100::Color) -> Option<crossterm::style::Color> {
    use crossterm::style::Color;
    match color {
        vt100::Color::Default => None,
        vt100::Color::Idx(index) => Some(Color::AnsiValue(index)),
        vt100::Color::Rgb(r, g, b) => Some(Color::Rgb { r, g, b }),
    }
}

/// xterm encoding of one key: legacy control bytes, ESC-prefixed Alt and the
/// `CSI 1;<mod>X` / `CSI <n>;<mod>~` forms for modified function keys.
pub fn encode_key(key: KeyEvent, app_cursor: bool) -> Option<Vec<u8>> {
    let mods = key.modifiers;
    let alt = mods.contains(KeyModifiers::ALT);
    let ctrl = mods.contains(KeyModifiers::CONTROL);
    // xterm modifier parameter: 1 + Shift(1) + Alt(2) + Ctrl(4).
    let param =
        1 + u8::from(mods.contains(KeyModifiers::SHIFT)) + 2 * u8::from(alt) + 4 * u8::from(ctrl);
    let prefixed = |mut bytes: Vec<u8>| {
        if alt {
            bytes.insert(0, 0x1b);
        }
        bytes
    };
    let letter = |c: char| -> Vec<u8> {
        if param > 1 {
            format!("\x1b[1;{param}{c}").into_bytes()
        } else if app_cursor {
            format!("\x1bO{c}").into_bytes()
        } else {
            format!("\x1b[{c}").into_bytes()
        }
    };
    let tilde = |n: u8| -> Vec<u8> {
        if param > 1 {
            format!("\x1b[{n};{param}~").into_bytes()
        } else {
            format!("\x1b[{n}~").into_bytes()
        }
    };
    Some(match key.code {
        KeyCode::Char(c) if ctrl => {
            let byte = match c.to_ascii_lowercase() {
                c @ 'a'..='z' => c as u8 - b'a' + 1,
                '@' | ' ' | '2' => 0,
                '[' | '3' => 0x1b,
                '\\' | '4' => 0x1c,
                ']' | '5' => 0x1d,
                '^' | '6' => 0x1e,
                '_' | '-' | '7' => 0x1f,
                '?' | '8' => 0x7f,
                _ => return None,
            };
            prefixed(vec![byte])
        }
        KeyCode::Char(c) => prefixed(c.to_string().into_bytes()),
        KeyCode::Enter => prefixed(b"\r".to_vec()),
        KeyCode::Backspace if ctrl => prefixed(vec![0x08]),
        KeyCode::Backspace => prefixed(vec![0x7f]),
        KeyCode::Tab => prefixed(b"\t".to_vec()),
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Esc => prefixed(vec![0x1b]),
        KeyCode::Up => letter('A'),
        KeyCode::Down => letter('B'),
        KeyCode::Right => letter('C'),
        KeyCode::Left => letter('D'),
        KeyCode::Home => letter('H'),
        KeyCode::End => letter('F'),
        KeyCode::Insert => tilde(2),
        KeyCode::Delete => tilde(3),
        KeyCode::PageUp => tilde(5),
        KeyCode::PageDown => tilde(6),
        KeyCode::F(n @ 1..=4) => {
            let c = b"PQRS"[usize::from(n - 1)] as char;
            if param > 1 {
                format!("\x1b[1;{param}{c}").into_bytes()
            } else {
                format!("\x1bO{c}").into_bytes()
            }
        }
        KeyCode::F(n @ 5..=12) => tilde([15, 17, 18, 19, 20, 21, 23, 24][usize::from(n - 5)]),
        _ => return None,
    })
}

/// A terminal status query that a real terminal answers on the input side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Query {
    /// `CSI c` / `CSI 0 c`: Primary Device Attributes.
    PrimaryAttributes,
    /// `CSI > c` / `CSI > 0 c`: Secondary Device Attributes.
    SecondaryAttributes,
    /// `CSI 5 n`: operating status.
    Status,
    /// `CSI 6 n` (or `CSI ? 6 n`): cursor position report.
    CursorPosition {
        /// DEC private form (`CSI ? row ; col R`).
        private: bool,
    },
}
impl Query {
    /// The answer an xterm-compatible VT220 with ANSI colour would give.
    pub fn reply(self, screen: &vt100::Screen) -> String {
        match self {
            Self::PrimaryAttributes => "\x1b[?62;22c".into(),
            Self::SecondaryAttributes => "\x1b[>1;10;0c".into(),
            Self::Status => "\x1b[0n".into(),
            Self::CursorPosition { private } => {
                let (row, col) = screen.cursor_position();
                format!(
                    "\x1b[{}{};{}R",
                    if private { "?" } else { "" },
                    row + 1,
                    col + 1
                )
            }
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum ScanState {
    #[default]
    Ground,
    Escape,
    Csi,
}

/// Incremental recogniser for status queries in shell output. Sequences may
/// be split across reads; only the bounded CSI parameter bytes are retained.
#[derive(Debug, Default)]
pub struct Replies {
    state: ScanState,
    params: Vec<u8>,
}
impl Replies {
    /// Find the next complete query: the offset just past its final byte, and its kind.
    pub fn scan(&mut self, bytes: &[u8]) -> Option<(usize, Query)> {
        for (index, &byte) in bytes.iter().enumerate() {
            match self.state {
                ScanState::Ground => {
                    if byte == 0x1b {
                        self.state = ScanState::Escape;
                    }
                }
                ScanState::Escape => {
                    self.state = match byte {
                        b'[' => {
                            self.params.clear();
                            ScanState::Csi
                        }
                        0x1b => ScanState::Escape,
                        _ => ScanState::Ground,
                    }
                }
                ScanState::Csi => match byte {
                    0x20..=0x3f => {
                        if self.params.len() < 32 {
                            self.params.push(byte);
                        } else {
                            self.state = ScanState::Ground;
                        }
                    }
                    0x40..=0x7e => {
                        self.state = ScanState::Ground;
                        let query = match (self.params.as_slice(), byte) {
                            (b"" | b"0", b'c') => Some(Query::PrimaryAttributes),
                            (b">" | b">0", b'c') => Some(Query::SecondaryAttributes),
                            (b"5", b'n') => Some(Query::Status),
                            (b"6", b'n') => Some(Query::CursorPosition { private: false }),
                            (b"?6", b'n') => Some(Query::CursorPosition { private: true }),
                            _ => None,
                        };
                        if let Some(query) = query {
                            return Some((index + 1, query));
                        }
                    }
                    0x1b => self.state = ScanState::Escape,
                    _ => self.state = ScanState::Ground,
                },
            }
        }
        None
    }
}

impl Drop for TerminalPane {
    /// Hang up the foreground job, then the shell (SIGHUP, which shells pass
    /// on to their jobs; SIGKILL after a short grace period), and reap it.
    fn drop(&mut self) {
        #[cfg(unix)]
        if self.exited.is_none() {
            use nix::{
                sys::signal::{killpg, Signal},
                unistd::Pid,
            };
            let shell = self.child.process_id().and_then(|p| i32::try_from(p).ok());
            if let Some(group) = self.master.process_group_leader() {
                if group > 0 && Some(group) != shell {
                    let _ = killpg(Pid::from_raw(group), Signal::SIGHUP);
                }
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan_all(chunks: &[&[u8]]) -> Vec<(usize, usize, Query)> {
        let mut replies = Replies::default();
        let mut found = Vec::new();
        for (index, chunk) in chunks.iter().enumerate() {
            let mut offset = 0;
            while let Some((end, query)) = replies.scan(&chunk[offset..]) {
                offset += end;
                found.push((index, offset, query));
            }
        }
        found
    }

    #[test]
    fn fish_startup_queries_are_recognised_even_when_split_across_reads() {
        // Exactly what fish 4.x writes before its first prompt.
        let fish =
            b"\x1b[?u\x1b[>0q\x1b]11;?\x1b\\\x1b[?1049h\x1bP+q696e646e\x1b\\\x1b[?1049l\x1b[0c";
        let found = scan_all(&[fish]);
        assert_eq!(found, vec![(0, fish.len(), Query::PrimaryAttributes)]);
        let (a, b) = fish.split_at(fish.len() - 2);
        assert_eq!(
            scan_all(&[a, b]),
            vec![(1, 2, Query::PrimaryAttributes)],
            "a query split between reads is still answered"
        );
        assert_eq!(
            scan_all(&[b"prompt> \x1b[6n\x1b[c\x1b[>c\x1b[5n\x1b[?6n"])
                .into_iter()
                .map(|(_, _, q)| q)
                .collect::<Vec<_>>(),
            vec![
                Query::CursorPosition { private: false },
                Query::PrimaryAttributes,
                Query::SecondaryAttributes,
                Query::Status,
                Query::CursorPosition { private: true },
            ]
        );
        // Ordinary output, colours and other CSI sequences are not queries.
        assert!(scan_all(&[b"\x1b[31mred\x1b[0m \x1b[2J \x1b[3c? text c n"]).is_empty());
    }

    #[test]
    fn replies_match_an_xterm_compatible_terminal() {
        let mut parser = vt100::Parser::new(24, 80, 0);
        parser.process(b"ab\r\ncdef");
        let screen = parser.screen();
        assert_eq!(Query::PrimaryAttributes.reply(screen), "\x1b[?62;22c");
        assert_eq!(Query::Status.reply(screen), "\x1b[0n");
        assert_eq!(
            Query::CursorPosition { private: false }.reply(screen),
            "\x1b[2;5R"
        );
        assert_eq!(
            Query::CursorPosition { private: true }.reply(screen),
            "\x1b[?2;5R"
        );
    }

    #[test]
    fn shell_colours_inverse_video_and_wide_characters_reach_the_panel() {
        // Regression: rows were built from plain text, so `ls --color`,
        // compiler diagnostics and the inverse selection bar of less/fzf/htop
        // were all drawn in the panel's default colours.
        use crossterm::style::Color;
        let mut parser = vt100::Parser::new(3, 12, 0);
        parser.process(b"\x1b[1;31mE\x1b[0m \x1b[7msel\x1b[0m \xe4\xb8\xad\x1b[38;2;1;2;3mx");
        let row = screen_row(parser.screen(), 0, 12);
        let text: String = row.runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(
            text, "E sel \u{4e2d}x   ",
            "wide char takes two cells, row is 12 cells"
        );
        let red = row.runs[0].cell.unwrap();
        assert_eq!(
            (row.runs[0].text.as_str(), red.fg, red.bold),
            ("E", Some(Color::AnsiValue(1)), true)
        );
        let sel = row.runs.iter().find(|r| r.text == "sel").unwrap();
        assert!(sel.cell.unwrap().inverse && !sel.selected);
        let rgb = row.runs.iter().find(|r| r.text == "x").unwrap();
        assert_eq!(rgb.cell.unwrap().fg, Some(Color::Rgb { r: 1, g: 2, b: 3 }));
        assert!(row
            .runs
            .iter()
            .filter(|r| r.text.trim().is_empty())
            .all(|r| r.cell.is_none()));
        // A wide character that would cross the right edge is not half-printed.
        let row = screen_row(parser.screen(), 0, 7);
        let text: String = row.runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(text, "E sel  ");
    }

    #[test]
    fn keys_use_xterm_encodings_with_modifiers() {
        let key = |code, modifiers| encode_key(KeyEvent::new(code, modifiers), false).unwrap();
        let none = KeyModifiers::NONE;
        let ctrl = KeyModifiers::CONTROL;
        let alt = KeyModifiers::ALT;
        assert_eq!(key(KeyCode::Char('c'), ctrl), b"\x03");
        assert_eq!(key(KeyCode::Char('C'), ctrl), b"\x03");
        assert_eq!(key(KeyCode::Char('d'), ctrl), b"\x04");
        assert_eq!(key(KeyCode::Char('x'), alt), b"\x1bx");
        assert_eq!(key(KeyCode::Char('c'), ctrl | alt), b"\x1b\x03");
        assert_eq!(key(KeyCode::Char('ж'), none), "ж".as_bytes());
        assert_eq!(key(KeyCode::Enter, none), b"\r");
        assert_eq!(key(KeyCode::Backspace, none), b"\x7f");
        assert_eq!(key(KeyCode::Backspace, alt), b"\x1b\x7f");
        assert_eq!(key(KeyCode::Left, none), b"\x1b[D");
        assert_eq!(key(KeyCode::Left, ctrl), b"\x1b[1;5D");
        assert_eq!(key(KeyCode::Delete, none), b"\x1b[3~");
        assert_eq!(key(KeyCode::Delete, ctrl | alt), b"\x1b[3;7~");
        assert_eq!(key(KeyCode::F(1), none), b"\x1bOP");
        assert_eq!(key(KeyCode::F(5), KeyModifiers::SHIFT), b"\x1b[15;2~");
        assert_eq!(
            encode_key(KeyEvent::new(KeyCode::Up, none), true).unwrap(),
            b"\x1bOA"
        );
    }
}

//! Interactive terminal panel: the user's shell in a real pseudo-terminal,
//! rendered through a VT100/xterm screen model. Input goes to the shell
//! unchanged except for the workspace's panel keys (Alt+1/2/3).
use super::layout::{self, Row};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
};

/// One shell session.
pub struct TerminalPane {
    parser: vt100::Parser,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    output: Receiver<Vec<u8>>,
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
        let (tx, output) = mpsc::channel();
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
            parser: vt100::Parser::new(rows, cols, 2000),
            master: pair.master,
            writer,
            child,
            output,
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
        loop {
            match self.output.try_recv() {
                Ok(bytes) => {
                    self.parser.process(&bytes);
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
                    self.parser.process(&bytes);
                }
                self.exited = Some(format!("exit {}", status.exit_code()));
                changed = true;
            }
        }
        changed
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
        self.parser.set_size(rows, cols);
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
        let arrow = |c: char| {
            if app_cursor {
                format!("\x1bO{c}")
            } else {
                format!("\x1b[{c}")
            }
        };
        let seq: Vec<u8> = match key.code {
            KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::CONTROL) => {
                let c = c.to_ascii_lowercase();
                match c {
                    'a'..='z' => vec![c as u8 - b'a' + 1],
                    '@' | ' ' => vec![0],
                    '[' => vec![0x1b],
                    '\\' => vec![0x1c],
                    ']' => vec![0x1d],
                    _ => return,
                }
            }
            KeyCode::Char(c) => {
                let mut s = String::new();
                if key.modifiers.contains(KeyModifiers::ALT) {
                    s.push('\x1b');
                }
                s.push(c);
                s.into_bytes()
            }
            KeyCode::Enter => b"\r".to_vec(),
            KeyCode::Backspace => b"\x7f".to_vec(),
            KeyCode::Tab => b"\t".to_vec(),
            KeyCode::BackTab => b"\x1b[Z".to_vec(),
            KeyCode::Esc => b"\x1b".to_vec(),
            KeyCode::Up => arrow('A').into_bytes(),
            KeyCode::Down => arrow('B').into_bytes(),
            KeyCode::Right => arrow('C').into_bytes(),
            KeyCode::Left => arrow('D').into_bytes(),
            KeyCode::Home => arrow('H').into_bytes(),
            KeyCode::End => arrow('F').into_bytes(),
            KeyCode::PageUp => b"\x1b[5~".to_vec(),
            KeyCode::PageDown => b"\x1b[6~".to_vec(),
            KeyCode::Delete => b"\x1b[3~".to_vec(),
            KeyCode::Insert => b"\x1b[2~".to_vec(),
            KeyCode::F(n) => match n {
                1 => b"\x1bOP".to_vec(),
                2 => b"\x1bOQ".to_vec(),
                3 => b"\x1bOR".to_vec(),
                4 => b"\x1bOS".to_vec(),
                5 => b"\x1b[15~".to_vec(),
                6 => b"\x1b[17~".to_vec(),
                7 => b"\x1b[18~".to_vec(),
                8 => b"\x1b[19~".to_vec(),
                9 => b"\x1b[20~".to_vec(),
                10 => b"\x1b[21~".to_vec(),
                11 => b"\x1b[23~".to_vec(),
                12 => b"\x1b[24~".to_vec(),
                _ => return,
            },
            _ => return,
        };
        self.write(&seq);
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
        let mut rows: Vec<Row> = screen
            .rows(0, width as u16)
            .take(height)
            .map(|line| layout::label(&line, width, false))
            .collect();
        rows.resize_with(height, Row::default);
        let cursor = (!screen.hide_cursor()).then(|| {
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

impl Drop for TerminalPane {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

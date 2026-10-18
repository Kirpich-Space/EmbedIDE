//! Terminal frontends for the shared editor engine.
//!
//! The default `tui` feature enables full-screen immediate input and a guarded
//! terminal lifecycle. Explicit line and batch modes retain the cooked input
//! protocol: `<Esc>`, `<C-r>`, `<Enter>` and other tokens represent keys.
//! All frontends delegate editing and file operations to the existing engine.
//! EOF with unsaved changes returns an error rather than success.
mod keys;
mod render;
#[cfg(feature = "tui")]
pub mod tui;

use embedide_editor::{vim::Mode, Editor, EditorError, Effect, Position};
use std::{
    fmt,
    io::{self, BufRead, IsTerminal, Read, Write},
    path::Path,
};

/// Failure in the input/output adapter or the shared editor.
#[derive(Debug)]
pub enum FrontendError {
    /// Input or output failed, including invalid UTF-8 input.
    Io(io::Error),
    /// A shared editor operation failed.
    Editor(EditorError),
    /// A submitted key line exceeds the bounded input budget.
    InputTooLong,
    /// Interactive mode needs terminal stdin and stdout; use batch explicitly.
    NeedsTerminal,
    /// Input ended while changes remained unsaved.
    UnsavedInput,
    /// A handled external signal requested an orderly terminal shutdown.
    Interrupted,
}
impl fmt::Display for FrontendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => fmt::Display::fmt(e, f),
            Self::Editor(e) => fmt::Display::fmt(e, f),
            Self::InputTooLong => {
                f.write_str("key line exceeds 64 KiB; no keys from this line were applied")
            }
            Self::NeedsTerminal => f.write_str(
                "interactive editing needs terminal stdin/stdout; use --batch for pipes",
            ),
            Self::UnsavedInput => {
                f.write_str("input ended with unsaved changes; file was not saved automatically")
            }
            Self::Interrupted => {
                f.write_str("terminal session interrupted; changes were not saved automatically")
            }
        }
    }
}
impl std::error::Error for FrontendError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Editor(e) => Some(e),
            _ => None,
        }
    }
}
impl From<io::Error> for FrontendError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<EditorError> for FrontendError {
    fn from(e: EditorError) -> Self {
        Self::Editor(e)
    }
}
impl From<embedide_editor::EditError> for FrontendError {
    fn from(e: embedide_editor::EditError) -> Self {
        Self::Editor(e.into())
    }
}

/// Final state; neither a quit nor EOF implicitly saves changes.
#[derive(Debug)]
pub struct SessionReport {
    /// Whether the shared engine accepted a quit command.
    pub closed: bool,
    /// Unsaved text may remain after an explicit `:q!`.
    pub modified: bool,
    /// Length of the final in-memory document in bytes.
    pub bytes: usize,
    /// Zero-based logical cursor position.
    pub position: Position,
}

/// The sole document owner; no Vim or text mutation logic lives in the frontend.
pub struct LineEditor {
    editor: Editor,
    message: String,
}
impl LineEditor {
    /// Wrap an editor supplied by the application core.
    pub fn new(editor: Editor) -> Self {
        Self {
            editor,
            message: String::new(),
        }
    }
    /// Read the engine state, including mode, selection and cursor.
    pub fn editor(&self) -> &Editor {
        &self.editor
    }
    /// Most recent status/error text, without terminal escapes when rendered.
    pub fn message(&self) -> &str {
        &self.message
    }
    /// Apply a decoded key immediately through the existing Vim engine.
    /// Unlike `submit`, this never synthesizes Enter for a command prompt.
    pub fn feed_key(&mut self, key: embedide_editor::vim::Key) -> Result<(), FrontendError> {
        self.message.clear();
        let result = self.key(key);
        if let Err(error) = &result {
            self.message = format!("Error: {error}");
        }
        result
    }
    /// Summarize the current document without saving or closing it.
    pub fn report(&self) -> Result<SessionReport, FrontendError> {
        Ok(SessionReport {
            closed: self.editor.state().is_closed(),
            modified: self.editor.state().is_modified(),
            bytes: self.editor.state().buffer().byte_len(),
            position: self.editor.state().position()?,
        })
    }
    fn key(&mut self, key: embedide_editor::vim::Key) -> Result<(), FrontendError> {
        match self.editor.feed(key)? {
            Effect::Saved(report) => {
                self.message = format!("Saved; directory_synced={}", report.directory_synced);
            }
            Effect::Ide(_) => {
                return Err(EditorError::Unsupported(
                    "IDE panel actions are unavailable in the line frontend",
                )
                .into())
            }
            Effect::Unhandled => self.message = "Key not handled by the editor".into(),
            _ => {}
        }
        Ok(())
    }
    /// Submit one key line. A line ending is transport framing, not inserted text.
    /// A pending command/search prompt is submitted at the end of the line.
    /// Explicit save/quit effects are immediate; a line is not an atomic script.
    pub fn submit(&mut self, line: &str) -> Result<(), FrontendError> {
        if line.len() > keys::MAX_INPUT_BYTES {
            return Err(FrontendError::InputTooLong);
        }
        self.message.clear();
        let result = self.apply(line);
        if let Err(error) = &result {
            self.message = format!("Error: {error}");
            if self.editor.input().mode() == Mode::Command {
                self.message.push_str("; use <Esc> to leave the prompt");
            }
        }
        result
    }
    fn apply(&mut self, line: &str) -> Result<(), FrontendError> {
        for key in keys::decode(line)? {
            self.key(key)?;
            if self.editor.state().is_closed() {
                return Ok(());
            }
        }
        if self.editor.input().mode() == Mode::Command {
            self.key(embedide_editor::vim::Key::Enter)?;
        }
        Ok(())
    }
    /// Render a bounded, escape-sanitized snapshot and key prompt.
    pub fn draw(&self, output: &mut impl Write) -> Result<(), FrontendError> {
        render::draw(self, output)
    }
    /// Read key lines until quit or EOF. Interactive errors are shown and remain
    /// recoverable; batch errors stop immediately with a failure result.
    pub fn run(
        &mut self,
        input: &mut impl BufRead,
        output: &mut impl Write,
        batch: bool,
    ) -> Result<SessionReport, FrontendError> {
        if !batch {
            writeln!(output, "EmbedIDE line editor — submit keys with Enter. <Esc> Normal; <C-r> redo; :w save; :q quit; :q! discard. Use <lt> for a literal < in key tokens.")?;
            self.draw(output)?;
        }
        while !self.editor.state().is_closed() {
            let mut line = String::new();
            let count = (&mut *input)
                .take((keys::MAX_INPUT_BYTES + 1) as u64)
                .read_line(&mut line)?;
            if count == 0 {
                if self.editor.state().is_modified() {
                    return Err(FrontendError::UnsavedInput);
                }
                break;
            }
            if line.len() > keys::MAX_INPUT_BYTES {
                return Err(FrontendError::InputTooLong);
            }
            if line.ends_with('\n') {
                line.pop();
                if line.ends_with('\r') {
                    line.pop();
                }
            }
            if let Err(error) = self.submit(&line) {
                if batch {
                    return Err(error);
                }
            }
            if !batch && !self.editor.state().is_closed() {
                self.draw(output)?;
            }
        }
        self.report()
    }
}

/// Open through the existing file layer, or bind a missing file without writing.
/// Other failures (including symlinks and invalid encoding) are never retried as creation.
pub fn open(path: &Path) -> Result<Editor, EditorError> {
    // Decide once. A failure while reading an observed existing file must not
    // turn into an empty new buffer, even if the file disappeared during open.
    match std::fs::symlink_metadata(path) {
        Ok(_) => Editor::open(path),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Editor::create(path),
        Err(e) => Err(e.into()),
    }
}

/// Connect a core-owned editor to the default frontend (TUI when enabled).
/// Batch input always retains the existing line protocol and structured output.
pub fn run_stdio(editor: Editor, batch: bool) -> Result<SessionReport, FrontendError> {
    #[cfg(feature = "tui")]
    if !batch {
        return tui::run(editor);
    }
    run_line_stdio(editor, batch)
}

/// Explicit legacy cooked-mode frontend, also used by batch automation.
pub fn run_line_stdio(editor: Editor, batch: bool) -> Result<SessionReport, FrontendError> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    if !batch && !(stdin.is_terminal() && stdout.is_terminal()) {
        return Err(FrontendError::NeedsTerminal);
    }
    LineEditor::new(editor).run(&mut stdin.lock(), &mut stdout.lock(), batch)
}

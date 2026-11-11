//! Full-screen terminal adapter. All modal interpretation and file I/O stay in Editor.
pub mod assistant;
pub mod binview;
mod guard;
pub mod layout;
pub mod popup;
pub mod screen;
pub mod term;
pub mod workspace;

use crate::{FrontendError, LineEditor, SessionReport};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    terminal,
};
use embedide_editor::{
    vim::{IdeAction, Key},
    Editor, Effect,
};
use layout::{Frame, Viewport};
use std::{
    io::{self, IsTerminal},
    time::Duration,
};

/// Presentation state with an engine-backed session and terminal-only viewport.
pub struct TuiEditor {
    session: LineEditor,
    viewport: Viewport,
    size: (u16, u16),
    ide_action: Option<IdeAction>,
    host_command: Option<String>,
}
impl TuiEditor {
    /// Start a presentation session without entering raw mode (also testable headlessly).
    pub fn new(editor: Editor, width: u16, height: u16) -> Self {
        Self {
            session: LineEditor::new(editor),
            viewport: Viewport::default(),
            size: (width, height),
            ide_action: None,
            host_command: None,
        }
    }
    /// Shared document and parser state; no terminal coordinates enter this API.
    pub fn editor(&self) -> &Editor {
        self.session.editor()
    }
    /// Current terminal viewport offsets.
    pub fn viewport(&self) -> &Viewport {
        &self.viewport
    }
    /// Compose an immutable screen frame for the diff writer.
    pub fn frame(&mut self) -> Result<Frame, FrontendError> {
        layout::compose(
            self.session.editor(),
            self.session.message(),
            &mut self.viewport,
            self.size,
        )
    }
    /// Consume a frontend-neutral IDE request emitted by a Normal-mode leader chord.
    pub fn take_ide_action(&mut self) -> Option<IdeAction> {
        self.ide_action.take()
    }
    /// Consume a `:` command addressed to the host IDE (with host commands enabled).
    pub fn take_host_command(&mut self) -> Option<String> {
        self.host_command.take()
    }
    fn feed_key(&mut self, key: Key) {
        self.session.message.clear();
        match self.session.editor.feed(key) {
            Ok(Effect::Ide(action)) => self.ide_action = Some(action),
            Ok(Effect::Command(command)) => self.host_command = Some(command),
            Ok(Effect::Saved(report)) => {
                self.session.message =
                    format!("Saved; directory_synced={}", report.directory_synced)
            }
            Ok(Effect::Unhandled) => self.session.message = "Key not handled by the editor".into(),
            Ok(_) => {}
            Err(error) => self.session.message = format!("Error: {error}"),
        }
    }
    /// Handle one decoded event. Recoverable editor failures stay visible in status.
    /// Returns false when no repaint is needed (release/unsupported event kinds).
    pub fn handle(&mut self, event: Event) -> bool {
        match event {
            Event::Resize(width, height) => {
                self.size = (width, height);
                true
            }
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                if key.modifiers == KeyModifiers::CONTROL
                    && matches!(key.code, KeyCode::Char('c' | 'C'))
                {
                    self.feed_key(Key::Escape);
                    self.session.message = "Interrupted; use :q or :q! to exit".into();
                } else if let Some(key) = translate(key) {
                    self.feed_key(key);
                } else {
                    // Let the existing parser cancel pending counts/operators;
                    // unsupported terminal chords must not leak a prefix.
                    self.feed_key(Key::Ctrl('?'));
                    self.session.message = "Unsupported key".into();
                }
                true
            }
            Event::Paste(text) => {
                self.feed_key(Key::Text(text));
                true
            }
            _ => false,
        }
    }
}

fn translate(event: KeyEvent) -> Option<Key> {
    if event.modifiers.intersects(
        KeyModifiers::ALT | KeyModifiers::SUPER | KeyModifiers::HYPER | KeyModifiers::META,
    ) {
        return None;
    }
    if event.modifiers.contains(KeyModifiers::CONTROL) {
        return match event.code {
            KeyCode::Char(ch) => Some(Key::Ctrl(ch.to_ascii_lowercase())),
            _ => None,
        };
    }
    Some(match event.code {
        KeyCode::Char(ch) => Key::Char(ch),
        KeyCode::Esc => Key::Escape,
        KeyCode::Enter => Key::Enter,
        KeyCode::Tab => Key::Tab,
        KeyCode::BackTab => Key::BackTab,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Delete => Key::Delete,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        _ => return None,
    })
}

/// Run the real full-screen adapter, restoring terminal state on return/unwind.
/// Unix SIGINT/SIGTERM/SIGHUP request cleanup within the 250 ms signal-check interval.
pub fn run(editor: Editor) -> Result<SessionReport, FrontendError> {
    if !(io::stdin().is_terminal() && io::stdout().is_terminal()) {
        return Err(FrontendError::NeedsTerminal);
    }
    if std::env::var_os("TERM").is_some_and(|term| term == "dumb") {
        return Err(embedide_editor::EditorError::Unsupported(
            "TERM=dumb cannot run the full-screen editor; use --line or --batch",
        )
        .into());
    }
    let (width, height) = terminal::size()?;
    let mut app = TuiEditor::new(editor, width, height);
    let guard = guard::TerminalGuard::enter(false)?;
    let mut screen = screen::Screen::default();
    screen.draw(&app.frame()?, &mut io::stdout())?;
    while !app.editor().state().is_closed() {
        if guard.interrupted() {
            return Err(FrontendError::Interrupted);
        }
        // Keep a signal flag as a Unix fallback when the event backend does not
        // surface SIGWINCH. Terminal dimensions still come from crossterm.
        if guard.take_resize() {
            let (width, height) = terminal::size()?;
            app.handle(Event::Resize(width, height));
            screen.draw(&app.frame()?, &mut io::stdout())?;
        }
        // A bounded wait is needed to observe handled termination signals.
        // Timeouts never redraw; ordinary keyboard input wakes immediately.
        if event::poll(Duration::from_millis(250))?
            && app.handle(event::read()?)
            && !app.editor().state().is_closed()
        {
            screen.draw(&app.frame()?, &mut io::stdout())?;
        }
    }
    app.session.report()
}

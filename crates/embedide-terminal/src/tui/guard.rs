//! RAII terminal ownership; cleanup paths do not depend on editor success.
use crossterm::{
    cursor::{SetCursorStyle, Show},
    event::{DisableBracketedPaste, EnableBracketedPaste},
    execute,
    style::{Attribute, SetAttribute},
    terminal::{self, DisableLineWrap, EnableLineWrap, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::{
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

pub(super) struct TerminalGuard {
    raw: bool,
    screen: bool,
    interrupted: Arc<AtomicBool>,
    resized: Arc<AtomicBool>,
    #[cfg(unix)]
    signals: Vec<signal_hook::SigId>,
}
impl TerminalGuard {
    pub fn enter() -> io::Result<Self> {
        let mut guard = Self {
            raw: false,
            screen: false,
            interrupted: Arc::new(AtomicBool::new(false)),
            resized: Arc::new(AtomicBool::new(false)),
            #[cfg(unix)]
            signals: Vec::new(),
        };
        #[cfg(unix)]
        for signal in [
            signal_hook::consts::SIGTERM,
            signal_hook::consts::SIGHUP,
            signal_hook::consts::SIGINT,
        ] {
            guard.signals.push(signal_hook::flag::register(
                signal,
                guard.interrupted.clone(),
            )?);
        }
        #[cfg(unix)]
        guard.signals.push(signal_hook::flag::register(
            signal_hook::consts::SIGWINCH,
            guard.resized.clone(),
        )?);
        terminal::enable_raw_mode()?;
        guard.raw = true;
        // Set ownership before writing: partial writes must still attempt cleanup.
        guard.screen = true;
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            EnableBracketedPaste,
            DisableLineWrap
        )?;
        Ok(guard)
    }
    pub fn interrupted(&self) -> bool {
        self.interrupted.load(Ordering::Relaxed)
    }
    pub fn take_resize(&self) -> bool {
        self.resized.swap(false, Ordering::Relaxed)
    }
}
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if self.screen {
            // Output failure must never prevent attempting raw-mode restoration.
            let _ = execute!(
                io::stdout(),
                SetAttribute(Attribute::Reset),
                SetCursorStyle::DefaultUserShape,
                EnableLineWrap,
                DisableBracketedPaste,
                Show,
                LeaveAlternateScreen
            );
        }
        if self.raw {
            let _ = terminal::disable_raw_mode();
        }
        #[cfg(unix)]
        for id in self.signals.drain(..) {
            signal_hook::low_level::unregister(id);
        }
    }
}

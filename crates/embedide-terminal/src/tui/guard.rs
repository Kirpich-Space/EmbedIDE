//! RAII terminal ownership; cleanup paths do not depend on editor success.
use crossterm::{
    cursor::{SetCursorStyle, Show},
    event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
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
    keyboard: bool,
    mouse: bool,
    interrupted: Arc<AtomicBool>,
    resized: Arc<AtomicBool>,
    #[cfg(unix)]
    signals: Vec<signal_hook::SigId>,
}
impl TerminalGuard {
    /// Take over the terminal. With `enhanced_keys`, terminals that implement the
    /// kitty keyboard protocol (answering `CSI ? u`) are asked to disambiguate
    /// modified keys and report press/repeat/release, which is what makes
    /// Ctrl+Alt+digit and held-key chords distinguishable at all.
    pub fn enter(enhanced_keys: bool) -> io::Result<Self> {
        let mut guard = Self {
            raw: false,
            screen: false,
            keyboard: false,
            mouse: false,
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
        if enhanced_keys
            && std::env::var_os("EMBEDIDE_KEYBOARD").is_none_or(|v| v != "legacy")
            && terminal::supports_keyboard_enhancement().unwrap_or(false)
        {
            guard.keyboard = true;
            execute!(
                io::stdout(),
                PushKeyboardEnhancementFlags(
                    KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                        | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                )
            )?;
        }
        Ok(guard)
    }
    /// Capture (or release) mouse events; released again on drop.
    pub fn set_mouse(&mut self, on: bool) -> io::Result<()> {
        if on != self.mouse {
            if on {
                execute!(io::stdout(), EnableMouseCapture)?;
            } else {
                execute!(io::stdout(), DisableMouseCapture)?;
            }
            self.mouse = on;
        }
        Ok(())
    }
    /// Whether the kitty keyboard protocol (with event types) is active.
    pub fn keyboard_enhanced(&self) -> bool {
        self.keyboard
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
        if self.mouse {
            let _ = execute!(io::stdout(), DisableMouseCapture);
        }
        if self.keyboard {
            let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
        }
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

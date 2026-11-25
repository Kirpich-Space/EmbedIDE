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
    time::Duration,
};

/// Result of waiting for terminal input without entering `crossterm` on a closed TTY.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PollOutcome {
    /// Input bytes are available on the controlling terminal.
    Ready,
    /// Timeout elapsed cleanly with no input.
    Timeout,
    /// Terminal hung up (`POLLHUP`/`POLLERR`/`POLLNVAL`) or a termination signal arrived.
    Disconnected,
}

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
    /// Non-blocking check for whether the controlling terminal hung up or closed.
    pub fn stdin_hung_up(&self) -> bool {
        #[cfg(unix)]
        {
            use nix::poll::{poll, PollFd, PollFlags, PollTimeout};
            use std::os::fd::AsFd;
            let stdin = io::stdin();
            let mut fds = [PollFd::new(stdin.as_fd(), PollFlags::POLLIN)];
            match poll(&mut fds, PollTimeout::ZERO) {
                Ok(0) => false,
                Ok(_) => fds[0].revents().is_some_and(|r| {
                    r.intersects(PollFlags::POLLHUP | PollFlags::POLLERR | PollFlags::POLLNVAL)
                }),
                Err(nix::errno::Errno::EINTR) => self.interrupted(),
                Err(_) => true,
            }
        }
        #[cfg(not(unix))]
        {
            false
        }
    }
    /// Wait up to `timeout` for input, catching `SIGHUP`/`SIGTERM`/`SIGINT` and
    /// `POLLHUP`/`POLLERR`/`POLLNVAL` before calling into `crossterm` so closing
    /// a terminal window never leaves the process spinning at 100% CPU.
    pub fn poll_input(&self, timeout: Duration) -> io::Result<PollOutcome> {
        if self.interrupted() {
            return Ok(PollOutcome::Disconnected);
        }
        #[cfg(unix)]
        {
            use nix::poll::{poll, PollFd, PollFlags, PollTimeout};
            use std::os::fd::AsFd;
            let stdin = io::stdin();
            let mut fds = [PollFd::new(stdin.as_fd(), PollFlags::POLLIN)];
            let poll_timeout =
                PollTimeout::try_from(timeout).unwrap_or(PollTimeout::from(u16::MAX));
            match poll(&mut fds, poll_timeout) {
                Ok(0) => {
                    if self.interrupted() {
                        Ok(PollOutcome::Disconnected)
                    } else {
                        Ok(PollOutcome::Timeout)
                    }
                }
                Ok(_) => {
                    let revents = fds[0].revents().unwrap_or(PollFlags::empty());
                    if revents
                        .intersects(PollFlags::POLLHUP | PollFlags::POLLERR | PollFlags::POLLNVAL)
                        || self.interrupted()
                    {
                        Ok(PollOutcome::Disconnected)
                    } else if revents.contains(PollFlags::POLLIN) {
                        Ok(PollOutcome::Ready)
                    } else {
                        Ok(PollOutcome::Timeout)
                    }
                }
                Err(nix::errno::Errno::EINTR) => {
                    if self.interrupted() {
                        Ok(PollOutcome::Disconnected)
                    } else {
                        Ok(PollOutcome::Timeout)
                    }
                }
                Err(err) => Err(io::Error::from_raw_os_error(err as i32)),
            }
        }
        #[cfg(not(unix))]
        {
            if crossterm::event::poll(timeout)? {
                Ok(PollOutcome::Ready)
            } else {
                Ok(PollOutcome::Timeout)
            }
        }
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

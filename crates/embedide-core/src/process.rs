//! Bounded asynchronous process execution shared by every frontend.
use crate::{BuildPlan, Error, Result};
use std::{
    io::Read,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// An incremental event; arbitrary child bytes are decoded lossily for display.
#[derive(Debug)]
pub enum ProcessEvent {
    /// Reviewable command, before each step.
    Command(String),
    /// Output chunk (at most 4 KiB). The boolean distinguishes stderr.
    Output(String, bool),
    /// Final outcome with elapsed wall time. None means cancellation or spawn failure.
    Finished(Option<i32>, Duration),
}

/// Child ownership that kills the process tree and reaps the direct child.
pub(crate) struct OwnedChild(pub Child, bool);
impl OwnedChild {
    pub fn spawn(command: &mut Command) -> std::io::Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        // ETXTBSY: an executable that was just written can still be open for
        // writing in a child forked concurrently by another thread (build
        // scripts and tests generate tools and run them at once). It clears as
        // soon as that child execs, so retry briefly instead of failing.
        let mut attempt = 0;
        loop {
            match command.spawn() {
                Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy && attempt < 5 => {
                    attempt += 1;
                    thread::sleep(Duration::from_millis(10 * attempt));
                }
                other => return other.map(|child| Self(child, false)),
            }
        }
    }
    pub fn terminate(&mut self) {
        if self.1 {
            return;
        }
        self.1 = true;
        #[cfg(unix)]
        {
            use nix::{
                sys::signal::{killpg, Signal},
                unistd::Pid,
            };
            let _ = killpg(Pid::from_raw(self.0.id() as i32), Signal::SIGKILL);
        }
        #[cfg(windows)]
        {
            let _ = Command::new("taskkill")
                .args(["/PID", &self.0.id().to_string(), "/T", "/F"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        self.terminate();
    }
}

pub(crate) fn capture(
    mut input: impl Read + Send + 'static,
    tx: SyncSender<ProcessEvent>,
    stderr: bool,
    dropped: Arc<AtomicU64>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut bytes = [0; 4096];
        loop {
            match input.read(&mut bytes) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx
                        .try_send(ProcessEvent::Output(
                            String::from_utf8_lossy(&bytes[..n]).into_owned(),
                            stderr,
                        ))
                        .is_err()
                    {
                        dropped.fetch_add(n as u64, Ordering::Relaxed);
                    }
                }
            }
        }
    })
}

/// One cancellable sequential plan. Dropping it requests cancellation and joins its worker.
/// Output is bounded; readers never wait on the frontend to drain the event queue.
pub struct ProcessTask {
    events: Receiver<ProcessEvent>,
    completion: Receiver<(Option<i32>, Duration)>,
    cancel: Arc<AtomicBool>,
    dropped: Arc<AtomicU64>,
    worker: Option<JoinHandle<()>>,
}
impl ProcessTask {
    /// Start only after an explicit frontend action. Never invokes a shell.
    pub fn start(
        plan: BuildPlan,
        environment: std::collections::BTreeMap<String, String>,
    ) -> Result<Self> {
        if plan.steps.is_empty() {
            return Err(Error("Empty process plan".into()));
        }
        let (tx, events) = mpsc::sync_channel(256);
        let (finished_tx, completion) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let dropped = Arc::new(AtomicU64::new(0));
        let stop = cancel.clone();
        let lost = dropped.clone();
        let worker = thread::spawn(move || {
            let started = Instant::now();
            let mut outcome = Some(0);
            for step in plan.steps {
                if stop.load(Ordering::Acquire) {
                    outcome = None;
                    break;
                }
                let _ = tx.try_send(ProcessEvent::Command(format!(
                    "{} {:?}",
                    step.program, step.args
                )));
                let spawned = OwnedChild::spawn(
                    Command::new(&step.program)
                        .args(&step.args)
                        .current_dir(&plan.directory)
                        .envs(&environment)
                        .stdin(Stdio::null())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped()),
                );
                let mut child = match spawned {
                    Ok(child) => child,
                    Err(error) => {
                        let _ = tx.try_send(ProcessEvent::Output(
                            format!("Cannot start {}: {error}\n", step.program),
                            true,
                        ));
                        outcome = None;
                        break;
                    }
                };
                let stdout = capture(
                    child.0.stdout.take().expect("piped stdout"),
                    tx.clone(),
                    false,
                    lost.clone(),
                );
                let stderr = capture(
                    child.0.stderr.take().expect("piped stderr"),
                    tx.clone(),
                    true,
                    lost.clone(),
                );
                loop {
                    if stop.load(Ordering::Acquire) {
                        child.terminate();
                        outcome = None;
                        break;
                    }
                    match child.0.try_wait() {
                        Ok(Some(status)) => {
                            outcome = Some(status.code().unwrap_or(1));
                            break;
                        }
                        Ok(None) => thread::sleep(Duration::from_millis(20)),
                        Err(error) => {
                            let _ = tx.try_send(ProcessEvent::Output(
                                format!("Process wait failed: {error}\n"),
                                true,
                            ));
                            outcome = None;
                            break;
                        }
                    }
                }
                // Kill descendants holding inherited pipes before joining readers.
                child.terminate();
                let _ = stdout.join();
                let _ = stderr.join();
                if outcome != Some(0) {
                    break;
                }
            }
            // Completion has its own reserved slot, including during cancellation.
            let _ = finished_tx.try_send((outcome, started.elapsed()));
        });
        Ok(Self {
            events,
            completion,
            cancel,
            dropped,
            worker: Some(worker),
        })
    }
    /// Drain available events without waiting.
    pub fn drain(&self) -> Vec<ProcessEvent> {
        let mut events: Vec<_> = self.events.try_iter().collect();
        if let Ok((code, elapsed)) = self.completion.try_recv() {
            events.extend(self.events.try_iter());
            events.push(ProcessEvent::Finished(code, elapsed));
        }
        events
    }
    /// Request termination of the current process group and skip remaining steps.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }
    /// Bytes omitted because the frontend did not keep up; never silently unbounded.
    pub fn dropped_bytes(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}
impl Drop for ProcessTask {
    fn drop(&mut self) {
        self.cancel();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

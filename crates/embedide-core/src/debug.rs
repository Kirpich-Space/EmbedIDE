//! Minimal structured GDB/MI session with owned optional OpenOCD lifecycle.
use crate::{
    config::DebugConfig,
    process::{capture, OwnedChild, ProcessEvent, ProcessTask},
    BuildPlan, BuildSystem, Error, ProcessSpec, Result,
};
use serde_json::{Map, Value};
use std::{
    collections::BTreeMap,
    io::Write,
    net::{TcpStream, ToSocketAddrs},
    path::Path,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// Parsed MI result/async record. Nested tuples and lists remain structured JSON.
#[derive(Debug, Clone)]
pub struct MiRecord {
    /// Numeric request identifier, if present.
    pub token: Option<u64>,
    /// ^ result, * exec async, = notify or + status.
    pub kind: char,
    /// done, error, running, stopped, etc.
    pub class: String,
    /// Named results including frame/stack/register-values/bkpt.
    pub data: Value,
}
struct Parser<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl Parser<'_> {
    fn name(&mut self) -> Option<String> {
        let start = self.at;
        while self
            .bytes
            .get(self.at)
            .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_')
        {
            self.at += 1;
        }
        (self.at > start).then(|| String::from_utf8_lossy(&self.bytes[start..self.at]).into_owned())
    }
    fn string(&mut self) -> Option<String> {
        if self.bytes.get(self.at) != Some(&b'"') {
            return None;
        }
        self.at += 1;
        let mut out = Vec::new();
        while let Some(&b) = self.bytes.get(self.at) {
            self.at += 1;
            match b {
                b'"' => return Some(String::from_utf8_lossy(&out).into_owned()),
                b'\\' => {
                    let b = *self.bytes.get(self.at)?;
                    self.at += 1;
                    match b {
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'0'..=b'7' => {
                            let mut value = (b - b'0') as u16;
                            for _ in 0..2 {
                                if let Some(&next @ b'0'..=b'7') = self.bytes.get(self.at) {
                                    value = value * 8 + (next - b'0') as u16;
                                    self.at += 1;
                                } else {
                                    break;
                                }
                            }
                            out.push(value as u8);
                        }
                        other => out.push(other),
                    }
                }
                other => out.push(other),
            }
        }
        None
    }
    fn results(&mut self, end: Option<u8>, depth: usize) -> Option<Value> {
        let mut map = Map::new();
        while self.at < self.bytes.len() && self.bytes.get(self.at).copied() != end {
            let key = self.name()?;
            if self.bytes.get(self.at) != Some(&b'=') {
                return None;
            }
            self.at += 1;
            map.insert(key, self.value(depth + 1)?);
            if self.bytes.get(self.at) != Some(&b',') {
                break;
            }
            self.at += 1;
        }
        Some(Value::Object(map))
    }
    fn value(&mut self, depth: usize) -> Option<Value> {
        if depth > 32 {
            return None;
        }
        match *self.bytes.get(self.at)? {
            b'"' => self.string().map(Value::String),
            b'{' => {
                self.at += 1;
                let value = self.results(Some(b'}'), depth)?;
                if self.bytes.get(self.at) != Some(&b'}') {
                    return None;
                }
                self.at += 1;
                Some(value)
            }
            b'[' => {
                self.at += 1;
                let mut values = Vec::new();
                while self.bytes.get(self.at) != Some(&b']') {
                    let saved = self.at;
                    if let Some(name) = self.name() {
                        if self.bytes.get(self.at) != Some(&b'=') {
                            return None;
                        }
                        self.at += 1;
                        let mut map = Map::new();
                        map.insert(name, self.value(depth + 1)?);
                        values.push(Value::Object(map));
                    } else {
                        self.at = saved;
                        values.push(self.value(depth + 1)?);
                    }
                    if self.bytes.get(self.at) != Some(&b',') {
                        break;
                    }
                    self.at += 1;
                }
                if self.bytes.get(self.at) != Some(&b']') {
                    return None;
                }
                self.at += 1;
                Some(Value::Array(values))
            }
            _ => None,
        }
    }
}
/// Parse bounded MI result/async records. Stream/console lines stay in the debug log.
pub fn parse_mi(line: &str) -> Option<MiRecord> {
    if line.len() > 65536 {
        return None;
    }
    let mut parser = Parser {
        bytes: line.trim_end().as_bytes(),
        at: 0,
    };
    while parser.bytes.get(parser.at).is_some_and(u8::is_ascii_digit) {
        parser.at += 1;
    }
    let token = line[..parser.at].parse().ok();
    let kind = *parser.bytes.get(parser.at)? as char;
    if !['^', '*', '=', '+'].contains(&kind) {
        return None;
    }
    parser.at += 1;
    let class = parser.name()?;
    let data = if parser.bytes.get(parser.at) == Some(&b',') {
        parser.at += 1;
        parser.results(None, 0)?
    } else {
        Value::Object(Map::new())
    };
    if parser.at != parser.bytes.len() {
        return None;
    }
    Some(MiRecord {
        token,
        kind,
        class,
        data,
    })
}
/// Supported operations; no implicit flash/load/reset or arbitrary CLI evaluation.
pub enum DebugCommand {
    /// Resume execution.
    Continue,
    /// Halt execution through async MI.
    Interrupt,
    /// Source-level step.
    Step,
    /// Source-level next.
    Next,
    /// Insert file:line breakpoint.
    Break(String, usize),
    /// Remove numeric breakpoint ID.
    Delete(u64),
    /// Fetch stack frames.
    Backtrace,
    /// Fetch register values as hex.
    Registers,
}
impl DebugCommand {
    fn mi(self) -> String {
        match self {
            Self::Continue => "-exec-continue".into(),
            Self::Interrupt => "-exec-interrupt --all".into(),
            Self::Step => "-exec-step".into(),
            Self::Next => "-exec-next".into(),
            Self::Break(file, line) => {
                format!("-break-insert {}", quote(&format!("{file}:{line}")))
            }
            Self::Delete(id) => format!("-break-delete {id}"),
            Self::Backtrace => "-stack-list-frames".into(),
            Self::Registers => "-data-list-register-values x".into(),
        }
    }
}
fn quote(value: &str) -> String {
    serde_json::to_string(value).expect("string serialization")
}
/// Bounded debug events.
#[derive(Debug)]
pub enum DebugEvent {
    /// Original GDB or server output, sanitized by the frontend on rendering.
    Output(String),
    /// Structured MI state or response.
    Record(MiRecord),
    /// Symbols loaded and remote connection acknowledged by GDB.
    Connected,
    /// Fatal session failure.
    Error(String),
    /// Session ended; process owners have been dropped.
    Stopped,
}
/// Owns GDB plus optional OpenOCD. All connection waits run outside the UI thread.
pub struct DebugSession {
    commands: SyncSender<DebugCommand>,
    events: Receiver<DebugEvent>,
    completion: Receiver<std::result::Result<(), String>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl DebugSession {
    /// Start a configured debug session only on explicit request.
    pub fn start(root: &Path, config: &DebugConfig) -> Result<Self> {
        let elf = root.join(&config.elf).canonicalize()?;
        if config.elf.is_empty() || !elf.is_file() || config.remote.contains(['\n', '\r']) {
            return Err(Error(
                "Debug requires an ELF file and remote host:port".into(),
            ));
        }
        if config.openocd.is_some() && config.configs.is_empty() {
            return Err(Error(
                "OpenOCD requires explicit interface/target config files".into(),
            ));
        }
        let root = root.to_path_buf();
        let config = config.clone();
        let (tx, events) = mpsc::sync_channel(256);
        let (finished_tx, completion) = mpsc::sync_channel(1);
        let (commands, rx) = mpsc::sync_channel::<DebugCommand>(32);
        let stop = Arc::new(AtomicBool::new(false));
        let cancelled = stop.clone();
        let worker = thread::spawn(move || {
            let result = (|| -> Result<()> {
                let mut server = None;
                if let Some(program) = &config.openocd {
                    // Own only a fresh server. Refuse an already listening port.
                    let address = config
                        .remote
                        .to_socket_addrs()
                        .map_err(|e| Error(e.to_string()))?
                        .next()
                        .ok_or_else(|| Error("No debug address".into()))?;
                    if !address.ip().is_loopback() {
                        return Err(Error(
                            "Owned OpenOCD requires a loopback remote address".into(),
                        ));
                    }
                    if TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_ok() {
                        return Err(Error(
                            "GDB port already occupied; refusing to connect to an unrelated server"
                                .into(),
                        ));
                    }
                    let args = config
                        .configs
                        .iter()
                        .flat_map(|file| ["-f".into(), file.clone()])
                        .chain(["-c".into(), format!("gdb_port {}", address.port())])
                        .collect();
                    server = Some(ProcessTask::start(
                        BuildPlan {
                            directory: root.clone(),
                            system: BuildSystem::Cargo,
                            steps: vec![ProcessSpec {
                                program: program.clone(),
                                args,
                            }],
                        },
                        BTreeMap::new(),
                    )?);
                    let start = Instant::now();
                    loop {
                        if cancelled.load(Ordering::Acquire) {
                            return Ok(());
                        }
                        for event in server.as_ref().expect("server").drain() {
                            match event {
                                ProcessEvent::Output(text, _) => {
                                    let _ =
                                        tx.try_send(DebugEvent::Output(format!("OpenOCD: {text}")));
                                }
                                ProcessEvent::Finished(code, _) => {
                                    return Err(Error(format!(
                                        "OpenOCD exited during startup: {code:?}"
                                    )))
                                }
                                _ => {}
                            }
                        }
                        if TcpStream::connect_timeout(&address, Duration::from_millis(50)).is_ok() {
                            break;
                        }
                        if start.elapsed() > Duration::from_secs(8) {
                            return Err(Error("OpenOCD connection timeout".into()));
                        }
                        thread::sleep(Duration::from_millis(30));
                    }
                }
                let mut gdb = OwnedChild::spawn(
                    Command::new(&config.gdb)
                        .args(["--nx", "--nh", "--quiet", "--interpreter=mi2"])
                        .current_dir(&root)
                        .stdin(Stdio::piped())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped()),
                )?;
                let mut input = gdb.0.stdin.take().expect("piped stdin");
                let (output, reads) = mpsc::sync_channel(256);
                let dropped = Arc::new(AtomicU64::new(0));
                let stdout = capture(
                    gdb.0.stdout.take().expect("stdout"),
                    output.clone(),
                    false,
                    dropped.clone(),
                );
                let stderr = capture(
                    gdb.0.stderr.take().expect("stderr"),
                    output,
                    true,
                    dropped.clone(),
                );
                let session = (|| -> Result<()> {
                    let mut startup = std::collections::VecDeque::from([
                        "-gdb-set auto-load off".to_string(),
                        "-gdb-set mi-async on".into(),
                        format!(
                            "-file-exec-and-symbols {}",
                            quote(
                                elf.to_str()
                                    .ok_or_else(|| Error("GDB ELF path must be UTF-8".into()))?
                            )
                        ),
                        format!("-target-select remote {}", quote(&config.remote)),
                    ]);
                    let mut token = 1u64;
                    let mut pending = true;
                    let mut deadline = Instant::now();
                    let mut buffer = String::new();
                    writeln!(input, "{token}{}", startup.pop_front().expect("startup"))?;
                    input.flush()?;
                    loop {
                        if cancelled.load(Ordering::Acquire) {
                            break;
                        }
                        if let Some(status) = gdb.0.try_wait()? {
                            return Err(Error(format!("GDB exited unexpectedly: {status}")));
                        }
                        if pending && deadline.elapsed() > Duration::from_secs(8) {
                            return Err(Error("GDB startup/remote connection timeout".into()));
                        }
                        if let Some(server) = &server {
                            for event in server.drain() {
                                match event {
                                    ProcessEvent::Output(text, _) => {
                                        let _ = tx.try_send(DebugEvent::Output(format!(
                                            "OpenOCD: {text}"
                                        )));
                                    }
                                    ProcessEvent::Finished(code, _) => {
                                        return Err(Error(format!("OpenOCD exited: {code:?}")))
                                    }
                                    _ => {}
                                }
                            }
                        }
                        for event in reads.try_iter() {
                            if let ProcessEvent::Output(text, err) = event {
                                let _ = tx.try_send(DebugEvent::Output(text.clone()));
                                if err {
                                    continue;
                                }
                                buffer.push_str(&text);
                                while let Some(end) = buffer.find('\n') {
                                    let line: String = buffer.drain(..=end).collect();
                                    if let Some(record) = parse_mi(&line) {
                                        if pending
                                            && record.kind == '^'
                                            && record.token == Some(token)
                                        {
                                            if record.class == "error" {
                                                return Err(Error(format!(
                                                    "GDB startup: {}",
                                                    record.data["msg"]
                                                )));
                                            }
                                            if let Some(command) = startup.pop_front() {
                                                token += 1;
                                                writeln!(input, "{token}{command}")?;
                                                input.flush()?;
                                                deadline = Instant::now();
                                            } else {
                                                pending = false;
                                                let _ = tx.try_send(DebugEvent::Connected);
                                            }
                                        }
                                        tx.try_send(DebugEvent::Record(record)).map_err(|_| Error("Debugger frontend event overflow; stopping session".into()))?;
                                    }
                                }
                                if buffer.len() > 65536 {
                                    return Err(Error("GDB output record exceeds 64 KiB".into()));
                                }
                            }
                        }
                        if dropped.load(Ordering::Relaxed) != 0 {
                            return Err(Error(
                                "GDB output overflow; stopping to avoid inconsistent state".into(),
                            ));
                        }
                        if !pending {
                            for command in rx.try_iter() {
                                token += 1;
                                writeln!(input, "{token}{}", command.mi())?;
                                input.flush()?;
                            }
                        }
                        thread::sleep(Duration::from_millis(10));
                    }
                    Ok(())
                })();
                gdb.terminate();
                let _ = stdout.join();
                let _ = stderr.join();
                drop(server);
                session
            })();
            let _ = finished_tx.try_send(result.map_err(|error| error.to_string()));
        });
        Ok(Self {
            commands,
            events,
            completion,
            stop,
            worker: Some(worker),
        })
    }
    /// Queue one supported MI operation.
    pub fn command(&self, command: DebugCommand) -> Result<()> {
        self.commands
            .try_send(command)
            .map_err(|e| Error(format!("Debugger command unavailable: {e}")))
    }
    /// Nonblocking event drain.
    pub fn drain(&self) -> Vec<DebugEvent> {
        let mut events: Vec<_> = self.events.try_iter().collect();
        if let Ok(result) = self.completion.try_recv() {
            events.extend(self.events.try_iter());
            if let Err(error) = result {
                events.push(DebugEvent::Error(error));
            }
            events.push(DebugEvent::Stopped);
        }
        events
    }
    /// Detect worker termination even if the event queue overflowed.
    pub fn is_finished(&self) -> bool {
        self.worker.as_ref().is_some_and(|w| w.is_finished())
    }
}
impl Drop for DebugSession {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

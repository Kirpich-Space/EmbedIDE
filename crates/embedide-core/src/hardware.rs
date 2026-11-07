//! Explicit hardware operations. Planning never writes to a device.
use crate::{
    config::{SerialConfig, Target},
    BuildPlan, BuildSystem, Error, ProcessSpec, Result,
};
use serialport::{DataBits, FlowControl, Parity, StopBits};
use std::{
    io::{Read, Write},
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

/// Validate a probe-rs ELF download with an exact chip and probe selection.
/// probe-rs performs chip matching and verification; mass erase/fuses are unavailable.
pub fn flash_plan(root: &Path, target: &Target, executable: &str) -> Result<BuildPlan> {
    let required = |value: &Option<String>, name: &str| -> Result<String> {
        value
            .as_ref()
            .filter(|s| {
                !s.trim().is_empty() && !s.starts_with('-') && !s.contains(['\n', '\r', '\0'])
            })
            .cloned()
            .ok_or_else(|| Error(format!("Flash requires explicit {name}")))
    };
    let chip = required(&target.chip, "chip")?;
    let probe = required(&target.probe, "probe VID:PID:serial")?;
    let parts: Vec<_> = probe.split(':').collect();
    if parts.len() != 3
        || parts[2].is_empty()
        || u16::from_str_radix(parts[0], 16).is_err()
        || u16::from_str_radix(parts[1], 16).is_err()
    {
        return Err(Error(
            "Probe must be an unambiguous VID:PID:serial selector".into(),
        ));
    }
    let path = root
        .join(required(&target.artifact, "ELF artifact")?)
        .canonicalize()?;
    let mut file = std::fs::File::open(&path)?;
    let mut header = [0; 20];
    file.read_exact(&mut header)?;
    if &header[..4] != b"\x7fELF" {
        return Err(Error(
            "Only ELF artifacts are supported by this flash workflow".into(),
        ));
    }
    if let Some(triple) = &target.compiler_target {
        let machine = if header[5] == 2 {
            u16::from_be_bytes([header[18], header[19]])
        } else {
            u16::from_le_bytes([header[18], header[19]])
        };
        if (triple.starts_with("thumb") || triple.starts_with("arm")) && machine != 40
            || triple.starts_with("riscv") && machine != 243
        {
            return Err(Error(
                "ELF architecture does not match configured compiler target".into(),
            ));
        }
    }
    Ok(BuildPlan {
        directory: root.into(),
        system: BuildSystem::Cargo,
        steps: vec![ProcessSpec {
            program: executable.into(),
            args: vec![
                "download".into(),
                "--chip".into(),
                chip,
                "--probe".into(),
                probe,
                "--verify".into(),
                path.to_str()
                    .ok_or_else(|| Error("Flash artifact path must be UTF-8".into()))?
                    .into(),
            ],
        }],
    })
}

/// Enumerate actual serial devices on demand; errors are never replaced by demo devices.
pub fn serial_ports() -> Result<Vec<String>> {
    serialport::available_ports()
        .map(|ports| {
            ports
                .into_iter()
                .map(|port| format!("{}  {:?}", port.port_name, port.port_type))
                .collect()
        })
        .map_err(|e| Error(format!("Serial enumeration: {e}")))
}

/// One enumerated serial device with the metadata the OS reports.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SerialPortEntry {
    /// Device path or name (e.g. /dev/ttyACM0, COM3).
    pub name: String,
    /// usb, pci, bluetooth or unknown.
    pub kind: &'static str,
    /// USB vendor id, when known.
    pub vid: Option<u16>,
    /// USB product id, when known.
    pub pid: Option<u16>,
    /// USB serial number, when known.
    pub serial_number: Option<String>,
    /// USB manufacturer string, when known.
    pub manufacturer: Option<String>,
    /// USB product string, when known.
    pub product: Option<String>,
}

/// Enumerate serial devices with structured metadata. An empty list stays empty.
pub fn serial_port_entries() -> Result<Vec<SerialPortEntry>> {
    let ports =
        serialport::available_ports().map_err(|e| Error(format!("Serial enumeration: {e}")))?;
    Ok(ports
        .into_iter()
        .map(|p| {
            let mut entry = SerialPortEntry {
                name: p.port_name,
                kind: "unknown",
                vid: None,
                pid: None,
                serial_number: None,
                manufacturer: None,
                product: None,
            };
            match p.port_type {
                serialport::SerialPortType::UsbPort(info) => {
                    entry.kind = "usb";
                    entry.vid = Some(info.vid);
                    entry.pid = Some(info.pid);
                    entry.serial_number = info.serial_number;
                    entry.manufacturer = info.manufacturer;
                    entry.product = info.product;
                }
                serialport::SerialPortType::PciPort => entry.kind = "pci",
                serialport::SerialPortType::BluetoothPort => entry.kind = "bluetooth",
                serialport::SerialPortType::Unknown => {}
            }
            entry
        })
        .collect())
}

impl SerialConfig {
    /// Reject unsupported framing before attempting any device access.
    pub fn validate(&self) -> Result<()> {
        if self.port.is_empty()
            || self.baud == 0
            || !(5..=8).contains(&self.data_bits)
            || ![1, 2].contains(&self.stop_bits)
            || !["none", "odd", "even"].contains(&self.parity.as_str())
            || !["none", "software", "hardware"].contains(&self.flow_control.as_str())
            || !["none", "lf", "crlf"].contains(&self.line_ending.as_str())
        {
            return Err(Error("Serial requires port, baud>0, data_bits=5..8, parity=none|odd|even, stop_bits=1|2, flow_control=none|software|hardware, line_ending=none|lf|crlf".into()));
        }
        Ok(())
    }
}
/// Receive events contain raw bytes so invalid UTF-8 and split codepoints remain safe.
#[derive(Debug)]
pub enum SerialEvent {
    /// Received data, capped by a bounded queue.
    Data(Vec<u8>),
    /// Disconnection, read error or write error.
    Error(String),
}
/// Bounded serial worker with a read timeout and explicit outgoing text.
pub struct SerialMonitor {
    events: Receiver<SerialEvent>,
    failure: Receiver<String>,
    outgoing: SyncSender<Vec<u8>>,
    stop: Arc<AtomicBool>,
    lost: Arc<AtomicU64>,
    config: SerialConfig,
    worker: Option<JoinHandle<()>>,
}
impl SerialMonitor {
    /// Open a configured port. Does not change OS permissions or reconnect silently.
    pub fn connect(config: &SerialConfig) -> Result<Self> {
        config.validate()?;
        let port = serialport::new(&config.port, config.baud)
            .data_bits(match config.data_bits {
                5 => DataBits::Five,
                6 => DataBits::Six,
                7 => DataBits::Seven,
                _ => DataBits::Eight,
            })
            .parity(match config.parity.as_str() {
                "odd" => Parity::Odd,
                "even" => Parity::Even,
                _ => Parity::None,
            })
            .stop_bits(if config.stop_bits == 2 {
                StopBits::Two
            } else {
                StopBits::One
            })
            .flow_control(match config.flow_control.as_str() {
                "software" => FlowControl::Software,
                "hardware" => FlowControl::Hardware,
                _ => FlowControl::None,
            })
            .timeout(Duration::from_millis(30))
            .open()
            .map_err(|e| {
                Error(format!(
                    "Cannot open serial {}: {e}; check device availability and OS permissions",
                    config.port
                ))
            })?;
        let (tx, events) = mpsc::sync_channel(128);
        let (failure_tx, failure) = mpsc::sync_channel(1);
        let (outgoing, rx) = mpsc::sync_channel::<Vec<u8>>(32);
        let stop = Arc::new(AtomicBool::new(false));
        let cancelled = stop.clone();
        let lost = Arc::new(AtomicU64::new(0));
        let dropped = lost.clone();
        let worker = thread::spawn(move || {
            let mut port = port;
            let mut bytes = [0; 4096];
            while !cancelled.load(Ordering::Acquire) {
                for text in rx.try_iter() {
                    if let Err(e) = port.write_all(&text) {
                        let _ = failure_tx.try_send(format!("Serial write: {e}"));
                        return;
                    }
                }
                match port.read(&mut bytes) {
                    Ok(0) => {
                        let _ = failure_tx.try_send("Serial device returned EOF".into());
                        return;
                    }
                    Ok(n) => {
                        if tx.try_send(SerialEvent::Data(bytes[..n].into())).is_err() {
                            dropped.fetch_add(n as u64, Ordering::Relaxed);
                        }
                    }
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                        ) => {}
                    Err(e) => {
                        let _ = failure_tx.try_send(format!("Serial disconnected: {e}"));
                        return;
                    }
                }
            }
        });
        Ok(Self {
            events,
            failure,
            outgoing,
            stop,
            lost,
            config: config.clone(),
            worker: Some(worker),
        })
    }
    /// Nonblocking receive drain.
    pub fn drain(&self) -> Vec<SerialEvent> {
        let mut events: Vec<_> = self.events.try_iter().collect();
        if let Ok(error) = self.failure.try_recv() {
            events.extend(self.events.try_iter());
            events.push(SerialEvent::Error(error));
        }
        events
    }
    /// Bounded transmission; receive-only is enforced in the service.
    pub fn send(&self, text: &str) -> Result<()> {
        if text.len() > 4096 {
            return Err(Error("Serial message exceeds 4 KiB".into()));
        }
        let mut bytes = text.as_bytes().to_vec();
        match self.config.line_ending.as_str() {
            "lf" => bytes.push(b'\n'),
            "crlf" => bytes.extend(b"\r\n"),
            _ => {}
        }
        self.send_bytes(&bytes)
    }
    /// Bounded transmission of raw bytes, exactly as given (binary-safe).
    pub fn send_bytes(&self, bytes: &[u8]) -> Result<()> {
        if self.config.receive_only {
            return Err(Error(
                "Serial receive_only=true; transmission disabled".into(),
            ));
        }
        if bytes.len() > 4098 {
            return Err(Error("Serial message exceeds 4 KiB".into()));
        }
        self.outgoing
            .try_send(bytes.to_vec())
            .map_err(|e| Error(format!("Serial transmission unavailable: {e}")))
    }
    /// Queue overflow is observable; pause in the frontend never stops receiving.
    pub fn dropped_bytes(&self) -> u64 {
        self.lost.load(Ordering::Relaxed)
    }
    /// Detect unexpected termination without blocking.
    pub fn is_finished(&self) -> bool {
        self.worker.as_ref().is_some_and(|w| w.is_finished())
    }
}
impl Drop for SerialMonitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

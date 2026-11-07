//! JSON-lines serial service for the desktop application.
//!
//! One request per stdin line, one event per stdout line. Payload bytes are
//! hex-encoded so binary data and split UTF-8 sequences survive unchanged.
//!
//! Requests:  {"op":"list"} | {"op":"open","config":{…SerialConfig…}}
//!            {"op":"write","hex":"…"} | {"op":"close"}
//! Events:    {"event":"ports","ports":[…]} | {"event":"opened","port":…}
//!            {"event":"data","hex":"…"} | {"event":"dropped","bytes":n}
//!            {"event":"disconnected","message":…} | {"event":"error","op":…,"message":…}
//!            {"event":"closed"}
//!
//! End of stdin (the parent exited) closes the port and terminates.
use embedide_core::{
    config::SerialConfig,
    hardware::{serial_port_entries, SerialEvent, SerialMonitor},
};
use serde_json::{json, Value};
use std::{
    io::{self, BufRead, Write},
    sync::mpsc,
    thread,
    time::Duration,
};

/// Lowercase hex encoding.
pub fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(DIGITS[(b >> 4) as usize] as char);
        s.push(DIGITS[(b & 15) as usize] as char);
    }
    s
}

/// Strict hex decoding (even length, 0-9a-fA-F only).
pub fn from_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    let digit = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    };
    s.as_bytes()
        .chunks(2)
        .map(|p| Some(digit(p[0])? << 4 | digit(p[1])?))
        .collect()
}

fn emit(out: &mut impl Write, value: Value) -> io::Result<()> {
    serde_json::to_writer(&mut *out, &value)?;
    out.write_all(b"\n")?;
    out.flush()
}

fn parse_config(v: &Value) -> Result<SerialConfig, String> {
    let mut c = SerialConfig::default();
    let obj = v.as_object().ok_or("config must be an object")?;
    let text = |k: &str| obj.get(k).and_then(Value::as_str).map(str::to_owned);
    let num = |k: &str| obj.get(k).and_then(Value::as_u64);
    c.port = text("port").ok_or("config.port is required")?;
    if let Some(b) = num("baud") {
        c.baud = u32::try_from(b).map_err(|_| "baud out of range")?;
    }
    if let Some(d) = num("data_bits") {
        c.data_bits = u8::try_from(d).map_err(|_| "data_bits out of range")?;
    }
    if let Some(s) = num("stop_bits") {
        c.stop_bits = u8::try_from(s).map_err(|_| "stop_bits out of range")?;
    }
    if let Some(p) = text("parity") {
        c.parity = p;
    }
    if let Some(f) = text("flow_control") {
        c.flow_control = f;
    }
    // Line endings are applied by the frontend; the bridge sends bytes verbatim
    c.line_ending = "none".into();
    c.receive_only = obj
        .get("receive_only")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    c.validate().map_err(|e| e.0)?;
    Ok(c)
}

/// Run until stdin closes or a close request arrives.
pub fn run() -> io::Result<u8> {
    let (tx, requests) = mpsc::channel::<Option<String>>();
    thread::spawn(move || {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            match line {
                Ok(l) => {
                    if tx.send(Some(l)).is_err() {
                        return;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = tx.send(None);
    });

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut monitor: Option<SerialMonitor> = None;
    let mut reported_drops = 0u64;

    loop {
        // Requests (non-blocking with a short wait so data events stay prompt)
        match requests.recv_timeout(Duration::from_millis(15)) {
            Ok(None) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                drop(monitor.take());
                return Ok(0);
            }
            Ok(Some(line)) if !line.trim().is_empty() => {
                let req: Value = match serde_json::from_str(&line) {
                    Ok(v) => v,
                    Err(e) => {
                        emit(
                            &mut out,
                            json!({"event":"error","op":null,"message":format!("Invalid JSON: {e}")}),
                        )?;
                        continue;
                    }
                };
                match req.get("op").and_then(Value::as_str).unwrap_or("") {
                    "list" => match serial_port_entries() {
                        Ok(ports) => emit(&mut out, json!({"event":"ports","ports":ports}))?,
                        Err(e) => {
                            emit(&mut out, json!({"event":"error","op":"list","message":e.0}))?
                        }
                    },
                    "open" => {
                        drop(monitor.take());
                        reported_drops = 0;
                        match parse_config(req.get("config").unwrap_or(&Value::Null)) {
                            Ok(cfg) => match SerialMonitor::connect(&cfg) {
                                Ok(m) => {
                                    emit(
                                        &mut out,
                                        json!({"event":"opened","port":cfg.port,"baud":cfg.baud}),
                                    )?;
                                    monitor = Some(m);
                                }
                                Err(e) => emit(
                                    &mut out,
                                    json!({"event":"error","op":"open","message":e.0}),
                                )?,
                            },
                            Err(e) => {
                                emit(&mut out, json!({"event":"error","op":"open","message":e}))?
                            }
                        }
                    }
                    "write" => {
                        let Some(m) = monitor.as_ref() else {
                            emit(
                                &mut out,
                                json!({"event":"error","op":"write","message":"Port is not open"}),
                            )?;
                            continue;
                        };
                        match req.get("hex").and_then(Value::as_str).and_then(from_hex) {
                            Some(bytes) => {
                                if let Err(e) = m.send_bytes(&bytes) {
                                    emit(
                                        &mut out,
                                        json!({"event":"error","op":"write","message":e.0}),
                                    )?;
                                }
                            }
                            None => emit(
                                &mut out,
                                json!({"event":"error","op":"write","message":"write.hex must be even-length hex"}),
                            )?,
                        }
                    }
                    "close" => {
                        drop(monitor.take());
                        emit(&mut out, json!({"event":"closed"}))?;
                    }
                    other => emit(
                        &mut out,
                        json!({"event":"error","op":other,"message":"Unknown op"}),
                    )?,
                }
            }
            Ok(Some(_)) | Err(mpsc::RecvTimeoutError::Timeout) => {}
        }

        // Device events
        let mut lost = None;
        if let Some(m) = monitor.as_ref() {
            for event in m.drain() {
                match event {
                    SerialEvent::Data(bytes) => {
                        emit(&mut out, json!({"event":"data","hex":to_hex(&bytes)}))?
                    }
                    SerialEvent::Error(message) => lost = Some(message),
                }
            }
            let dropped = m.dropped_bytes();
            if dropped > reported_drops {
                emit(
                    &mut out,
                    json!({"event":"dropped","bytes":dropped - reported_drops}),
                )?;
                reported_drops = dropped;
            }
            if lost.is_none() && m.is_finished() {
                lost = Some("Serial worker stopped".into());
            }
        }
        if let Some(message) = lost {
            drop(monitor.take());
            emit(&mut out, json!({"event":"disconnected","message":message}))?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips_binary_and_rejects_bad_input() {
        let all: Vec<u8> = (0..=255).collect();
        assert_eq!(from_hex(&to_hex(&all)).unwrap(), all);
        assert_eq!(from_hex("0A0b"), Some(vec![10, 11]));
        assert_eq!(from_hex("abc"), None);
        assert_eq!(from_hex("zz"), None);
    }

    #[test]
    fn config_requires_port_and_valid_framing() {
        assert!(parse_config(&json!({})).is_err());
        assert!(parse_config(&json!({"port":"/dev/x","baud":0})).is_err());
        assert!(parse_config(&json!({"port":"/dev/x","parity":"mark"})).is_err());
        let c = parse_config(
            &json!({"port":"/dev/x","baud":9600,"data_bits":7,"parity":"even","stop_bits":2}),
        )
        .unwrap();
        assert_eq!(
            (c.baud, c.data_bits, c.parity.as_str(), c.stop_bits),
            (9600, 7, "even", 2)
        );
        assert_eq!(c.line_ending, "none");
    }
}

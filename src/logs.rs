//! Real-time watcher over the unified log via `log stream`, filtered inside logd to keep it cheap.

use serde::Deserialize;
use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

const PREDICATE: &str = r#"messageType == error OR messageType == fault OR process == "kernel" OR process IN {"sudo", "authd", "loginwindow", "securityd", "syspolicyd", "powerd", "launchd", "trustd"}"#;
const CAPACITY: usize = 5000;
const MAX_MESSAGE: usize = 2000;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Level {
    Fault,
    Error,
    Default,
}

#[derive(Clone, Debug)]
pub struct LogEntry {
    pub time: String,
    pub level: Level,
    pub process: String,
    pub subsystem: String,
    pub message: String,
}

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum Source {
    #[default]
    All,
    Problems,
    Kernel,
    Security,
    Power,
}

impl Source {
    pub const ALL: [(Source, &str); 5] = [
        (Source::All, "All key logs"),
        (Source::Problems, "Errors & faults"),
        (Source::Kernel, "Kernel"),
        (Source::Security, "Security"),
        (Source::Power, "Power"),
    ];
}

impl LogEntry {
    pub fn matches(&self, source: Source, query_lower: &str) -> bool {
        let in_source = match source {
            Source::All => true,
            Source::Problems => self.level != Level::Default,
            Source::Kernel => self.process == "kernel",
            Source::Security => {
                matches!(
                    self.process.as_str(),
                    "sudo" | "authd" | "loginwindow" | "securityd" | "syspolicyd" | "trustd"
                ) || self.subsystem.starts_with("com.apple.securityd")
            }
            Source::Power => self.process == "powerd" || self.subsystem.contains("power"),
        };
        in_source
            && (query_lower.is_empty()
                || self.message.to_lowercase().contains(query_lower)
                || self.process.to_lowercase().contains(query_lower)
                || self.subsystem.to_lowercase().contains(query_lower))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Raw<'a> {
    #[serde(borrow)]
    timestamp: std::borrow::Cow<'a, str>,
    #[serde(borrow)]
    message_type: Option<std::borrow::Cow<'a, str>>,
    #[serde(borrow, default)]
    process_image_path: std::borrow::Cow<'a, str>,
    #[serde(borrow, default)]
    subsystem: std::borrow::Cow<'a, str>,
    #[serde(borrow, default)]
    event_message: std::borrow::Cow<'a, str>,
}

/// `None` for the banner line and anything else that is not a log event.
pub fn parse_line(line: &str) -> Option<LogEntry> {
    let raw: Raw = serde_json::from_str(line).ok()?;
    let level = match raw.message_type.as_deref() {
        Some("Fault") => Level::Fault,
        Some("Error") => Level::Error,
        _ => Level::Default,
    };
    let process = raw
        .process_image_path
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_string();
    // "2026-10-31 18:39:56.286465-0700" -> "18:39:56.286"
    let time = raw
        .timestamp
        .get(11..23)
        .unwrap_or(&raw.timestamp)
        .to_string();
    let mut message = raw.event_message.replace(['\n', '\r'], " ");
    if message.len() > MAX_MESSAGE {
        message.truncate(message.floor_char_boundary(MAX_MESSAGE));
    }
    Some(LogEntry {
        time,
        level,
        process,
        subsystem: raw.subsystem.into_owned(),
        message,
    })
}

#[derive(Default)]
pub struct LogBuffer {
    pub pending: Mutex<VecDeque<LogEntry>>,
    pub status: Mutex<Option<String>>,
    pub problems: AtomicU64,
}

impl LogBuffer {
    fn push(&self, e: LogEntry) {
        if e.level != Level::Default {
            self.problems.fetch_add(1, Ordering::Relaxed);
        }
        let mut q = self.pending.lock().unwrap();
        if q.len() >= CAPACITY {
            q.pop_front();
        }
        q.push_back(e);
    }
}

// Keep the child handle until it has been reaped; a stored PID can be recycled by macOS.
static CHILD: Mutex<Option<Child>> = Mutex::new(None);
const MAX_RECORD: usize = 64 * 1024;

/// Discard oversized events without letting an untrusted log message grow the reader's buffer.
fn read_record(reader: &mut impl BufRead, line: &mut Vec<u8>) -> io::Result<bool> {
    line.clear();
    let mut oversized = false;
    let mut seen = false;
    loop {
        let chunk = reader.fill_buf()?;
        if chunk.is_empty() {
            return Ok(seen);
        }
        seen = true;
        let end = chunk.iter().position(|&b| b == b'\n').map(|n| n + 1);
        let n = end.unwrap_or(chunk.len());
        if !oversized && line.len() + n <= MAX_RECORD {
            line.extend_from_slice(&chunk[..n]);
        } else {
            oversized = true;
            line.clear();
        }
        reader.consume(n);
        if end.is_some() {
            return Ok(true);
        }
    }
}

pub fn start() -> Arc<LogBuffer> {
    let buf = Arc::new(LogBuffer::default());
    let out = buf.clone();
    let spawned = Command::new("/usr/bin/log")
        .args([
            "stream",
            "--style",
            "ndjson",
            "--level",
            "default",
            "--predicate",
            PREDICATE,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .spawn();
    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => {
            *buf.status.lock().unwrap() = Some(format!("Could not start log stream: {e}"));
            return buf;
        }
    };
    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");
    *CHILD.lock().unwrap() = Some(child);
    let errors = buf.clone();
    std::thread::Builder::new()
        .name("log-errors".into())
        .spawn(move || {
            let mut err = String::new();
            let _ = stderr.take(4096).read_to_string(&mut err);
            if !err.trim().is_empty() {
                *errors.status.lock().unwrap() = Some(err.trim().to_string());
            }
        })
        .expect("spawn log stderr reader");
    std::thread::Builder::new()
        .name("log-stream".into())
        .spawn(move || {
            let mut reader = BufReader::with_capacity(64 * 1024, stdout);
            let mut line = Vec::with_capacity(4096);
            loop {
                match read_record(&mut reader, &mut line) {
                    Ok(false) => break,
                    Ok(true) => {
                        if let Some(e) = std::str::from_utf8(&line).ok().and_then(parse_line) {
                            out.push(e);
                        }
                    }
                    Err(e) => {
                        *out.status.lock().unwrap() = Some(format!("Log read failed: {e}"));
                        break;
                    }
                }
            }
            let mut child = CHILD.lock().unwrap();
            if child
                .as_mut()
                .is_some_and(|c| c.try_wait().ok().flatten().is_some())
            {
                child.take();
            }
            let mut status = out.status.lock().unwrap();
            if status.is_none() {
                *status = Some("log stream exited; reopen the app to reconnect".into());
            }
        })
        .expect("spawn log thread");
    buf
}

pub fn stop() {
    if let Some(mut child) = CHILD.lock().unwrap().take() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ERROR: &str = r#"{"messageType":"Error","processImagePath":"\/usr\/libexec\/foo","timestamp":"2026-10-31 18:39:56.286465-0700","subsystem":"com.apple.x","eventMessage":"bad\nthing"}"#;

    #[::core::prelude::v1::test]
    fn parses_events_and_skips_banner() {
        assert!(parse_line(r#"Filtering the log data using "x""#).is_none());
        let e = parse_line(ERROR).unwrap();
        assert_eq!(e.level, Level::Error);
        assert_eq!(e.process, "foo");
        assert_eq!(e.time, "18:39:56.286");
        assert_eq!(e.message, "bad thing");
    }

    #[::core::prelude::v1::test]
    fn long_multibyte_messages_truncate_safely() {
        let msg = "é".repeat(MAX_MESSAGE);
        let line = serde_json::json!({"timestamp": "t", "eventMessage": msg}).to_string();
        assert!(parse_line(&line).unwrap().message.len() <= MAX_MESSAGE);
    }

    #[::core::prelude::v1::test]
    fn source_and_query_filters() {
        let e = parse_line(ERROR).unwrap();
        assert!(e.matches(Source::All, ""));
        assert!(e.matches(Source::Problems, "bad"));
        assert!(e.matches(Source::All, "com.apple.x"));
        assert!(!e.matches(Source::Kernel, ""));
        assert!(!e.matches(Source::All, "nope"));
    }

    #[::core::prelude::v1::test]
    fn oversized_record_is_discarded_and_next_record_is_read() {
        let mut input = vec![b'x'; MAX_RECORD + 20];
        input.extend_from_slice(b"\n");
        input.extend_from_slice(ERROR.as_bytes());
        input.extend_from_slice(b"\n");
        let mut reader = BufReader::with_capacity(17, &input[..]);
        let mut line = Vec::new();
        assert!(read_record(&mut reader, &mut line).unwrap());
        assert!(line.is_empty());
        assert!(read_record(&mut reader, &mut line).unwrap());
        assert!(parse_line(std::str::from_utf8(&line).unwrap()).is_some());
        assert!(!read_record(&mut reader, &mut line).unwrap());
    }

    #[::core::prelude::v1::test]
    fn buffer_is_bounded_and_counts_problems() {
        let b = LogBuffer::default();
        for _ in 0..CAPACITY + 10 {
            b.push(parse_line(ERROR).unwrap());
        }
        assert_eq!(b.pending.lock().unwrap().len(), CAPACITY);
        assert_eq!(b.problems.load(Ordering::Relaxed), CAPACITY as u64 + 10);
    }
}

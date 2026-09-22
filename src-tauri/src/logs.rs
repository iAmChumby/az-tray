use crate::types::{LogEntry, LogLevel, LogStream, LogsQuery, SaveLogsResult, ServiceName};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const MAX_PER_SERVICE: usize = 2_000;
pub const MAX_MERGED: usize = 6_000;

#[derive(Clone, Debug, Default)]
pub struct LogStore {
    next_sequence: u64,
    per_service: BTreeMap<ServiceName, Vec<LogEntry>>,
    merged: Vec<LogEntry>,
}

impl LogStore {
    pub fn push(&mut self, service: ServiceName, stream: LogStream, message: String) -> LogEntry {
        self.next_sequence = self.next_sequence.saturating_add(1);
        let level = match stream {
            LogStream::Stdout => LogLevel::Info,
            LogStream::Stderr => LogLevel::Warn,
            LogStream::System => LogLevel::Info,
        };
        let timestamp = now_timestamp();
        let entry = LogEntry {
            id: format!("{}-{}", timestamp.replace([':', '.', '-'], ""), self.next_sequence),
            sequence: self.next_sequence,
            service: service.clone(),
            stream: stream.clone(),
            level,
            message,
            timestamp,
        };
        let service_logs = self.per_service.entry(service).or_default();
        service_logs.push(entry.clone());
        if service_logs.len() > MAX_PER_SERVICE {
            let remove = service_logs.len() - MAX_PER_SERVICE;
            service_logs.drain(..remove);
        }
        self.merged.push(entry.clone());
        if self.merged.len() > MAX_MERGED {
            let remove = self.merged.len() - MAX_MERGED;
            self.merged.drain(..remove);
        }
        entry
    }

    pub fn query(&self, query: &LogsQuery) -> Vec<LogEntry> {
        let values = if let Some(service) = query.service_name.as_ref() {
            self.per_service.get(service).map(Vec::as_slice).unwrap_or(&[])
        } else {
            self.merged.as_slice()
        };
        take_tail(values, query.limit.unwrap_or(values.len()))
    }

    pub fn all(&self) -> (BTreeMap<ServiceName, Vec<LogEntry>>, Vec<LogEntry>) {
        // Keep the snapshot shape stable before any process has emitted a log
        // and after a service-specific clear. The frontend models this as a
        // Record<ServiceName, LogEntry[]> rather than an optional map entry.
        let mut per_service = self.per_service.clone();
        for service in [ServiceName::Blob, ServiceName::Queue, ServiceName::Table] {
            per_service.entry(service).or_default();
        }
        (per_service, self.merged.clone())
    }

    pub fn clear(&mut self, service: Option<&ServiceName>) {
        if let Some(service) = service {
            self.per_service.remove(service);
            self.merged.retain(|entry| &entry.service != service);
        } else {
            self.per_service.clear();
            self.merged.clear();
        }
    }

    pub fn save(&self, service: Option<&ServiceName>, path: Option<&str>) -> Result<SaveLogsResult, String> {
        let entries = if let Some(service) = service {
            self.per_service.get(service).map(Vec::as_slice).unwrap_or(&[])
        } else {
            self.merged.as_slice()
        };
        let output_path = path.map(PathBuf::from).unwrap_or_else(default_log_path);
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("could not create log directory: {error}"))?;
        }
        let file = File::create(&output_path).map_err(|error| format!("could not create {}: {error}", output_path.display()))?;
        let mut writer = BufWriter::new(file);
        for entry in entries {
            writeln!(
                writer,
                "[{}] [{}] [{}] {}",
                entry.timestamp,
                service_label(&entry.service),
                stream_label(&entry.stream),
                entry.message
            )
            .map_err(|error| format!("could not write logs: {error}"))?;
        }
        writer.flush().map_err(|error| format!("could not flush logs: {error}"))?;
        Ok(SaveLogsResult { path: output_path.to_string_lossy().into_owned(), line_count: entries.len() })
    }
}

pub fn now_timestamp() -> String {
    let duration = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let seconds = duration.as_secs();
    let millis = duration.subsec_millis();
    let days = seconds / 86_400;
    let day_seconds = seconds % 86_400;
    let (year, month, day) = civil_from_days(days as i64);
    let hour = day_seconds / 3_600;
    let minute = (day_seconds % 3_600) / 60;
    let second = day_seconds % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

// Howard Hinnant's civil date conversion, valid for the full Unix timestamp
// range without pulling a date/time dependency into the tray controller.
fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    (year, m, d)
}

fn default_log_path() -> PathBuf {
    let root = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_STATE_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|path| PathBuf::from(path).join(".local").join("state")))
        .unwrap_or_else(|| PathBuf::from("."));
    root.join("AzTray").join(format!("logs-{}.txt", now_timestamp().replace(['.', ':'], "-")))
}

fn take_tail(values: &[LogEntry], limit: usize) -> Vec<LogEntry> {
    let start = values.len().saturating_sub(limit);
    values[start..].to_vec()
}

fn service_label(service: &ServiceName) -> &'static str {
    match service {
        ServiceName::Blob => "blob",
        ServiceName::Queue => "queue",
        ServiceName::Table => "table",
    }
}

fn stream_label(stream: &LogStream) -> &'static str {
    match stream {
        LogStream::Stdout => "stdout",
        LogStream::Stderr => "stderr",
        LogStream::System => "system",
    }
}

#[allow(dead_code)]
fn _path_is_absolute(path: &Path) -> bool {
    path.is_absolute()
}

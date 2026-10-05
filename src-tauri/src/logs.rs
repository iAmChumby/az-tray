use crate::config;
use crate::types::{LogEntry, LogLevel, LogStream, LogsQuery, SaveLogsResult, ServiceName};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub const MAX_PER_SERVICE: usize = 2_000;
pub const MAX_MERGED: usize = 6_000;
/// `aztray.log` rotates to `aztray.log.1` past this size.
pub const APP_LOG_MAX_BYTES: u64 = 1_024 * 1_024;

type Key = (String, ServiceName);

/// In-memory Azurite output, keyed by `(instance_id, service)`. Each instance
/// also keeps a merged arrival-order buffer.
#[derive(Clone, Debug, Default)]
pub struct LogStore {
    next_sequence: u64,
    per_service: BTreeMap<Key, Vec<LogEntry>>,
    merged: BTreeMap<String, Vec<LogEntry>>,
}

impl LogStore {
    pub fn push(&mut self, instance_id: &str, service: ServiceName, stream: LogStream, message: String) -> LogEntry {
        let level = match stream {
            LogStream::Stdout => LogLevel::Info,
            LogStream::Stderr => LogLevel::Warn,
            LogStream::System => LogLevel::Info,
        };
        self.push_with_level(instance_id, service, stream, level, message)
    }

    pub fn push_with_level(
        &mut self,
        instance_id: &str,
        service: ServiceName,
        stream: LogStream,
        level: LogLevel,
        message: String,
    ) -> LogEntry {
        self.next_sequence = self.next_sequence.saturating_add(1);
        let timestamp = now_timestamp();
        let entry = LogEntry {
            id: format!("{}-{}", timestamp.replace([':', '.', '-'], ""), self.next_sequence),
            sequence: self.next_sequence,
            instance_id: instance_id.to_string(),
            service: service.clone(),
            stream,
            level,
            message,
            timestamp,
        };
        let service_logs = self.per_service.entry((instance_id.to_string(), service)).or_default();
        service_logs.push(entry.clone());
        if service_logs.len() > MAX_PER_SERVICE {
            let remove = service_logs.len() - MAX_PER_SERVICE;
            service_logs.drain(..remove);
        }
        let merged = self.merged.entry(instance_id.to_string()).or_default();
        merged.push(entry.clone());
        if merged.len() > MAX_MERGED {
            let remove = merged.len() - MAX_MERGED;
            merged.drain(..remove);
        }
        entry
    }

    fn entries(&self, instance_id: &str, service: Option<&ServiceName>) -> &[LogEntry] {
        match service {
            Some(service) => self
                .per_service
                .get(&(instance_id.to_string(), service.clone()))
                .map(Vec::as_slice)
                .unwrap_or(&[]),
            None => self.merged.get(instance_id).map(Vec::as_slice).unwrap_or(&[]),
        }
    }

    /// Entries for one instance (`query.instance_id` is already resolved by the
    /// caller and passed as `instance_id`).
    pub fn query(&self, instance_id: &str, query: &LogsQuery) -> Vec<LogEntry> {
        let values = self.entries(instance_id, query.service_name.as_ref());
        take_tail(values, query.limit.unwrap_or(values.len()))
    }

    /// Per-service and merged logs for one instance, each trimmed to the newest
    /// `per_service_cap` / `merged_cap` entries. The map always holds all three
    /// services so the frontend's `Record<ServiceName, LogEntry[]>` stays total.
    pub fn instance_logs(
        &self,
        instance_id: &str,
        per_service_cap: usize,
        merged_cap: usize,
    ) -> (BTreeMap<ServiceName, Vec<LogEntry>>, Vec<LogEntry>) {
        let mut per_service = BTreeMap::new();
        for service in [ServiceName::Blob, ServiceName::Queue, ServiceName::Table] {
            let entries = take_tail(self.entries(instance_id, Some(&service)), per_service_cap);
            per_service.insert(service, entries);
        }
        (per_service, take_tail(self.entries(instance_id, None), merged_cap))
    }

    pub fn clear(&mut self, instance_id: &str, service: Option<&ServiceName>) {
        if let Some(service) = service {
            self.per_service.remove(&(instance_id.to_string(), service.clone()));
            if let Some(merged) = self.merged.get_mut(instance_id) {
                merged.retain(|entry| &entry.service != service);
            }
        } else {
            self.remove_instance(instance_id);
        }
    }

    pub fn remove_instance(&mut self, instance_id: &str) {
        self.per_service.retain(|(id, _), _| id != instance_id);
        self.merged.remove(instance_id);
    }

    pub fn save(
        &self,
        instance_id: &str,
        service: Option<&ServiceName>,
        path: Option<&str>,
    ) -> Result<SaveLogsResult, String> {
        let entries = self.entries(instance_id, service);
        let output_path = path.map(PathBuf::from).unwrap_or_else(|| default_log_path(instance_id));
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("could not create log directory: {error}"))?;
        }
        let file = File::create(&output_path).map_err(|error| format!("could not create {}: {error}", output_path.display()))?;
        let mut writer = BufWriter::new(file);
        for entry in entries {
            writeln!(
                writer,
                "[{}] [{}] [{}] [{}] {}",
                entry.timestamp,
                entry.instance_id,
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

fn default_log_path(instance_id: &str) -> PathBuf {
    let root = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_STATE_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|path| PathBuf::from(path).join(".local").join("state")))
        .unwrap_or_else(|| PathBuf::from("."));
    root.join("AzTray")
        .join(format!("logs-{instance_id}-{}.txt", now_timestamp().replace(['.', ':'], "-")))
}

fn take_tail(values: &[LogEntry], limit: usize) -> Vec<LogEntry> {
    let start = values.len().saturating_sub(limit);
    values[start..].to_vec()
}

fn service_label(service: &ServiceName) -> &'static str {
    config::service_label(service)
}

fn stream_label(stream: &LogStream) -> &'static str {
    match stream {
        LogStream::Stdout => "stdout",
        LogStream::Stderr => "stderr",
        LogStream::System => "system",
    }
}

// ---------------------------------------------------------------------------
// Persistent application log (%APPDATA%\AzTray\logs\aztray.log)
// ---------------------------------------------------------------------------

static APP_LOG_LOCK: Mutex<()> = Mutex::new(());

fn level_label(level: &LogLevel) -> &'static str {
    match level {
        LogLevel::Info => "INFO",
        LogLevel::Warn => "WARN",
        LogLevel::Error => "ERROR",
    }
}

/// Append one line to the persistent app log. Thread-safe, rotates at
/// `APP_LOG_MAX_BYTES`, never panics. A failure is reported once to stderr
/// (invisible in the release build, but useful in dev) and then swallowed.
pub fn app_log(level: LogLevel, message: &str) {
    app_log_to(&config::app_log_path(), level, message);
}

pub fn app_log_to(path: &Path, level: LogLevel, message: &str) {
    let _guard = APP_LOG_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Err(error) = append_line(path, &format!("{} {:<5} {}", now_timestamp(), level_label(&level), message.replace(['\r', '\n'], " "))) {
        static WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            eprintln!("AzTray could not write {}: {error}", path.display());
        }
    }
}

fn append_line(path: &Path, line: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if fs::metadata(path).map(|meta| meta.len() >= APP_LOG_MAX_BYTES).unwrap_or(false) {
        let mut rotated = path.as_os_str().to_owned();
        rotated.push(".1");
        let rotated = PathBuf::from(rotated);
        let _ = fs::remove_file(&rotated);
        fs::rename(path, &rotated)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{line}")
}

/// Last `limit` lines of the app log, oldest first.
pub fn app_log_tail(limit: usize) -> Vec<String> {
    tail_lines(&config::app_log_path(), limit)
}

pub fn tail_lines(path: &Path, limit: usize) -> Vec<String> {
    let _guard = APP_LOG_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Ok(bytes) = fs::read(path) else { return Vec::new() };
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(limit);
    lines[start..].iter().map(|line| line.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query_all() -> LogsQuery {
        LogsQuery { instance_id: None, service_name: None, limit: None }
    }

    #[test]
    fn push_with_level_preserves_error_level_for_exported_diagnostics() {
        let mut store = LogStore::default();
        let entry = store.push_with_level(
            "default",
            ServiceName::Blob,
            LogStream::System,
            LogLevel::Error,
            "could not start azurite-blob: access denied".into(),
        );

        assert!(matches!(entry.level, LogLevel::Error));
        assert_eq!(entry.instance_id, "default");
        let logs = store.query("default", &query_all());
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].message, "could not start azurite-blob: access denied");
    }

    #[test]
    fn logs_are_isolated_per_instance() {
        let mut store = LogStore::default();
        store.push("a", ServiceName::Blob, LogStream::Stdout, "from a".into());
        store.push("b", ServiceName::Blob, LogStream::Stdout, "from b".into());
        assert_eq!(store.query("a", &query_all()).len(), 1);
        assert_eq!(store.query("b", &query_all())[0].message, "from b");
        store.clear("a", Some(&ServiceName::Blob));
        assert!(store.query("a", &query_all()).is_empty());
        assert_eq!(store.query("b", &query_all()).len(), 1);
        let (per_service, merged) = store.instance_logs("a", 10, 10);
        assert_eq!(per_service.len(), 3);
        assert!(merged.is_empty());
        store.remove_instance("b");
        assert!(store.query("b", &query_all()).is_empty());
    }

    #[test]
    fn save_exports_prelaunch_diagnostic_message() {
        let mut store = LogStore::default();
        store.push_with_level(
            "default",
            ServiceName::Queue,
            LogStream::System,
            LogLevel::Error,
            "start failed before process launch: executable missing".into(),
        );

        let path = std::env::temp_dir().join(format!(
            "aztray-log-test-{}-{}.txt",
            std::process::id(),
            store.query("default", &query_all())[0].sequence
        ));
        let result = store.save("default", None, Some(path.to_string_lossy().as_ref())).expect("logs save");
        let contents = std::fs::read_to_string(&path).expect("saved logs");

        assert_eq!(result.line_count, 1);
        assert!(contents.contains("start failed before process launch: executable missing"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn app_log_appends_rotates_and_tails() {
        let dir = std::env::temp_dir().join(format!("aztray-applog-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("logs").join("aztray.log");
        app_log_to(&path, LogLevel::Info, "first\nline");
        app_log_to(&path, LogLevel::Error, "second");
        let lines = tail_lines(&path, 10);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("INFO") && lines[0].ends_with("first line"));
        assert!(lines[1].contains("ERROR") && lines[1].ends_with("second"));
        assert_eq!(tail_lines(&path, 1).len(), 1);

        fs::write(&path, vec![b'x'; (APP_LOG_MAX_BYTES + 1) as usize]).unwrap();
        app_log_to(&path, LogLevel::Info, "after rotation");
        assert!(dir.join("logs").join("aztray.log.1").is_file());
        assert_eq!(tail_lines(&path, 10).len(), 1);
        let _ = fs::remove_dir_all(dir);
    }
}

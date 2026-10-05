use crate::types::{
    AppConfig, InstanceConfig, McpConfig, ServiceName, CONFIG_SCHEMA_VERSION, DEFAULT_INSTANCE_ID,
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_HOST: &str = "127.0.0.1";
pub const DEFAULT_BLOB_PORT: u16 = 10_000;
pub const DEFAULT_QUEUE_PORT: u16 = 10_001;
pub const DEFAULT_TABLE_PORT: u16 = 10_002;
/// First port tried for auto-allocated instance trios, and the last blob port
/// a trio may start on (stride 3).
pub const ALLOC_START_PORT: u16 = 10_000;
pub const ALLOC_END_PORT: u16 = 19_998;
/// The MCP supervisor tries `port ..= port + MCP_PORT_SPAN`.
pub const MCP_PORT_SPAN: u16 = 9;
pub const MAX_NAME_CHARS: usize = 48;
pub const MAX_ID_CHARS: usize = 32;

const SERVICES: [ServiceName; 3] = [ServiceName::Blob, ServiceName::Queue, ServiceName::Table];

#[derive(Clone, Debug)]
pub struct LoadedConfig {
    pub config: AppConfig,
    /// Non-fatal warning (migration failure, malformed file, validation error).
    pub error: Option<String>,
    pub source: Option<PathBuf>,
}

pub fn default_ports() -> BTreeMap<ServiceName, u16> {
    let mut ports = BTreeMap::new();
    ports.insert(ServiceName::Blob, DEFAULT_BLOB_PORT);
    ports.insert(ServiceName::Queue, DEFAULT_QUEUE_PORT);
    ports.insert(ServiceName::Table, DEFAULT_TABLE_PORT);
    ports
}

pub fn default_instance() -> InstanceConfig {
    InstanceConfig {
        id: DEFAULT_INSTANCE_ID.to_string(),
        name: "Default".to_string(),
        host: DEFAULT_HOST.to_string(),
        ports: default_ports(),
        data_directory: default_data_directory().to_string_lossy().into_owned(),
        loose: false,
        skip_api_version_check: false,
    }
}

pub fn default_config() -> AppConfig {
    AppConfig {
        schema_version: CONFIG_SCHEMA_VERSION,
        executable_path: None,
        node_path: None,
        mcp: McpConfig::default(),
        instances: vec![default_instance()],
    }
}

pub fn app_data_directory() -> PathBuf {
    if let Some(path) = std::env::var_os("APPDATA") {
        return PathBuf::from(path).join("AzTray");
    }
    if let Some(path) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(path).join("AzTray");
    }
    if let Some(path) = std::env::var_os("HOME") {
        return PathBuf::from(path).join(".config").join("AzTray");
    }
    PathBuf::from("AzTray")
}

pub fn config_path() -> PathBuf {
    app_data_directory().join("config.json")
}

/// `%APPDATA%\AzTray\logs\aztray.log`
pub fn app_log_path() -> PathBuf {
    app_data_directory().join("logs").join("aztray.log")
}

pub fn default_data_directory() -> PathBuf {
    if let Some(path) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(path).join("azurite");
    }
    if let Some(path) = std::env::var_os("USERPROFILE") {
        return PathBuf::from(path).join(".azurite");
    }
    if let Some(path) = std::env::var_os("HOME") {
        return PathBuf::from(path).join(".azurite");
    }
    PathBuf::from(".azurite")
}

/// Default data directory for a non-default instance:
/// `%LOCALAPPDATA%\AzTray\instances\<id>\data`.
pub fn instance_data_directory(id: &str) -> PathBuf {
    let root = std::env::var_os("LOCALAPPDATA")
        .map(|path| PathBuf::from(path).join("AzTray"))
        .unwrap_or_else(app_data_directory);
    root.join("instances").join(id).join("data")
}

pub fn azctl_config_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|path| PathBuf::from(path).join("azctl").join("config.json"))
}

pub fn service_label(service: &ServiceName) -> &'static str {
    match service {
        ServiceName::Blob => "blob",
        ServiceName::Queue => "queue",
        ServiceName::Table => "table",
    }
}

// ---------------------------------------------------------------------------
// Identity helpers
// ---------------------------------------------------------------------------

pub fn is_valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    let Some(first) = chars.next() else { return false };
    if id.len() > MAX_ID_CHARS || !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
        return false;
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Lowercase slug: runs of anything outside `[a-z0-9]` become one `-`, ends
/// trimmed, capped at 32 chars. Falls back to `instance` when nothing remains.
pub fn slugify(name: &str) -> String {
    let mut slug = String::new();
    let mut pending_dash = false;
    for c in name.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            pending_dash = false;
            slug.push(c);
        } else {
            pending_dash = true;
        }
    }
    slug.truncate(MAX_ID_CHARS);
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "instance".to_string()
    } else {
        slug
    }
}

/// `base`, then `base-2`, `base-3`, ... until it does not collide with `taken`.
pub fn unique_id(base: &str, taken: &[String]) -> String {
    let exists = |candidate: &str| taken.iter().any(|id| id == candidate);
    if !exists(base) {
        return base.to_string();
    }
    for n in 2u32.. {
        let suffix = format!("-{n}");
        let keep = MAX_ID_CHARS.saturating_sub(suffix.len());
        let mut stem = base.to_string();
        stem.truncate(keep);
        let candidate = format!("{}{suffix}", stem.trim_end_matches('-'));
        if !exists(&candidate) {
            return candidate;
        }
    }
    unreachable!("an unused id always exists")
}

/// Normalize a directory for collision checks: absolute, forward slashes
/// folded to backslashes, trailing separators trimmed, lowercased.
pub fn normalize_dir(dir: &str) -> String {
    let trimmed = dir.trim();
    let absolute = std::path::absolute(trimmed)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|_| trimmed.to_string());
    absolute
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

fn dirs_overlap(a: &str, b: &str) -> bool {
    let (a, b) = (normalize_dir(a), normalize_dir(b));
    a == b || a.starts_with(&format!("{b}\\")) || b.starts_with(&format!("{a}\\"))
}

pub fn mcp_port_range(mcp: &McpConfig) -> (u16, u16) {
    (mcp.port, mcp.port.saturating_add(MCP_PORT_SPAN))
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Validate one instance on its own (no cross-instance checks).
pub fn validate_instance(instance: &InstanceConfig) -> Result<(), String> {
    if !is_valid_id(&instance.id) {
        return Err(format!(
            "instance id \"{}\" is invalid; use 1-{MAX_ID_CHARS} lowercase letters, digits, or hyphens, starting with a letter or digit",
            instance.id
        ));
    }
    let name_chars = instance.name.trim().chars().count();
    if name_chars == 0 || name_chars > MAX_NAME_CHARS {
        return Err(format!("instance name must be 1 to {MAX_NAME_CHARS} characters"));
    }
    if instance.host.trim().is_empty() {
        return Err("host cannot be empty".into());
    }
    if instance.ports.len() != 3 {
        return Err("an instance must include blob, queue, and table ports".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    for service in SERVICES {
        let port = instance
            .ports
            .get(&service)
            .copied()
            .ok_or_else(|| format!("missing {} port", service_label(&service)))?;
        if port == 0 {
            return Err(format!("{} port must be between 1 and 65535", service_label(&service)));
        }
        if !seen.insert(port) {
            return Err("service ports must be distinct".into());
        }
    }
    if instance.data_directory.trim().is_empty() {
        return Err("data directory cannot be empty".into());
    }
    Ok(())
}

/// Check `candidate` against every other instance (skipping `ignore_id`) and
/// the MCP port range. Messages name the owning instance.
pub fn check_instance_conflicts(
    config: &AppConfig,
    candidate: &InstanceConfig,
    ignore_id: Option<&str>,
) -> Result<(), String> {
    let (mcp_low, mcp_high) = mcp_port_range(&config.mcp);
    for service in SERVICES {
        let Some(&port) = candidate.ports.get(&service) else { continue };
        if (mcp_low..=mcp_high).contains(&port) {
            return Err(format!(
                "{} port {port} falls inside the MCP port range {mcp_low}-{mcp_high}",
                service_label(&service)
            ));
        }
    }
    let wanted_name = candidate.name.trim().to_lowercase();
    for other in config.instances.iter().filter(|i| Some(i.id.as_str()) != ignore_id) {
        if other.id == candidate.id {
            return Err(format!("instance id \"{}\" is already in use", candidate.id));
        }
        if other.name.trim().to_lowercase() == wanted_name {
            return Err(format!("instance name \"{}\" is already used by instance \"{}\"", candidate.name.trim(), other.id));
        }
        for service in SERVICES {
            let Some(&port) = candidate.ports.get(&service) else { continue };
            if other.ports.values().any(|p| *p == port) {
                return Err(format!(
                    "{} port {port} is already assigned to instance \"{}\"",
                    service_label(&service),
                    other.name
                ));
            }
        }
        if dirs_overlap(&candidate.data_directory, &other.data_directory) {
            return Err(format!(
                "data directory {} overlaps instance \"{}\" ({}); each instance needs its own directory that is not nested in another",
                candidate.data_directory, other.name, other.data_directory
            ));
        }
    }
    Ok(())
}

pub fn validate(config: &AppConfig) -> Result<(), String> {
    if config.schema_version != CONFIG_SCHEMA_VERSION {
        return Err(format!(
            "unsupported config schemaVersion {} (expected {CONFIG_SCHEMA_VERSION})",
            config.schema_version
        ));
    }
    if config.instances.is_empty() {
        return Err("at least one instance is required".into());
    }
    if config.mcp.port < 1024 {
        return Err("mcp.port must be between 1024 and 65535".into());
    }
    for (index, instance) in config.instances.iter().enumerate() {
        validate_instance(instance).map_err(|e| format!("instance \"{}\": {e}", instance.id))?;
        // Compare against every earlier instance plus the MCP range.
        let earlier = AppConfig {
            instances: config.instances[..index].to_vec(),
            ..config.clone()
        };
        check_instance_conflicts(&earlier, instance, None)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Loading / migration
// ---------------------------------------------------------------------------

/// Load the AzTray config (see docs/MULTI-INSTANCE-PLAN.md section 2).
pub fn load() -> LoadedConfig {
    load_from(&config_path(), azctl_config_path().as_deref())
}

pub fn load_from(own: &Path, azctl: Option<&Path>) -> LoadedConfig {
    if own.is_file() {
        return load_existing(own, own);
    }
    if let Some(azctl) = azctl {
        if azctl.is_file() {
            let mut loaded = load_existing(azctl, own);
            loaded.source = Some(azctl.to_path_buf());
            return loaded;
        }
    }
    LoadedConfig { config: default_config(), error: None, source: None }
}

/// Read `path`; `own` is where the AzTray v2 file lives (differs from `path`
/// for the azctl import, which is never written).
fn load_existing(path: &Path, own: &Path) -> LoadedConfig {
    let source = Some(path.to_path_buf());
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => {
            return LoadedConfig {
                config: default_config(),
                error: Some(format!("could not read {}: {error}", path.display())),
                source,
            }
        }
    };
    let value: Value = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(error) => {
            return LoadedConfig {
                config: default_config(),
                error: Some(format!("malformed config {}: {error}", path.display())),
                source,
            }
        }
    };
    let is_v2 = value.get("instances").map(Value::is_array).unwrap_or(false);
    if is_v2 {
        return match parse_v2(&value) {
            Ok(config) => {
                let error = validate(&config).err();
                LoadedConfig { config, error, source }
            }
            Err(error) => {
                // The file holds user instances we cannot read. Preserve it
                // before anything can overwrite it with the defaults.
                let note = match backup_unusable(path, &text) {
                    Ok(backup) => format!("; the original was saved to {}", backup.display()),
                    Err(backup_error) => format!("; {backup_error}"),
                };
                LoadedConfig {
                    config: default_config(),
                    error: Some(format!("malformed config {}: {error}{note}", path.display())),
                    source,
                }
            }
        };
    }

    // v1 (or azctl) -> single "default" instance.
    let config = match parse_v1(&value) {
        Ok(config) => config,
        Err(error) => {
            return LoadedConfig {
                config: default_config(),
                error: Some(format!("malformed config {}: {error}", path.display())),
                source,
            }
        }
    };
    let mut error = validate(&config).err();
    if error.is_none() {
        if let Err(migration) = migrate_files(path, own, &text, &config) {
            error = Some(migration);
        }
    }
    LoadedConfig { config, error, source }
}

/// Back up the v1 file (once) and write the v2 config to `own`.
fn migrate_files(path: &Path, own: &Path, original_text: &str, config: &AppConfig) -> Result<(), String> {
    if path == own {
        let backup = own.with_file_name("config.v1.json.bak");
        if !backup.exists() {
            fs::write(&backup, original_text)
                .map_err(|e| format!("could not back up v1 config to {}: {e}; running with the migrated config in memory", backup.display()))?;
        }
    }
    write_config(own, config)
        .map_err(|e| format!("could not write migrated config: {e}; running with the migrated config in memory"))
}

fn parse_v2(value: &Value) -> Result<AppConfig, serde_json::Error> {
    let mut value = value.clone();
    if let Some(object) = value.as_object_mut() {
        object
            .entry("schemaVersion")
            .or_insert_with(|| Value::from(CONFIG_SCHEMA_VERSION));
        object
            .entry("mcp")
            .or_insert_with(|| serde_json::to_value(McpConfig::default()).unwrap_or(Value::Null));
    }
    serde_json::from_value(value)
}

fn invalid_data(message: impl Into<String>) -> serde_json::Error {
    serde_json::Error::io(std::io::Error::new(std::io::ErrorKind::InvalidData, message.into()))
}

fn parse_v1(value: &Value) -> Result<AppConfig, serde_json::Error> {
    let defaults = default_instance();
    let object = value
        .as_object()
        .ok_or_else(|| invalid_data("config must be a JSON object"))?;

    let host = string_field(object, &["host", "bindHost", "bind_host"]).unwrap_or_else(|| defaults.host.clone());
    let data_directory = string_field(
        object,
        &["dataDirectory", "data_directory", "dataDir", "data_dir", "location"],
    )
    .unwrap_or_else(|| defaults.data_directory.clone());
    let executable_path = string_field(
        object,
        &["executablePath", "executable_path", "azuritePath", "azurite_path"],
    );
    let node_path = string_field(object, &["nodePath", "node_path"]);

    let mut ports = defaults.ports;
    if let Some(port_object) = object.get("ports").and_then(Value::as_object) {
        for (name, service) in [
            ("blob", ServiceName::Blob),
            ("queue", ServiceName::Queue),
            ("table", ServiceName::Table),
        ] {
            if let Some(value) = port_object.get(name).or_else(|| port_object.get(&format!("{name}Port"))) {
                ports.insert(service, parse_port(value, name)?);
            }
        }
    }
    for (name, service) in [
        ("blobPort", ServiceName::Blob),
        ("blob_port", ServiceName::Blob),
        ("queuePort", ServiceName::Queue),
        ("queue_port", ServiceName::Queue),
        ("tablePort", ServiceName::Table),
        ("table_port", ServiceName::Table),
    ] {
        if let Some(value) = object.get(name) {
            ports.insert(service, parse_port(value, name)?);
        }
    }

    Ok(AppConfig {
        schema_version: CONFIG_SCHEMA_VERSION,
        executable_path,
        node_path,
        mcp: McpConfig::default(),
        instances: vec![InstanceConfig {
            id: DEFAULT_INSTANCE_ID.to_string(),
            name: "Default".to_string(),
            host,
            ports,
            data_directory,
            loose: false,
            skip_api_version_check: false,
        }],
    })
}

fn parse_port(value: &Value, name: &str) -> Result<u16, serde_json::Error> {
    if let Some(port) = value.as_u64() {
        return u16::try_from(port).map_err(|_| invalid_data(format!("{name} is outside the valid port range")));
    }
    if let Some(port) = value.as_str() {
        return port.parse::<u16>().map_err(|_| invalid_data(format!("{name} must be a port number")));
    }
    Err(invalid_data(format!("{name} must be a port number")))
}

fn string_field(object: &serde_json::Map<String, Value>, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        object
            .get(*name)
            .and_then(Value::as_str)
            .map(|value| expand_environment(value.trim()))
            .filter(|value| !value.is_empty())
    })
}

fn expand_environment(value: &str) -> String {
    let mut result = value.to_string();
    for (key, value) in std::env::vars() {
        result = result.replace(&format!("%{key}%"), &value);
        result = result.replace(&format!("${{{key}}}"), &value);
    }
    result
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

pub fn persist(config: &AppConfig) -> Result<(), String> {
    persist_to(&config_path(), config)
}

pub fn persist_to(path: &Path, config: &AppConfig) -> Result<(), String> {
    validate(config)?;
    ensure_backups(path)?;
    write_config(path, config)
}

/// Before overwriting an existing file, keep a copy of anything we would
/// destroy: a malformed or unparseable-v2 file -> `config.broken.json.bak`, an
/// unmigrated v1 file -> `config.v1.json.bak`. Existing backups are never
/// overwritten. Persisting is refused (an error is returned) when the file
/// cannot be safely backed up, so a broken config is never silently lost.
fn ensure_backups(path: &Path) -> Result<(), String> {
    let Ok(text) = fs::read_to_string(path) else { return Ok(()) };
    match serde_json::from_str::<Value>(&text) {
        Err(_) => backup_unusable(path, &text).map(|_| ()),
        Ok(value) if !value.get("instances").map(Value::is_array).unwrap_or(false) => {
            let backup = path.with_file_name("config.v1.json.bak");
            if !backup.exists() {
                let _ = fs::write(backup, text);
            }
            Ok(())
        }
        Ok(value) => match parse_v2(&value) {
            Ok(_) => Ok(()),
            Err(_) => backup_unusable(path, &text).map(|_| ()),
        },
    }
}

/// Ensure a copy of an unreadable config exists: reuse a backup with identical
/// content, otherwise write `config.broken.json.bak` (or a numbered variant if
/// that name holds different content). Never overwrites an existing backup.
fn backup_unusable(path: &Path, text: &str) -> Result<PathBuf, String> {
    for attempt in 0..100u32 {
        let name = if attempt == 0 {
            "config.broken.json.bak".to_string()
        } else {
            format!("config.broken.{attempt}.json.bak")
        };
        let backup = path.with_file_name(name);
        match fs::read_to_string(&backup) {
            Ok(existing) if existing == text => return Ok(backup),
            Ok(_) => continue,
            Err(_) if backup.exists() => continue,
            Err(_) => {
                return fs::write(&backup, text).map(|_| backup.clone()).map_err(|error| {
                    format!(
                        "refusing to overwrite the unreadable config {}: could not back it up to {}: {error}",
                        path.display(),
                        backup.display()
                    )
                });
            }
        }
    }
    Err(format!(
        "refusing to overwrite the unreadable config {}: too many existing backups; remove old config.broken*.bak files",
        path.display()
    ))
}

fn write_config(path: &Path, config: &AppConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("could not create config directory: {e}"))?;
    }
    let text = serde_json::to_string_pretty(config).map_err(|e| format!("could not encode config: {e}"))?;
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, format!("{text}\n")).map_err(|e| format!("could not write {}: {e}", temp.display()))?;
    fs::rename(&temp, path).map_err(|e| {
        let _ = fs::remove_file(&temp);
        format!("could not write {}: {e}", path.display())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aztray-config-{tag}-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn instance(id: &str, name: &str, base: u16, dir: &str) -> InstanceConfig {
        let mut ports = BTreeMap::new();
        ports.insert(ServiceName::Blob, base);
        ports.insert(ServiceName::Queue, base + 1);
        ports.insert(ServiceName::Table, base + 2);
        InstanceConfig {
            id: id.into(),
            name: name.into(),
            host: "127.0.0.1".into(),
            ports,
            data_directory: dir.into(),
            loose: false,
            skip_api_version_check: false,
        }
    }

    fn config_with(instances: Vec<InstanceConfig>) -> AppConfig {
        AppConfig { instances, ..default_config() }
    }

    #[test]
    fn migrates_v1_config_to_default_instance_with_backup() {
        let dir = temp_dir("v1");
        let path = dir.join("config.json");
        let v1 = r#"{"host":"0.0.0.0","ports":{"blob":11000,"queue":11001,"table":11002},"dataDirectory":"C:\\az\\data","executablePath":"C:\\az\\azurite.cmd","nodePath":"C:\\node.exe"}"#;
        fs::write(&path, v1).unwrap();

        let loaded = load_from(&path, None);
        assert!(loaded.error.is_none(), "{:?}", loaded.error);
        let config = loaded.config;
        assert_eq!(config.schema_version, 2);
        assert_eq!(config.instances.len(), 1);
        let instance = &config.instances[0];
        assert_eq!(instance.id, "default");
        assert_eq!(instance.name, "Default");
        assert_eq!(instance.host, "0.0.0.0");
        assert_eq!(instance.ports[&ServiceName::Blob], 11000);
        assert_eq!(instance.data_directory, "C:\\az\\data");
        assert_eq!(config.executable_path.as_deref(), Some("C:\\az\\azurite.cmd"));
        assert_eq!(config.node_path.as_deref(), Some("C:\\node.exe"));
        assert_eq!(config.mcp, McpConfig::default());

        // backup holds the original text; the live file is now v2
        assert_eq!(fs::read_to_string(dir.join("config.v1.json.bak")).unwrap(), v1);
        let rewritten: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(rewritten["schemaVersion"], 2);
        assert!(rewritten["instances"].is_array());

        // second load reads v2 and never overwrites the backup
        fs::write(dir.join("config.v1.json.bak"), "sentinel").unwrap();
        let again = load_from(&path, None);
        assert!(again.error.is_none());
        assert_eq!(again.config.instances[0].ports[&ServiceName::Queue], 11001);
        assert_eq!(fs::read_to_string(dir.join("config.v1.json.bak")).unwrap(), "sentinel");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn v2_missing_optional_fields_use_defaults() {
        let dir = temp_dir("v2");
        let path = dir.join("config.json");
        fs::write(
            &path,
            r#"{"schemaVersion":2,"instances":[{"id":"dev","name":"Dev","host":"127.0.0.1","ports":{"blob":10000,"queue":10001,"table":10002},"dataDirectory":"C:\\d"}]}"#,
        )
        .unwrap();
        let loaded = load_from(&path, None);
        assert!(loaded.error.is_none(), "{:?}", loaded.error);
        assert_eq!(loaded.config.mcp, McpConfig::default());
        assert!(!loaded.config.instances[0].loose);
        assert!(!loaded.config.instances[0].skip_api_version_check);
        assert!(!dir.join("config.v1.json.bak").exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn malformed_file_yields_defaults_and_is_not_overwritten_until_saved() {
        let dir = temp_dir("bad");
        let path = dir.join("config.json");
        fs::write(&path, "{ not json").unwrap();
        let loaded = load_from(&path, None);
        assert!(loaded.error.as_deref().unwrap().contains("malformed"));
        assert_eq!(loaded.config.instances[0].id, "default");
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ not json");

        persist_to(&path, &loaded.config).unwrap();
        assert_eq!(fs::read_to_string(dir.join("config.broken.json.bak")).unwrap(), "{ not json");
        assert!(fs::read_to_string(&path).unwrap().contains("\"instances\""));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn unparseable_v2_is_backed_up_at_load_and_never_lost_on_persist() {
        let dir = temp_dir("badv2");
        let path = dir.join("config.json");
        let bad = r#"{"schemaVersion":2,"instances":[{"id":"dev","name":"Dev","ports":"oops"}]}"#;
        fs::write(&path, bad).unwrap();

        let loaded = load_from(&path, None);
        let error = loaded.error.as_deref().expect("load reports an error");
        assert!(error.contains("malformed") && error.contains("config.broken.json.bak"), "{error}");
        assert_eq!(loaded.config.instances[0].id, "default");
        assert_eq!(fs::read_to_string(&path).unwrap(), bad, "live file untouched at load");
        assert_eq!(fs::read_to_string(dir.join("config.broken.json.bak")).unwrap(), bad);

        // A second load reuses the identical backup rather than duplicating it.
        let _ = load_from(&path, None);
        assert!(!dir.join("config.broken.1.json.bak").exists());

        // Persisting keeps the backup intact.
        persist_to(&path, &loaded.config).unwrap();
        assert_eq!(fs::read_to_string(dir.join("config.broken.json.bak")).unwrap(), bad);
        assert!(fs::read_to_string(&path).unwrap().contains("\"default\""));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn persist_refuses_when_unparseable_v2_cannot_be_backed_up() {
        let dir = temp_dir("nobackup");
        let path = dir.join("config.json");
        let bad = r#"{"instances":[{"id":"dev"}]}"#;
        fs::write(&path, bad).unwrap();
        // Occupy every backup name with a directory so no backup can be written.
        fs::create_dir(dir.join("config.broken.json.bak")).unwrap();
        for attempt in 1..100 {
            fs::create_dir(dir.join(format!("config.broken.{attempt}.json.bak"))).unwrap();
        }
        let err = persist_to(&path, &default_config()).unwrap_err();
        assert!(err.contains("refusing to overwrite"), "{err}");
        assert_eq!(fs::read_to_string(&path).unwrap(), bad);

        // A differing pre-existing backup is preserved and a numbered one is added.
        let dir2 = temp_dir("numbered");
        let path2 = dir2.join("config.json");
        fs::write(&path2, bad).unwrap();
        fs::write(dir2.join("config.broken.json.bak"), "older").unwrap();
        persist_to(&path2, &default_config()).unwrap();
        assert_eq!(fs::read_to_string(dir2.join("config.broken.json.bak")).unwrap(), "older");
        assert_eq!(fs::read_to_string(dir2.join("config.broken.1.json.bak")).unwrap(), bad);
        let _ = fs::remove_dir_all(dir);
        let _ = fs::remove_dir_all(dir2);
    }

    #[test]
    fn absent_everywhere_gives_default_config_without_writing() {
        let dir = temp_dir("none");
        let path = dir.join("config.json");
        let loaded = load_from(&path, None);
        assert!(loaded.error.is_none());
        assert_eq!(loaded.config.instances.len(), 1);
        assert_eq!(loaded.config.instances[0].ports, default_ports());
        assert!(!path.exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn azctl_import_is_persisted_as_v2_and_source_untouched() {
        let dir = temp_dir("azctl");
        let own = dir.join("AzTray").join("config.json");
        let azctl = dir.join("azctl.json");
        let original = r#"{"host":"127.0.0.1","blobPort":12000,"queuePort":12001,"tablePort":12002,"dataDir":"C:\\old"}"#;
        fs::write(&azctl, original).unwrap();
        let loaded = load_from(&own, Some(&azctl));
        assert!(loaded.error.is_none(), "{:?}", loaded.error);
        assert_eq!(loaded.config.instances[0].ports[&ServiceName::Blob], 12000);
        assert_eq!(loaded.config.instances[0].data_directory, "C:\\old");
        assert_eq!(fs::read_to_string(&azctl).unwrap(), original);
        assert!(own.is_file());
        assert!(!own.with_file_name("config.v1.json.bak").exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn slug_and_unique_id_rules() {
        assert_eq!(slugify("Integration Tests!"), "integration-tests");
        assert_eq!(slugify("  --A__B--  "), "a-b");
        assert_eq!(slugify("***"), "instance");
        assert!(slugify(&"x".repeat(80)).len() <= 32);
        let taken = vec!["dev".to_string(), "dev-2".to_string()];
        assert_eq!(unique_id("dev", &taken), "dev-3");
        assert_eq!(unique_id("qa", &taken), "qa");
        assert!(is_valid_id("a-1"));
        assert!(!is_valid_id("-a"));
        assert!(!is_valid_id("A"));
        assert!(!is_valid_id(""));
    }

    #[test]
    fn validate_rejects_port_name_and_directory_conflicts() {
        let a = instance("default", "Default", 10000, "C:\\data\\a");
        let b = instance("dev", "Dev", 10003, "C:\\data\\b");
        assert!(validate(&config_with(vec![a.clone(), b.clone()])).is_ok());

        let clash = instance("qa", "QA", 10003, "C:\\data\\c");
        let err = validate(&config_with(vec![a.clone(), b.clone(), clash.clone()])).unwrap_err();
        assert!(err.contains("blob port 10003 is already assigned to instance \"Dev\""), "{err}");
        let err = check_instance_conflicts(&config_with(vec![a.clone(), b.clone()]), &clash, None).unwrap_err();
        assert!(err.contains("already assigned to instance \"Dev\""), "{err}");

        let dup_name = instance("qa", "dev", 10006, "C:\\data\\c");
        assert!(validate(&config_with(vec![a.clone(), b.clone(), dup_name])).unwrap_err().contains("name"));

        let nested = instance("qa", "QA", 10006, "C:\\DATA\\b\\inner\\");
        assert!(validate(&config_with(vec![a.clone(), b.clone(), nested])).unwrap_err().contains("overlaps"));
        let same = instance("qa", "QA", 10006, "c:/data/B/");
        assert!(validate(&config_with(vec![a.clone(), b.clone(), same])).unwrap_err().contains("overlaps"));

        let in_mcp = instance("qa", "QA", 47550, "C:\\data\\c");
        assert!(validate(&config_with(vec![a.clone(), in_mcp])).unwrap_err().contains("MCP"));

        let mut bad_version = config_with(vec![a.clone()]);
        bad_version.schema_version = 1;
        assert!(validate(&bad_version).is_err());
        assert!(validate(&config_with(vec![])).is_err());

        // updating an instance must ignore its own entry
        assert!(check_instance_conflicts(&config_with(vec![a, b.clone()]), &b, Some("dev")).is_ok());
    }
}

use crate::types::{Config, ServiceName};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_HOST: &str = "127.0.0.1";
pub const DEFAULT_BLOB_PORT: u16 = 10_000;
pub const DEFAULT_QUEUE_PORT: u16 = 10_001;
pub const DEFAULT_TABLE_PORT: u16 = 10_002;

#[derive(Clone, Debug)]
pub struct LoadedConfig {
    pub config: Config,
    pub error: Option<String>,
    pub source: Option<PathBuf>,
}

pub fn default_config() -> Config {
    let mut ports = BTreeMap::new();
    ports.insert(ServiceName::Blob, DEFAULT_BLOB_PORT);
    ports.insert(ServiceName::Queue, DEFAULT_QUEUE_PORT);
    ports.insert(ServiceName::Table, DEFAULT_TABLE_PORT);

    Config {
        host: DEFAULT_HOST.to_string(),
        ports,
        data_directory: default_data_directory().to_string_lossy().into_owned(),
        executable_path: None,
        node_path: None,
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

pub fn azctl_config_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|path| PathBuf::from(path).join("azctl").join("config.json"))
}

/// Load the AzTray config, importing the old azctl config on first launch.
/// The azctl file is only read; it is never written or renamed.
pub fn load() -> LoadedConfig {
    let own = config_path();
    if own.is_file() {
        return load_file(&own);
    }
    if let Some(azctl) = azctl_config_path() {
        if azctl.is_file() {
            let mut loaded = load_file(&azctl);
            loaded.source = Some(azctl);
            return loaded;
        }
    }
    LoadedConfig {
        config: default_config(),
        error: None,
        source: None,
    }
}

pub fn persist(config: &Config) -> Result<(), String> {
    validate(config)?;
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("could not create config directory: {e}"))?;
    }
    let text = serde_json::to_string_pretty(config).map_err(|e| format!("could not encode config: {e}"))?;
    fs::write(&path, format!("{text}\n")).map_err(|e| format!("could not write {}: {e}", path.display()))
}

pub fn validate(config: &Config) -> Result<(), String> {
    if config.host.trim().is_empty() {
        return Err("host cannot be empty".into());
    }
    if config.ports.len() != 3 {
        return Err("config must include blob, queue, and table ports".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    for service in [ServiceName::Blob, ServiceName::Queue, ServiceName::Table] {
        let port = config
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
    if config.data_directory.trim().is_empty() {
        return Err("data directory cannot be empty".into());
    }
    Ok(())
}

fn load_file(path: &Path) -> LoadedConfig {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => {
            return LoadedConfig {
                config: default_config(),
                error: Some(format!("could not read {}: {error}", path.display())),
                source: Some(path.to_path_buf()),
            }
        }
    };
    match serde_json::from_str::<Value>(&text).and_then(|value| parse_value(&value)) {
        Ok(config) => match validate(&config) {
            Ok(()) => LoadedConfig {
                config,
                error: None,
                source: Some(path.to_path_buf()),
            },
            Err(error) => LoadedConfig {
                config,
                error: Some(error),
                source: Some(path.to_path_buf()),
            },
        },
        Err(error) => LoadedConfig {
            config: default_config(),
            error: Some(format!("malformed config {}: {error}", path.display())),
            source: Some(path.to_path_buf()),
        },
    }
}

fn parse_value(value: &Value) -> Result<Config, serde_json::Error> {
    let defaults = default_config();
    let object = value.as_object().ok_or_else(|| serde_json::Error::io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "config must be a JSON object",
    )))?;

    let host = string_field(object, &["host", "bindHost", "bind_host"])
        .unwrap_or_else(|| defaults.host.clone());
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

    Ok(Config {
        host,
        ports,
        data_directory,
        executable_path,
        node_path,
    })
}

fn parse_port(value: &Value, name: &str) -> Result<u16, serde_json::Error> {
    if let Some(port) = value.as_u64() {
        return u16::try_from(port).map_err(|_| serde_json::Error::io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("{name} is outside the valid port range"),
        )));
    }
    if let Some(port) = value.as_str() {
        return port.parse::<u16>().map_err(|_| serde_json::Error::io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("{name} must be a port number"),
        )));
    }
    Err(serde_json::Error::io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("{name} must be a port number"),
    )))
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

fn service_label(service: &ServiceName) -> &'static str {
    match service {
        ServiceName::Blob => "blob",
        ServiceName::Queue => "queue",
        ServiceName::Table => "table",
    }
}

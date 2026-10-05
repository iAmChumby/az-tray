use crate::types::{PortOwner, ProcessIdentity};
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr, TcpListener};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// One TCP listener from the OS table.
#[derive(Clone, Debug)]
pub struct Listener {
    pub host: String,
    pub port: u16,
    pub pid: u32,
}

/// True when nothing is bound to `host:port` right now. Binds and drops a
/// socket; spawns no process. Unparseable hosts probe 127.0.0.1.
pub fn is_port_free(host: &str, port: u16) -> bool {
    let ip: IpAddr = host
        .trim()
        .trim_matches(['[', ']'])
        .parse()
        .unwrap_or_else(|_| IpAddr::from([127, 0, 0, 1]));
    TcpListener::bind(SocketAddr::new(ip, port)).is_ok()
}

/// First `[blob, queue, table]` trio, scanning blob ports from `start` to `end`
/// in steps of 3, where no port is in `taken` and every port is free on `host`.
pub fn find_free_port_trio(host: &str, taken: &HashSet<u16>, start: u16, end: u16) -> Option<[u16; 3]> {
    let mut blob = u32::from(start);
    while blob <= u32::from(end) && blob + 2 <= 65_535 {
        let trio = [blob as u16, blob as u16 + 1, blob as u16 + 2];
        if trio.iter().all(|port| !taken.contains(port) && is_port_free(host, *port)) {
            return Some(trio);
        }
        blob += 3;
    }
    None
}

#[derive(Clone, Debug)]
pub struct PortProbe {
    pub owner: Option<PortOwner>,
    pub error: Option<String>,
}

pub fn probe(host: &str, port: u16, owned_pids: &HashSet<u32>) -> PortProbe {
    match listeners() {
        Ok(table) => probe_in(&table, host, port, owned_pids),
        Err(error) => PortProbe { owner: None, error: Some(error) },
    }
}

/// Like `probe`, but against a listener table captured once (so a refresh of
/// many instances runs one `netstat`, not one per port).
pub fn probe_in(table: &[Listener], host: &str, port: u16, owned_pids: &HashSet<u32>) -> PortProbe {
    probe_with(table, host, port, owned_pids, process_identity)
}

/// How long a foreign pid's identity is reused by `probe_in_cached`.
const IDENTITY_TTL: Duration = Duration::from_secs(10);

type IdentityCache = Mutex<HashMap<u32, (Instant, Option<ProcessIdentity>)>>;

fn identity_cache() -> &'static IdentityCache {
    static CACHE: OnceLock<IdentityCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Cheap variant for periodic refreshes. Identities of app-owned pids come
/// from `known` (the engine's records) and foreign pids are cached briefly, so
/// a refresh spawns PowerShell only for a pid it has not seen recently.
pub fn probe_in_cached(
    table: &[Listener],
    host: &str,
    port: u16,
    owned_pids: &HashSet<u32>,
    known: &HashMap<u32, ProcessIdentity>,
) -> PortProbe {
    probe_with(table, host, port, owned_pids, |pid| {
        if let Some(identity) = known.get(&pid) {
            return Some(identity.clone());
        }
        let now = Instant::now();
        {
            let mut cache = identity_cache().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            cache.retain(|_, (at, _)| now.duration_since(*at) < IDENTITY_TTL);
            if let Some((_, identity)) = cache.get(&pid) {
                return identity.clone();
            }
        }
        let identity = process_identity(pid);
        identity_cache()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(pid, (Instant::now(), identity.clone()));
        identity
    })
}

fn probe_with(
    table: &[Listener],
    host: &str,
    port: u16,
    owned_pids: &HashSet<u32>,
    identify: impl FnOnce(u32) -> Option<ProcessIdentity>,
) -> PortProbe {
    let pid = table
        .iter()
        .find(|listener| listener.port == port && host_matches(&listener.host, host))
        .map(|listener| listener.pid);
    let Some(pid) = pid else {
        return PortProbe { owner: None, error: None };
    };
    let identity = identify(pid).unwrap_or(ProcessIdentity {
        pid,
        name: None,
        executable_path: None,
        command_line: None,
        started_at: None,
    });
    let can_terminate = owned_pids.contains(&pid) || is_probably_user_process(&identity);
    PortProbe {
        owner: Some(PortOwner {
            process: identity,
            owned_by_app: owned_pids.contains(&pid),
            can_terminate,
        }),
        error: None,
    }
}

pub fn identity_matches(actual: &PortOwner, expected_pid: u32, expected_started_at: Option<&str>) -> bool {
    if actual.process.pid != expected_pid {
        return false;
    }
    match (expected_started_at, actual.process.started_at.as_deref()) {
        (Some(expected), Some(actual)) if !expected.is_empty() && !actual.is_empty() => expected == actual,
        // A PID alone is reusable. Refuse to terminate when either side lacks
        // the creation identity needed to distinguish a reused PID.
        _ => false,
    }
}

pub fn terminate(pid: u32, tree: bool) -> Result<(), String> {
    if pid == 0 || pid == 4 {
        return Err("refusing to terminate a system PID".into());
    }
    #[cfg(windows)]
    {
        let mut command = Command::new("taskkill");
        command.arg("/PID").arg(pid.to_string());
        if tree {
            command.arg("/T");
        }
        command.arg("/F");
        hide_console(&mut command);
        let output = command.output().map_err(|error| format!("could not invoke taskkill: {error}"))?;
        if output.status.success() {
            return Ok(());
        }
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if detail.is_empty() {
            format!("taskkill failed for PID {pid}")
        } else {
            detail
        })
    }
    #[cfg(not(windows))]
    {
        let signal = if tree { "-TERM" } else { "-TERM" };
        let output = Command::new("kill")
            .arg(signal)
            .arg(pid.to_string())
            .output()
            .map_err(|error| format!("could not invoke kill: {error}"))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
        }
    }
}

pub fn process_alive(pid: u32) -> bool {
    process_identity(pid).is_some()
}

/// Confirm that a listener process is the launched process or a descendant of
/// it. This closes the startup race where an unrelated process binds the port
/// between our preflight probe and Azurite's listen event.
pub fn is_descendant_or_self(pid: u32, root_pid: u32) -> bool {
    if pid == root_pid {
        return true;
    }
    #[cfg(windows)]
    {
        let script = format!(
            "$root={root_pid}; $p=Get-CimInstance Win32_Process -Filter \"ProcessId = {pid}\"; while ($p) {{ if ([int]$p.ParentProcessId -eq $root) {{ exit 0 }}; if ([int]$p.ParentProcessId -le 0 -or [int]$p.ParentProcessId -eq [int]$p.ProcessId) {{ break }}; $p=Get-CimInstance Win32_Process -Filter (\"ProcessId = \" + [int]$p.ParentProcessId) }}; exit 1"
        );
        let executable = if which("powershell.exe") { "powershell.exe" } else { "powershell" };
        let mut command = Command::new(executable);
        command.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &script]);
        hide_console(&mut command);
        command.status().map(|status| status.success()).unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        let _ = root_pid;
        false
    }
}

/// Snapshot of every listening TCP socket (Windows: `netstat -ano`; other
/// platforms report none).
pub fn listeners() -> Result<Vec<Listener>, String> {
    #[cfg(windows)]
    {
        let mut command = Command::new("netstat");
        command.args(["-ano", "-p", "tcp"]);
        hide_console(&mut command);
        let output = command.output()
            .map_err(|error| format!("could not inspect TCP listeners: {error}"))?;
        if !output.status.success() {
            return Err("netstat could not inspect TCP listeners".into());
        }
        Ok(parse_netstat(&String::from_utf8_lossy(&output.stdout)))
    }
    #[cfg(not(windows))]
    {
        Ok(Vec::new())
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
fn parse_netstat(text: &str) -> Vec<Listener> {
    let mut table = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 5 || !fields[0].eq_ignore_ascii_case("TCP") {
            continue;
        }
        if !fields[3].eq_ignore_ascii_case("LISTENING") {
            continue;
        }
        let Some((local_host, local_port)) = fields[1].rsplit_once(':') else {
            continue;
        };
        let (Ok(port), Ok(pid)) = (local_port.parse::<u16>(), fields[4].parse::<u32>()) else {
            continue;
        };
        table.push(Listener { host: local_host.to_string(), port, pid });
    }
    table
}

fn host_matches(local: &str, requested: &str) -> bool {
    let local = local.trim_matches(['[', ']']);
    let requested = requested.trim_matches(['[', ']']);
    local == "0.0.0.0" || local == "::" || local == "0:0:0:0:0:0:0:0" || local.eq_ignore_ascii_case(requested)
}

fn process_identity(pid: u32) -> Option<ProcessIdentity> {
    #[cfg(windows)]
    {
        let script = format!(
            "$p=Get-CimInstance Win32_Process -Filter \"ProcessId = {pid}\"; if ($p) {{ [pscustomobject]@{{pid=$p.ProcessId;name=$p.Name;executable_path=$p.ExecutablePath;command_line=$p.CommandLine;started_at=$(if($p.CreationDate){{$p.CreationDate.ToString('o')}}else{{$null}})}} | ConvertTo-Json -Compress }}"
        );
        let executable = if which("powershell.exe") { "powershell.exe" } else { "powershell" };
        let mut command = Command::new(executable);
        command
            .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &script])
            ;
        hide_console(&mut command);
        let output = command.output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
        Some(ProcessIdentity {
            pid: value.get("pid").and_then(serde_json::Value::as_u64).unwrap_or(pid as u64) as u32,
            name: value.get("name").and_then(serde_json::Value::as_str).map(str::to_string),
            executable_path: value.get("executable_path").and_then(serde_json::Value::as_str).map(str::to_string),
            command_line: value.get("command_line").and_then(serde_json::Value::as_str).map(str::to_string),
            started_at: value.get("started_at").and_then(serde_json::Value::as_str).map(str::to_string),
        })
    }
    #[cfg(not(windows))]
    {
        let output = Command::new("ps").args(["-p", &pid.to_string(), "-o", "comm=,args="]).output().ok()?;
        if !output.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if text.is_empty() {
            return None;
        }
        let mut fields = text.splitn(2, char::is_whitespace);
        let name = fields.next().map(str::to_string);
        let command_line = fields.next().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
        Some(ProcessIdentity { pid, name, executable_path: None, command_line, started_at: None })
    }
}

fn is_probably_user_process(identity: &ProcessIdentity) -> bool {
    identity
        .name
        .as_deref()
        .map(|name| !name.eq_ignore_ascii_case("system") && !name.eq_ignore_ascii_case("system idle process"))
        .unwrap_or(false)
}

fn hide_console(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
}

#[cfg(windows)]
fn which(executable: &str) -> bool {
    let mut command = Command::new("where");
    command.arg(executable);
    hide_console(&mut command);
    command.output().map(|output| output.status.success()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_trio_skips_taken_and_busy_ports() {
        let busy = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral");
        let busy_port = busy.local_addr().unwrap().port();
        assert!(!is_port_free("127.0.0.1", busy_port));

        // The trio starting at the busy port is skipped.
        let trio = find_free_port_trio("127.0.0.1", &HashSet::new(), busy_port, busy_port.saturating_add(60))
            .expect("a free trio exists");
        assert!(!trio.contains(&busy_port));
        assert_eq!((trio[0] - busy_port) % 3, 0);
        assert_eq!(trio[1], trio[0] + 1);
        assert_eq!(trio[2], trio[0] + 2);

        // Taken ports are skipped even when the OS says they are free.
        let mut taken = HashSet::new();
        taken.insert(trio[1]);
        let next = find_free_port_trio("127.0.0.1", &taken, trio[0], trio[0] + 60).expect("next trio");
        assert!(next[0] >= trio[0] + 3);
        drop(busy);
    }

    #[test]
    fn exhausted_range_returns_none() {
        let taken: HashSet<u16> = (20_000..=20_002).collect();
        assert_eq!(find_free_port_trio("127.0.0.1", &taken, 20_000, 20_000), None);
    }

    #[test]
    fn cached_probe_reuses_known_identity_without_spawning() {
        let table = vec![Listener { host: "0.0.0.0".into(), port: 10_000, pid: 4_242_424 }];
        let identity = ProcessIdentity {
            pid: 4_242_424,
            name: Some("node.exe".into()),
            executable_path: None,
            command_line: Some("azurite".into()),
            started_at: Some("t0".into()),
        };
        let known: HashMap<u32, ProcessIdentity> = [(4_242_424, identity)].into_iter().collect();
        let owned: HashSet<u32> = [4_242_424].into_iter().collect();
        let owner = probe_in_cached(&table, "127.0.0.1", 10_000, &owned, &known).owner.expect("owner");
        assert_eq!(owner.process.name.as_deref(), Some("node.exe"));
        assert_eq!(owner.process.started_at.as_deref(), Some("t0"));
        assert!(owner.owned_by_app && owner.can_terminate);
    }

    #[test]
    fn netstat_parsing_collects_listening_rows() {
        let text = "  TCP    0.0.0.0:10000    0.0.0.0:0    LISTENING    4242
  TCP    127.0.0.1:10001    1.2.3.4:5    ESTABLISHED    9
  TCP    [::]:10002    [::]:0    LISTENING    77
";
        let table = parse_netstat(text);
        assert_eq!(table.len(), 2);
        assert!(probe_in(&table, "127.0.0.1", 10000, &HashSet::new()).owner.is_some());
        assert!(probe_in(&table, "127.0.0.1", 10001, &HashSet::new()).owner.is_none());
    }
}

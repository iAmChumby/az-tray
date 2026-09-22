use crate::types::{PortOwner, ProcessIdentity};
use std::collections::HashSet;
use std::process::Command;

#[derive(Clone, Debug)]
pub struct PortProbe {
    pub owner: Option<PortOwner>,
    pub error: Option<String>,
}

pub fn probe(host: &str, port: u16, owned_pids: &HashSet<u32>) -> PortProbe {
    let pid = match listening_pid(host, port) {
        Ok(pid) => pid,
        Err(error) => return PortProbe { owner: None, error: Some(error) },
    };
    let Some(pid) = pid else {
        return PortProbe { owner: None, error: None };
    };
    let process = process_identity(pid);
    let identity = process.unwrap_or_else(|| ProcessIdentity {
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
        return Err(if detail.is_empty() {
            format!("taskkill failed for PID {pid}")
        } else {
            detail
        });
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
        return command.status().map(|status| status.success()).unwrap_or(false);
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        let _ = root_pid;
        false
    }
}

fn listening_pid(host: &str, port: u16) -> Result<Option<u32>, String> {
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
        let text = String::from_utf8_lossy(&output.stdout);
        return Ok(parse_netstat(&text, host, port));
    }
    #[cfg(not(windows))]
    {
        let _ = host;
        let _ = port;
        Ok(None)
    }
}

#[cfg(windows)]
fn parse_netstat(text: &str, host: &str, port: u16) -> Option<u32> {
    let wanted_port = port.to_string();
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 5 || !fields[0].eq_ignore_ascii_case("TCP") {
            continue;
        }
        if !fields[3].eq_ignore_ascii_case("LISTENING") {
            continue;
        }
        let local = fields[1];
        let Some((local_host, local_port)) = local.rsplit_once(':') else {
            continue;
        };
        if local_port != wanted_port || !host_matches(local_host, host) {
            continue;
        }
        if let Ok(pid) = fields[4].parse::<u32>() {
            return Some(pid);
        }
    }
    None
}

#[cfg(windows)]
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
        return Some(ProcessIdentity {
            pid: value.get("pid").and_then(serde_json::Value::as_u64).unwrap_or(pid as u64) as u32,
            name: value.get("name").and_then(serde_json::Value::as_str).map(str::to_string),
            executable_path: value.get("executable_path").and_then(serde_json::Value::as_str).map(str::to_string),
            command_line: value.get("command_line").and_then(serde_json::Value::as_str).map(str::to_string),
            started_at: value.get("started_at").and_then(serde_json::Value::as_str).map(str::to_string),
        });
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

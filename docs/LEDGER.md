# AzTray delivery ledger

## v0.1.0 — initial installed release

- Built the Next.js static export and Tauri current-user NSIS bundle.
- Published source and a public release with a SHA-256 checksum.
- Accepted the installed app in native Windows UI: all three services started,
  live logs appeared, restart and stop worked, a port conflict was handled,
  and Quit terminated app-owned services.

## Work-PC readiness iteration

- Fixed a nonexistent executable-path override that appeared Ready until
  Start was clicked. Installed v0.1.1 showed Missing Azurite, actionable
  guidance, and disabled Start; restoring the real path returned to Ready.
- Clarified separate user-space Node/Azurite installation in the public docs.
- Installed the local v0.1.1 NSIS bundle through `scripts/install.ps1` with a
  verified SHA-256 under `AdminToken=False`, without a UAC prompt.
- In the installed native UI, Start all reached 3/3 Running and merged logs
  showed Blob, Queue, and Table listeners. Restarting Blob changed its listener
  PID while Queue and Table stayed running. An external temporary Azurite Blob
  listener was identified by name/PID in the Free port confirmation and
  released. Stop & quit exited the tray and cleared all three default ports.
- Final source adds the missing-engine banner to dashboard Services as well as
  Settings and the popover. The v0.1.1 Windows release workflow passed and
  published the installer plus SHA-256 checksum.
- Ran the README's public install command from an `AdminToken=False` shell.
  It downloaded the published v0.1.1 installer, verified its SHA-256, installed
  in `%LOCALAPPDATA%` without a UAC prompt, launched AzTray, and registered
  current-user sign-in startup.
- Used that GitHub-downloaded installed app in native Windows UI. Start all
  reached 3/3 Running; the dashboard showed live Blob and merged service logs;
  restarting Blob changed its PID while Queue and Table remained running.
  Stop & quit exited the process and cleared ports 10000–10002. Relaunching
  left the tray app running idle with sign-in startup intact.

## v0.3.0 — multi-instance, provisioning, robust MCP (issue #1)

- Root cause of the unreachable MCP endpoint: no released build contained the
  MCP server. Tags v0.1.1 to v0.1.5 predate the MCP commit (6f61a65), and
  `releases/latest` served v0.1.5. Bind failures were also invisible (stderr in a
  windowed exe, no log file, no port fallback, no retry).
- Added multiple concurrent Azurite instances with automatic port-trio
  allocation, per-instance data directories, config schema 2 with automatic
  v1 migration and backup, and per-instance connection strings.
- MCP now enabled by default with port fallback, a self-verifying supervisor
  with retry, status in the snapshot and UI (popover footer, Local MCP card with
  Retry), instance-aware tools including `aztray_create_instance`, and
  `aztray_get_app_log`.
- Added `%APPDATA%\AzTray\logs\aztray.log` (1 MB rotation), an About panel
  with version and feature list, and a Diagnostics log tail.
- Release workflow now runs `cargo test` and asserts the built exe contains
  `aztray_mcp_status`; version bumped to 0.3.0 across `package.json`,
  `Cargo.toml`, `Cargo.lock`, and `tauri.conf.json`.
- Redesigned dashboard and popover (instance rail, connection panel, MCP and
  diagnostics settings); screenshots live in `docs/screenshots/`.
- Pending after tagging `v0.3.0`: install the published release through
  `scripts/install.ps1` and confirm `aztray_mcp_status` reports `running:true`.

# Install and connect AzTray MCP as an agent

AzTray includes its MCP server in the Windows tray app. There is no separate MCP server package or command to keep running. While AzTray is running, it serves a **Streamable HTTP** MCP endpoint on loopback, by default `http://127.0.0.1:47551/mcp`. Register that URL in a client on the same Windows machine.

With the MCP tools an agent can also provision isolated Azurite instances on demand (see [Provisioning an Azurite instance for a test run](#provisioning-an-azurite-instance-for-a-test-run)).

## 1. Install the MCP-capable app

**Install AzTray v0.3.0 or newer.** Earlier releases (v0.1.x and v0.2.x) contain no MCP server at all, so every connection to port 47551 fails with `ECONNREFUSED`. Download the latest installer from the GitHub releases page, or run the install script, which defaults to the latest release:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install.ps1
```

After installing, open **AzTray Dashboard > Settings > About** and confirm the version is 0.3.0 or newer and that the feature list contains `mcp`.

To build from source instead, clone the repository in Windows PowerShell and set up the [official Tauri Windows prerequisites](https://v2.tauri.app/start/prerequisites/) (Node/npm, Rust with the MSVC toolchain, Microsoft C++ Build Tools, WebView2):

```powershell
git clone https://github.com/iAmChumby/az-tray.git
Set-Location .\az-tray
$version = (Get-Content .\package.json -Raw | ConvertFrom-Json).version
npm ci
npm run tauri build
$installer = Join-Path (Get-Location).Path "src-tauri\target\release\bundle\nsis\AzTray_${version}_x64-setup.exe"
$sha256 = (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install.ps1 -InstallerPath $installer -Sha256 $sha256
```

If an existing AzTray process is running, the install script waits for it to exit. Use **Quit AzTray > Stop & quit** in that app, then continue the installer. The app installation is separate from Node.js and Azurite; follow [the runtime setup in the README](../README.md#2-install-nodejs-and-azurite-in-your-user-profile) when the Azurite engine reports missing dependencies.

## 2. Confirm the local endpoint and find the actual URL

AzTray starts the MCP endpoint at launch and verifies it: after binding the socket it connects to itself and completes an MCP `initialize` handshake before reporting **running**. Open **AzTray Dashboard > Settings > Local MCP** (or look at the MCP dot in the tray popover footer). The card shows:

- the **actual URL** to register (copy it with the Copy button),
- the state: green running, amber retrying, red failed,
- the last event, the error if any, and the number of bind attempts.

### Port and fallback behavior

- The preferred port is `47551` (setting `mcp.port`). The listener binds `127.0.0.1` only, plus `[::1]` when available so clients that resolve `localhost` to IPv6 also connect.
- If the preferred port is busy and **port fallback** is on (the default), AzTray tries the next ports up to `+9` (`47551` to `47560`). The card then shows `fallbackUsed` and the real URL, for example `http://127.0.0.1:47552/mcp`. Register **that** URL.
- With fallback off, AzTray stays on the configured port and reports the bind error.
- If binding or the self-check fails, AzTray retries automatically (every 15 s, backing off to 60 s). The **Retry** button on the Local MCP card restarts the endpoint immediately. A working endpoint is never moved after it is running.
- The port, enable switch, and fallback switch live in **Settings > Local MCP** in the dashboard. MCP clients cannot change them, so an agent cannot cut off its own connection.

An agent can ask for the same information with the `aztray_mcp_status` tool, which returns `{enabled, running, url, port, requestedPort, fallbackUsed, error, startedAt, lastEvent, attempts}`.

The URL is loopback-only, so a client on another computer or inside an isolated container cannot reach it through that address. Requests with a non-local `Origin` or `Host` header are rejected (DNS-rebinding protection).

## 3. Register the server in your MCP client

For Claude Code, add this to `.mcp.json` (project) or `~/.claude.json`, or run `claude mcp add --transport http aztray http://127.0.0.1:47551/mcp`:

```json
{
  "mcpServers": {
    "aztray": {
      "type": "http",
      "url": "http://127.0.0.1:47551/mcp"
    }
  }
}
```

For Codex, run:

```powershell
codex mcp add aztray --url http://127.0.0.1:47551/mcp
codex mcp get aztray
```

If `aztray` is already registered, run `codex mcp get aztray` first. Keep it when the URL matches. If it points elsewhere, run `codex mcp remove aztray` and then the `add` command above.

For another client, add a server named `aztray`, choose **Streamable HTTP**, and use the URL shown in the Local MCP card. Replace `47551` with the actual port if fallback was used.

Open a new agent session if the client does not refresh its tool catalog immediately. Ask the agent to call `aztray_mcp_status` and `aztray_list_instances`. A working connection returns `running: true`, the same URL, and the configured instances.

Protocol support: MCP `2026-07-28`, `2025-11-25`, `2025-06-18`, and `2025-03-26`, negotiated during `initialize`. Requests are `POST /mcp` with a JSON body; responses are `application/json`. Notifications get `202`. `GET /mcp` returns `405` because AzTray offers no standalone event stream. `initialize` returns an `Mcp-Session-Id` header; AzTray holds no per-session state, so the header is advisory.

## Tools

Every tool that acts on an instance takes an optional `instance` argument: an instance id or name (case-insensitive). When omitted, the `default` instance is used, or the only instance if there is exactly one; otherwise the call fails and lists the available instances. `serviceName` is `blob`, `queue`, or `table`.

| Tool | Purpose |
|---|---|
| `aztray_snapshot` | Full state: app version, config, engine, MCP status, all instances with service states and connection info. Log lines are omitted. |
| `aztray_mcp_status` | MCP endpoint status and the actual URL. |
| `aztray_list_instances` | All instances with state, ports, data directory, connection strings. |
| `aztray_get_instance` | One instance in full. |
| `aztray_create_instance` | Create an isolated instance (`name` required; `id`, `host`, `ports`, `dataDirectory`, `loose`, `skipApiVersionCheck`, `start` optional). Ports and data directory are auto-assigned. `start` defaults to true over MCP. |
| `aztray_update_instance` | Change name (any time) or host, ports, data directory, flags (instance must be stopped). |
| `aztray_delete_instance` | Remove a stopped instance (`confirmed: true`). Azurite data on disk is never deleted. |
| `aztray_start_instance`, `aztray_stop_instance`, `aztray_restart_instance` | Lifecycle for all three services of an instance. |
| `aztray_start_service`, `aztray_stop_service`, `aztray_restart_service` | Lifecycle for one service of an instance. |
| `aztray_start_all`, `aztray_stop_all`, `aztray_restart_all` | Lifecycle for every instance. |
| `aztray_connection_string` | A service's connection string, or the combined string, per-service strings, and endpoints for an instance. |
| `aztray_get_config` | Global settings, MCP settings, and all instance configs. |
| `aztray_set_settings` | Set the Node.js / Azurite executable overrides (`executablePath`, `nodePath`; `null` clears). MCP port and enabled are dashboard-only. Replaces `aztray_set_config`. |
| `aztray_check_engine` | Check that Node.js and Azurite are available. |
| `aztray_identify_port_owner` | Process identity holding a service port. |
| `aztray_free_port` | Terminate a port owner (see boundary below). |
| `aztray_get_logs`, `aztray_save_logs`, `aztray_clear_logs` | Azurite logs per instance and service. `aztray_get_logs` is the only tool that returns log lines (newest 200 by default, `limit` max 1000). All tool results are compact JSON, omit log arrays, and are capped at 32 KB with a `truncated` note. |
| `aztray_get_app_log` | Tail of AzTray's own `aztray.log` (`limit` 1 to 2000, default 200); returns `{path, lines}`. |
| `aztray_quit` | `stop_and_quit`, `leave_running`, or `cancel`. |

`aztray_set_config` was removed in v0.3.0. Use `aztray_set_settings` for global paths and the instance tools for host, ports, and data directories.

## Provisioning an Azurite instance for a test run

1. `aztray_create_instance {"name": "integration-tests"}` creates and starts a dedicated instance on free ports with its own data directory.
2. Read `connection.connectionString` from the response (or call `aztray_connection_string {"instance": "integration-tests"}`) and use it in the tests.
3. When finished: `aztray_stop_instance {"instance": "integration-tests"}`, then `aztray_delete_instance {"instance": "integration-tests", "confirmed": true}`. The data directory remains on disk.

Instances run concurrently, so the `default` instance (ports 10000 to 10002) keeps working.

## Operating boundary

The MCP tools manage AzTray's Azurite instances, configuration, port ownership, logs, connection strings, and app lifecycle. They do not browse or edit stored Blob, Queue, or Table records. For `aztray_free_port`, inspect `aztray_identify_port_owner` first and get the user's approval before passing the observed PID, process start time, and `confirmed: true`. AzTray rechecks that identity before releasing the port.

## Troubleshooting

**`ECONNREFUSED` on `127.0.0.1:47551`**

1. **Check the version.** Open **Dashboard > Settings > About**. AzTray older than v0.3.0 has no MCP server, and nothing will ever listen on 47551. Install the latest release. If the feature list lacks `mcp`, the binary is stale.
2. **Check that AzTray is running** (tray icon present). The endpoint exists only while the app runs.
3. **Check the Local MCP status** in Settings or the popover footer. If it is amber or red, read the error and last event; press **Retry**. If it is green, compare its URL with the one your client uses: when fallback was used, the port differs from 47551.
4. **Read the log.** `%APPDATA%\AzTray\logs\aztray.log` records every MCP event (bind attempt, bind failure with the OS error, fallback, running, retry). It rotates at 1 MB to `aztray.log.1`. In the dashboard, Settings > Diagnostics shows the tail, and `aztray_get_app_log` returns it to an agent.
5. **Port conflicts.** Find the owner with `netstat -ano | findstr :47551`. Windows can also reserve port ranges (`netsh interface ipv4 show excludedportrange protocol=tcp`). Free the port, or enable port fallback, or pick another port in Settings, then press **Retry**.
6. **Other clients and containers.** A client in WSL, a container, or on another machine cannot reach Windows loopback through `127.0.0.1`.

**`403` from the endpoint**: the request carried a non-local `Origin` or `Host` header. Use `127.0.0.1` or `localhost` with the endpoint's port.

**`aztray_set_config` not found**: it was replaced by `aztray_set_settings` and the instance tools in v0.3.0.

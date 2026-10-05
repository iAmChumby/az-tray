# Multi-instance Azurite + robust MCP: implementation plan (issue #1)

Status: **implemented** on branch `fix/issue-1-mcp-multi-instance` for release v0.3.0 (version bump, release workflow assertion, and docs done in the integration step; tagging pending). Original status: contract frozen. `src-tauri/src/types.rs`, `src/lib/types.ts`, and `src/lib/ipc.ts` are the source of truth for every type and command named below. Three agents (`engine`, `mcp`, `ui`) build against them in parallel; see section 9 for file ownership.

## 1. Problem 1: MCP endpoint refuses connections

### Root cause (verified against git history)

1. **The installed binary predates the MCP server.** `git log -- src-tauri/src/mcp.rs` shows one commit, `6f61a65` "Add local MCP control and agent setup guide", dated **2026-09-29**. The reporter's `az-tray.exe` was built **2026-09-22**, so it contains no MCP code at all. That matches every symptom: no listener on 47551, no `mcp`/`tools/list`/`inputSchema` strings in the exe, ECONNREFUSED.
2. **No release contains MCP.** Tags `v0.1.1` to `v0.1.5` all sit on 2026-09-22 commits (latest `v0.1.5` = `c46b526`, which precedes `6f61a65`). `package.json`, `Cargo.toml`, and `tauri.conf.json` say `0.2.0` but there is no `v0.2.0` tag. `.github/workflows/release.yml` triggers only on `v*` tag pushes, and `scripts/install.ps1` defaults to the GitHub `releases/latest`, which is `v0.1.5`. Anyone following the README or `docs/AGENT-MCP-SETUP.md` installs a binary without MCP. (The setup doc tells users to build from source, but nothing in the app or release says the binary is stale.)
3. **The MCP code on `main` is compiled in unconditionally** (no cargo feature or `cfg` gate; `pub mod mcp;` in `lib.rs`) and is started in `setup()` on defaults. Defaults need no `config.json`: the absent file is not a cause. There is no enable flag today, so "gated off by default" is not a cause either.
4. **Silent-failure gaps that would hide a real failure** (these are the robustness bugs to fix regardless):
   - Bind failure in `setup()` is reported with `eprintln!`. The release exe uses `windows_subsystem = "windows"`, so stderr goes nowhere. The only surfaced signal is the dashboard card.
   - There is **no persistent log file**. `logs.rs` is an in-memory ring buffer of Azurite output plus an explicit "save logs" export. Nothing writes AzTray's own events to disk, which is why the reporter's log files were empty.
   - The port is hard-coded (47551) with no fallback and no retry. A transient owner (or a Windows excluded-port range) leaves MCP dead until restart.
   - `tray::build(...)?` runs before MCP start in `setup()`; a tray failure aborts setup and MCP never starts.
   - `McpStatus.active` flips true after `Server::http` bind only. Nothing verifies the socket answers.
   - `McpStatus` is only visible on the dashboard settings card. The popover and the snapshot do not carry it, and there is no build/version identity to reveal a stale install.
   - A listener-thread death after startup sets `active=false` but nothing retries.

### Required fixes (owner in parentheses, see section 9)

| # | Fix | Owner |
|---|---|---|
| F1 | MCP is enabled by default through `McpConfig { enabled: true, port: 47551, portFallback: true }`; a missing `mcp` key in `config.json` means defaults. | engine (config.rs) |
| F2 | Start MCP in `setup()` **before** `tray::build` and treat both as non-fatal; log any failure and keep running. | engine (lib.rs) |
| F3 | `McpServer::start(engine, on_quit) -> McpServer` never fails. It runs a supervisor thread: try `port`, then `port+1..port+9` when `portFallback`; after binding, self-connect to `127.0.0.1:<port>` and only then mark `running`; on failure or listener death retry every 15 s (exponential to 60 s). Every transition updates the status via `AppEngine::set_mcp_status`. | mcp |
| F4 | Every MCP lifecycle event (attempt, bind failure with the OS error, fallback used, running, stopped, retry) is written with `logs::app_log` to `%APPDATA%\AzTray\logs\aztray.log` (append, rotate at 1 MB to `aztray.log.1`). `eprintln!` is replaced everywhere. | engine (logs.rs), mcp calls it |
| F5 | MCP status is part of `AppSnapshot.mcp` and pushed through `mcp_updated`. UI shows it in the popover footer (dot: green running / amber retrying / red failed; label with the port) and in a dashboard "Local MCP" card with `url`, `lastEvent`, `error`, attempts, a Copy-URL button, and a **Retry** button (`restart_mcp`). Setting UI: enabled toggle, port, fallback toggle (saved with `set_settings`, then `restart_mcp`). | ui |
| F6 | `AppSnapshot.app = { version, features: ["mcp","multiInstance"], configPath, logPath, configError }`. UI shows "AzTray v{version}" and the feature list in Settings > About, and warns when `features` lacks `mcp`. MCP `initialize` reports the same version as `serverInfo.version`. | engine/ui |
| F7 | New command `get_app_log({limit})` returning the tail of `aztray.log`; the dashboard Settings page gets a "Diagnostics" section showing `logPath` and the last lines. | engine/ui |
| F8 | Release hygiene: bump version to **0.3.0** (breaking config change) in `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`; add to `release.yml` a `cargo test --manifest-path src-tauri/Cargo.toml` step and a post-build assertion that the built `az-tray.exe` contains the string `aztray_mcp_status` (fail the release otherwise); tag `v0.3.0` so `releases/latest` serves an MCP build. `docs/AGENT-MCP-SETUP.md` leads with "Install the v0.3.0+ release" and says to check `AzTray Dashboard > Settings > About` for the version. | integration step (section 9) |
| F9 | `docs/AGENT-MCP-SETUP.md` documents the fallback port, the Retry button, the log path, and that `aztray_mcp_status` returns `{enabled,running,url,port,requestedPort,fallbackUsed,error,lastEvent,attempts}`. | mcp |

Optional hardening for the mcp agent: also listen on `[::1]:<port>` so clients that resolve `localhost` to IPv6 do not hit ECONNREFUSED. A failure to bind `::1` is logged but never marks the status failed.

## 2. Problem 2: multiple concurrent Azurite instances

An **instance** is the unit users create, name, start, and stop: one Blob + Queue + Table trio (three child processes, exactly as today) with its own host, ports, and data directory. The engine keeps one `ServiceRecord` and one `ManagedProcess` per `(instanceId, ServiceName)`.

Azurite CLI flags per service process, per instance: `--{blob|queue|table}Host <host> --{blob|queue|table}Port <port> --location <dataDirectory>` (as now), plus `--loose` when `loose`, plus `--skipApiVersionCheck` when `skipApiVersionCheck`. The global `executablePath` / `nodePath` apply to all instances.

### Data model (authoritative: types.rs / types.ts)

```
AppConfig        { schemaVersion: 2, executablePath, nodePath, mcp: McpConfig, instances: InstanceConfig[] }
McpConfig        { enabled=true, port=47551, portFallback=true }
InstanceConfig   { id, name, host, ports{blob,queue,table}, dataDirectory, loose=false, skipApiVersionCheck=false }
InstanceSnapshot { config, state: stopped|starting|running|partial|broken, services{blob,queue,table}: ServiceSnapshot,
                   connection: ConnectionInfo, logs{service: LogEntry[]}, mergedLogs: LogEntry[] }
ConnectionInfo   { instanceId, accountName, accountKey, endpoints{svc}, connectionStrings{svc}, connectionString }
ServiceSnapshot  { instanceId, name, state, host, port, pid, ... }   // + instanceId, otherwise unchanged
LogEntry         { id, sequence, instanceId, service, stream, level, message, timestamp }
McpStatus        { enabled, running, url, port, requestedPort, fallbackUsed, error, startedAt, lastEvent, attempts }
AppInfo          { version, features, configPath, logPath, configError }
AppSnapshot      { app, config: AppConfig, engine, mcp, instances: InstanceSnapshot[], generatedAt }
```

The old `Config`, `ConfigUpdate`, and `AppSnapshot.services/logs/mergedLogs` are removed. `QuitResult.stoppedServices` is now `ServiceRef[]` (`{instanceId, serviceName}`).

### Instance identity and selectors

- `id`: slug `^[a-z0-9][a-z0-9-]{0,31}$`, immutable, unique. Auto-generated from `name` (lowercase, non `[a-z0-9]` runs become `-`, trimmed, `-2`, `-3`... on collision). `default` is reserved for the migrated instance.
- `name`: 1 to 48 chars after trim, unique case-insensitively.
- **Selector** (every `instanceId` arg in commands, every `instance` arg in MCP): matches an `id` exactly, otherwise a `name` case-insensitively. If omitted: the `default` instance if present; else the sole instance; else the error `multiple instances; specify instance. Available: default ("Default"), dev ("Dev")`. Not found: `no instance matches "x". Available: ...`.

### Port allocation

- Default instance: **10000 / 10001 / 10002** (blob/queue/table), host `127.0.0.1`.
- New instance (auto): scan trios `(b, b+1, b+2)` starting at `b = 10000` in steps of 3 up to `b = 19998`. The first trio qualifies when **all** hold: none of the three ports is used by any configured instance (running or not) or equals the MCP port (configured `mcp.port` through `mcp.port+9`); and none is currently bound by an OS listener (`ports::is_port_free` for each). Result for the second instance: 10003/10004/10005, third: 10006/10007/10008, then skipping any trio that is busy.
- Explicit ports on create/update are validated, never silently changed: three distinct ports in 1..=65535; none collides with another instance's configured ports or with the MCP port range; error text names the conflicting instance (`blob port 10003 is already assigned to instance "dev"`). A port that is merely busy on the OS at config time is **allowed** (the existing `portInUse` flow at start time handles it) but `create_instance`/`update_instance` return it as a warning inside the log (`aztray.log`) only; the UI form should pre-check with `identify`-style `suggest_instance` and show the live status.
- Data directory: default instance keeps the migrated directory (default `%USERPROFILE%\.azurite`). New instances default to `%LOCALAPPDATA%\AzTray\instances\<id>\data` (fallback `app_data_directory()/instances/<id>/data`). Two instances may not share a normalized (case-insensitive, trailing-slash-trimmed, absolute) data directory, and one may not be nested inside another. AzTray creates the directory on start and never reads, alters, or deletes its contents.
- Conflicts at start time: unchanged per-service probe against the OS (`portInUse` state with owner identity, `free_port` flow), now scoped to `(instanceId, service)`.

### Config migration

`%APPDATA%\AzTray\config.json` loading order:
1. If it exists and has `instances` (array) -> parse as v2. Missing `mcp` -> defaults. Missing per-instance `loose`/`skipApiVersionCheck` -> false.
2. If it exists without `instances` (v1: `host`, `ports{}`, `dataDirectory`, `executablePath`, `nodePath`, plus the legacy aliases `parse_value` already accepts) -> build one instance `{id:"default", name:"Default", host, ports, dataDirectory}`; global `executablePath`/`nodePath` move to the top level; `mcp` = defaults. Copy the original to `config.v1.json.bak` (once, never overwrite), then rewrite as v2. If the backup or rewrite fails, run in memory and set `AppInfo.configError` to the reason.
3. If absent and `%APPDATA%\azctl\config.json` exists -> same as step 2 from that file (read-only, never written), persisted as v2 AzTray config.
4. If absent everywhere -> `default_config()`: one `default` instance on 10000/10001/10002, default data dir, MCP defaults. (Written lazily on first change; the app must work with no file.)
5. Malformed file -> defaults in memory, `configError` set, the bad file is not overwritten until the user saves a change (and then it is first copied to `config.broken.json.bak`).

`config::validate(&AppConfig)` enforces: schemaVersion 2, at least one instance, id/name rules above, per-instance port/dir rules, global uniqueness, `mcp.port` in 1024..=65535 and not inside any instance port, `host` non-empty.

## 3. Engine public API (`AppEngine`)

`AppEngine` is the existing `#[derive(Clone)] struct AppEngine { inner: Arc<Mutex<EngineInner>> }`. All methods take `&self`, are blocking (callers use `spawn_blocking` or a thread, as `lib.rs` does today), return `Result<_, String>` with user-facing messages, and emit `EngineEvent`s. A selector argument `Option<&str>` is resolved by `resolve_instance`; `None` follows the default rule.

```rust
// ---- construction / events ----
impl AppEngine {
    pub fn new() -> Self;                                   // loads + migrates config, opens aztray.log
    pub fn set_event_handler(&self, h: Option<EventHandler>);

    // ---- snapshots ----
    pub fn get_snapshot(&self) -> AppSnapshot;
    pub fn list_instances(&self) -> Vec<InstanceSnapshot>;
    pub fn get_instance(&self, selector: Option<&str>) -> Result<InstanceSnapshot, String>;
    pub fn resolve_instance_id(&self, selector: Option<&str>) -> Result<String, String>;
    pub fn check_engine(&self) -> EngineSnapshot;

    // ---- settings / instance CRUD ----
    pub fn set_settings(&self, settings: GlobalSettings) -> Result<AppSnapshot, String>;       // exe/node need all stopped; mcp edits persist only (caller restarts MCP)
    pub fn suggest_instance(&self, name: Option<&str>) -> InstanceDraft;
    pub fn create_instance(&self, req: CreateInstanceRequest) -> Result<InstanceSnapshot, String>; // persists; if req.start, starts all 3 and returns after launch
    pub fn update_instance(&self, req: UpdateInstanceRequest) -> Result<InstanceSnapshot, String>;
    pub fn delete_instance(&self, selector: &str) -> Result<AppSnapshot, String>;               // must be stopped; last instance cannot be deleted; data untouched

    // ---- lifecycle ----
    pub fn start_instance(&self, selector: Option<&str>) -> Result<AppSnapshot, String>;
    pub fn stop_instance(&self, selector: Option<&str>) -> Result<AppSnapshot, String>;
    pub fn restart_instance(&self, selector: Option<&str>) -> Result<AppSnapshot, String>;
    pub fn start_service(&self, selector: Option<&str>, service: ServiceName) -> Result<AppSnapshot, String>;
    pub fn stop_service(&self, selector: Option<&str>, service: ServiceName) -> Result<AppSnapshot, String>;
    pub fn restart_service(&self, selector: Option<&str>, service: ServiceName) -> Result<AppSnapshot, String>;
    pub fn start_all(&self) -> Result<AppSnapshot, String>;      // every instance; first error returned after attempting all
    pub fn stop_all(&self) -> Result<AppSnapshot, String>;
    pub fn restart_all(&self) -> Result<AppSnapshot, String>;

    // ---- ports ----
    pub fn identify_port_owner(&self, selector: Option<&str>, service: ServiceName) -> Option<PortOwner>;
    pub fn free_port(&self, expected: PortOwnerExpectation) -> Result<FreePortResult, String>;          // uses expected.instance_id
    pub fn free_port_confirmed(&self, expected: PortOwnerExpectation, confirmed: bool) -> Result<FreePortResult, String>;

    // ---- logs ----
    pub fn get_logs(&self, query: LogsQuery) -> Vec<LogEntry>;   // query.instance_id None => resolved by default rule
    pub fn save_logs(&self, args: SaveLogsArgs) -> Result<SaveLogsResult, String>;
    pub fn clear_logs(&self, selector: Option<&str>, service: Option<ServiceName>) -> AppSnapshot;
    pub fn app_log_tail(&self, limit: usize) -> Vec<String>;      // lines of aztray.log, oldest first

    // ---- connection provisioning ----
    pub fn connection_info(&self, selector: Option<&str>) -> Result<ConnectionInfo, String>;
    pub fn connection_string(&self, selector: Option<&str>, service: Option<ServiceName>) -> Result<String, String>; // None => combined

    // ---- MCP status plumbing (state lives in the engine so snapshots carry it) ----
    pub fn mcp_config(&self) -> McpConfig;
    pub fn mcp_status(&self) -> McpStatus;
    pub fn set_mcp_status(&self, status: McpStatus);              // stores, logs transition, emits McpUpdated + SnapshotUpdated

    // ---- shutdown ----
    pub fn quit(&self, mode: QuitMode) -> Result<QuitResult, String>; // StopAndQuit stops every instance's owned services
}
```

`EngineEvent` (serialized with `tag="type"`, `content="payload"`, snake_case) gains `InstanceUpdated(InstanceSnapshot)` and `McpUpdated(McpStatus)`. Tauri event names: `snapshot_updated`, `instance_updated`, `service_updated`, `log_entry`, `engine_updated`, `mcp_updated`.

Connection string format (per `ConnectionInfo`): account `devstoreaccount1`, key = `AZURITE_ACCOUNT_KEY` (already in engine.rs). Endpoint = `http://{host}:{port}/devstoreaccount1` with host `0.0.0.0`/`::` shown as `127.0.0.1`. Per-service string: `DefaultEndpointsProtocol=http;AccountName=devstoreaccount1;AccountKey=<key>;{Blob|Queue|Table}Endpoint=<endpoint>;`. Combined: all three endpoints in one string (order Blob, Queue, Table).

`ports.rs` additions (engine owns):

```rust
pub fn is_port_free(host: &str, port: u16) -> bool;                       // TcpListener::bind probe, no process spawn
pub fn find_free_port_trio(host: &str, taken: &HashSet<u16>, start: u16, end: u16) -> Option<[u16; 3]>; // stride 3, [blob,queue,table]
```

`config.rs` additions (engine owns): `load() -> LoadedConfig { config: AppConfig, error, source }`, `persist(&AppConfig)`, `validate(&AppConfig)`, `instance_data_directory(id) -> PathBuf`, `slugify(name) -> String`, `app_log_path() -> PathBuf`.

`logs.rs` additions (engine owns): `LogStore` keyed by `(instance_id, ServiceName)`; `push*` take an instance id; `query`/`all`/`clear`/`save` filter by instance; `pub fn app_log(level: LogLevel, message: &str)` appends to `aztray.log` (thread-safe, rotating, never panics; failures swallowed after one best-effort stderr write). `merged` order stays arrival order per instance.

### Shared state struct (lib.rs, engine owns)

```rust
#[derive(Clone)]
pub struct AppState {
    pub engine: AppEngine,                                   // owns McpStatus now
    pub mcp_server: Arc<Mutex<Option<mcp::McpServer>>>,      // dropping stops + joins
}
```
The old `AppState.mcp_status: Arc<RwLock<McpStatus>>` is removed. `mcp::McpStatus` is removed too: `mcp.rs` does `pub use crate::types::McpStatus;` only if it needs the name.

### MCP module API (mcp.rs, mcp agent owns; lib.rs calls only this)

```rust
pub struct McpServer { /* supervisor thread handle + stop flag */ }
impl McpServer {
    /// Never fails and never blocks on bind. Reads engine.mcp_config(); if !enabled
    /// publishes McpStatus{enabled:false,running:false,..} and spawns no listener.
    /// Otherwise spawns the supervisor (section 1 F3). All status goes through engine.set_mcp_status.
    pub fn start(engine: AppEngine, on_quit: Arc<dyn Fn() + Send + Sync + 'static>) -> McpServer;
    pub fn stop(self);                       // also called by Drop
}
```
`lib.rs` commands `restart_mcp` and settings changes do: take `mcp_server`, drop the old value, `McpServer::start(...)`, return `engine.mcp_status()`.

## 4. Tauri commands (lib.rs, engine owns). Names are snake_case; JS args camelCase

`instanceId` is `Option<String>` in Rust on every command below; the TS wrappers always pass it.

| Command | Rust params (after `app: AppHandle`) | Returns | Replaces |
|---|---|---|---|
| `get_snapshot` | - | `AppSnapshot` | same |
| `set_settings` | `settings: GlobalSettings` | `Result<AppSnapshot,String>` | `set_config` (removed) |
| `check_engine` | - | `EngineSnapshot` | same |
| `quit_app` | `mode: QuitMode` | `Result<QuitResult,String>` | same |
| `list_instances` | - | `Vec<InstanceSnapshot>` | new |
| `suggest_instance` | `name: Option<String>` | `InstanceDraft` | new |
| `create_instance` | `request: CreateInstanceRequest` | `Result<InstanceSnapshot,String>` | new |
| `update_instance` | `request: UpdateInstanceRequest` | `Result<InstanceSnapshot,String>` | new |
| `delete_instance` | `request: DeleteInstanceRequest` | `Result<AppSnapshot,String>` | new |
| `start_instance` / `stop_instance` / `restart_instance` | `instance_id: Option<String>` | `Result<AppSnapshot,String>` | new |
| `start_service` / `stop_service` / `restart_service` | `instance_id: Option<String>, service_name: ServiceName` | `Result<AppSnapshot,String>` | gains `instance_id` |
| `start_all` / `stop_all` / `restart_all` | - | `Result<AppSnapshot,String>` | now spans all instances |
| `identify_port_owner` | `instance_id: Option<String>, service_name: ServiceName` | `Option<PortOwner>` | gains `instance_id` |
| `free_port` | `instance_id: Option<String>, service_name, pid: u32, started_at: Option<String>` | `Result<FreePortResult,String>` | gains `instance_id` |
| `get_logs` | `instance_id: Option<String>, service_name: Option<ServiceName>, limit: Option<usize>` | `Vec<LogEntry>` | gains `instance_id` |
| `save_logs` | `instance_id: Option<String>, service_name: Option<ServiceName>, path: Option<String>` | `Result<SaveLogsResult,String>` | gains `instance_id` |
| `clear_logs` | `instance_id: Option<String>, service_name: Option<ServiceName>` | `AppSnapshot` | gains `instance_id` |
| `get_app_log` | `limit: Option<usize>` | `Vec<String>` | new |
| `get_connection_info` | `instance_id: Option<String>` | `Result<ConnectionInfo,String>` | new |
| `get_connection_string` | `instance_id: Option<String>, service_name: Option<ServiceName>` | `Result<String,String>` | gains `instance_id`, optional service, fallible |
| `get_mcp_status` | - | `McpStatus` | now from engine |
| `restart_mcp` | - | `McpStatus` | new |

TS wrappers in `src/lib/ipc.ts` (exact exports): `getSnapshot, setSettings(settings), checkEngine, quitApp(mode), listInstances, suggestInstance(name?), createInstance(request), updateInstance(request), deleteInstance(instanceId), startInstance/stopInstance/restartInstance(instanceId), startService/stopService/restartService(instanceId, serviceName), startAll/stopAll/restartAll, identifyPortOwner(instanceId, serviceName), freePort(args), getLogs(args), saveLogs(args), clearLogs(instanceId?, serviceName?), getAppLog(limit?), getConnectionInfo(instanceId), getConnectionString(instanceId, serviceName?), getMcpStatus, restartMcp, subscribe`, plus the `aztrayIpc` aggregate.

## 5. MCP tool surface (mcp.rs, mcp agent owns)

Rules for all tools:
- Server name `aztray`, `serverInfo.version` = `CARGO_PKG_VERSION`. Protocol handling in mcp.rs is unchanged.
- Existing tool names are kept. A tool that acts on an instance gains an optional `instance` string (id or case-insensitive name, default rule in section 2) next to its existing arguments. `serviceName` stays `blob|queue|table`.
- Tool results are the JSON of the engine return value (`AppSnapshot`, `InstanceSnapshot`, ...). Errors are tool errors (`isError:true`) carrying the engine `String`.
- `instance` property used below: `{"type":"string","minLength":1,"description":"Instance id or name. Omit for the default instance (or the only instance)."}`.
- `serviceName` property used below: `{"type":"string","enum":["blob","queue","table"]}`.
- All schemas use `"additionalProperties": false`. Annotations: `[R]` readOnly, `[W]` mutating non-destructive (`idempotentHint` per row), `[D]` destructive.

| Tool | Ann. | `inputSchema` properties (required in bold) | Behavior |
|---|---|---|---|
| `aztray_snapshot` | R | none | `engine.get_snapshot()` |
| `aztray_mcp_status` | R | none | `engine.mcp_status()` (new shape) |
| `aztray_list_instances` | R | none | `engine.list_instances()` (each with `connection`) |
| `aztray_get_instance` | R | `instance` | `engine.get_instance` |
| `aztray_create_instance` | W | **`name`** string 1..48; `id` string `^[a-z0-9][a-z0-9-]{0,31}$`; `host` string; `ports` object `{blob,queue,table}` integers 1..65535 (all three required if present); `dataDirectory` string; `loose` boolean; `skipApiVersionCheck` boolean; `start` boolean (default **true** for MCP) | `engine.create_instance`; MCP defaults `start` to true so one call provisions and returns a running instance plus `connection.connectionString` |
| `aztray_update_instance` | W (idempotent) | **`instance`**; `name`; `host`; `ports`; `dataDirectory`; `loose`; `skipApiVersionCheck` | `engine.update_instance`; non-name fields require the instance stopped |
| `aztray_delete_instance` | D | **`instance`**, **`confirmed`** `{"type":"boolean","const":true}` | `engine.delete_instance`; refuses unless stopped; never deletes Azurite data |
| `aztray_start_instance` / `aztray_stop_instance` / `aztray_restart_instance` | W / D / D | `instance` | engine instance lifecycle |
| `aztray_start_service` / `aztray_stop_service` / `aztray_restart_service` | W / D / D | `instance`, **`serviceName`** | engine service lifecycle |
| `aztray_start_all` / `aztray_stop_all` / `aztray_restart_all` | W / D / D | none | every instance |
| `aztray_connection_string` | R | `instance`, `serviceName` | With `serviceName` -> `{"instance":id,"serviceName":s,"connectionString":..}`; without -> `{"instance":id,"connectionString":combined,"connectionStrings":{blob,queue,table},"endpoints":{..}}` (the full `ConnectionInfo`) |
| `aztray_get_config` | R | none | `AppConfig` (all instances, global settings, MCP config) |
| `aztray_set_settings` | W (idempotent) | `executablePath` (string\|null), `nodePath` (string\|null) | merges into `GlobalSettings`, keeping current `mcp`; MCP port/enabled are **not** changeable over MCP (use the dashboard) so a client cannot cut its own connection. Replaces `aztray_set_config`. |
| `aztray_check_engine` | R | none | `engine.check_engine()` |
| `aztray_identify_port_owner` | R | `instance`, **`serviceName`** | `engine.identify_port_owner` |
| `aztray_free_port` | D | `instance`, **`serviceName`**, **`pid`** int 1..4294967295, **`startedAt`** string\|null, **`confirmed`** const true | unchanged safety rules (identity recheck in engine) |
| `aztray_get_logs` | R | `instance`, `serviceName`, `limit` int 0..6000 | `engine.get_logs` |
| `aztray_save_logs` | D | `instance`, `serviceName`, `path` string | `engine.save_logs` |
| `aztray_clear_logs` | D (idempotent) | `instance`, `serviceName` | `engine.clear_logs` |
| `aztray_get_app_log` | R | `limit` int 1..2000 (default 200) | `{"path":..,"lines":[..]}` from `engine.app_log_tail` |
| `aztray_quit` | D | **`mode`** enum `stop_and_quit|leave_running|cancel` | unchanged; `stop_and_quit` stops every instance |

Removed: `aztray_set_config` (replaced by `aztray_set_settings` + the instance tools). The catalog must stay deterministic and ordered as in this table (existing test `tool_catalog_is_deterministic_and_marks_mutation` asserts the first tool and a destructive flag; update it to the new first tool `aztray_snapshot` unchanged and keep `aztray_free_port` destructive).

Agent-facing provisioning workflow to document in `docs/AGENT-MCP-SETUP.md`: `aztray_create_instance {name:"integration-tests"}` -> response `connection.connectionString` -> use it; `aztray_delete_instance` when done (after `aztray_stop_instance`).

## 6. UI behavior (ui agent)

- `useAzTray` model: `snapshot`, `instances`, `selectedInstanceId` (persisted in localStorage, falls back to `default`), `mcpStatus` (from `snapshot.mcp` and `mcp_updated`), actions parameterized by instance. Keep the existing optimistic/error handling patterns (`actionErrorMessage`).
- Popover: instance switcher (compact pills or a select) above the three service rows; per-instance Start/Stop all; footer with MCP dot + `:port` + "Retry" when failed. A "running instances" count when more than one exists.
- Dashboard: left rail lists instances (name, aggregate state dot, ports); header actions for the selected instance (Start/Stop/Restart instance); "New instance" dialog prefilled from `suggest_instance` (name, 3 ports, data dir, loose, skip API check, "Start after creating"); instance settings tab (edit while stopped, delete with confirm and the note that data on disk is untouched); Connection panel per instance (combined string + per-service strings, copy buttons, endpoints); Settings page: global engine paths, MCP card (enabled, port, fallback, status, last event, error, Retry), About (version, features, configPath, logPath), Diagnostics (`get_app_log` tail with refresh/copy).
- `mockBackend.ts` implements every command in `CommandMap` with an in-memory multi-instance model, incl. port auto-assignment, selector resolution, MCP states (running / failed / fallback), so `npm run build` and browser dev work without Tauri.

## 7. Compatibility notes

- Event payload `ServiceSnapshot.instanceId` and `LogEntry.instanceId` let listeners route partial updates; `snapshot_updated` carries the full `AppSnapshot` as before.
- Snapshot size: each `InstanceSnapshot` carries bounded logs (2000/service). With many instances prefer `instance_updated` for incremental updates; the engine may cap per-service logs inside snapshots to the newest 500 while `get_logs` returns the full buffer.
- Behavior of `start_all/stop_all/restart_all` changed from "my one instance" to "every instance". The tray menu keeps these labels but should read "Start all instances".

## 8. Open questions (defaults chosen; flag to Luke)

1. Version bump target `0.3.0` vs keeping `0.2.0`: chosen 0.3.0 because the config schema and Tauri command set change.
2. MCP port fallback changes the URL clients registered; chosen to keep fallback on by default since a visible `url` + `fallbackUsed` is better than a dead endpoint. Users who want a fixed URL turn off `portFallback`.
3. `aztray_create_instance` defaults `start:true` for MCP but `false` for the Tauri command. Chosen because agents want a working endpoint in one call.
4. Azurite instances still all use the `devstoreaccount1` account; per-instance accounts (`AZURITE_ACCOUNTS`) are out of scope.
5. Tray-menu multi-instance rendering is minimal (global start/stop/open dashboard only).

## 9. File ownership (no file has two owners)

| Agent | Owns (may edit) | Must not edit |
|---|---|---|
| **engine** | `src-tauri/src/config.rs`, `engine.rs`, `ports.rs`, `lib.rs` (AppState, `setup()` ordering F2, command registration, new commands, event forwarding incl. `instance_updated`/`mcp_updated`), `tray.rs`, `logs.rs`, `src-tauri/Cargo.toml` (only if a dependency is needed), follow-up fixes to `types.rs` (additive only; announce any change in the final report) | `mcp.rs`, anything under `src/` or `app/` |
| **mcp** | `src-tauri/src/mcp.rs`, `docs/AGENT-MCP-SETUP.md`, plus mcp.rs unit tests | everything else; if a needed engine signature is missing or wrong, report it instead of editing |
| **ui** | `app/**`, `src/**` (components, hooks, `mockBackend.ts`, `ipc.ts` implementation details, `types.ts` additive UI-only helpers), `package.json` only for UI deps | `src-tauri/**`; the exported type shapes in `src/lib/types.ts` and `types.rs` are frozen (add helpers, never change a wire field) |
| **integration** (orchestrator, after the three land) | version bump (`package.json`, `Cargo.toml`, `tauri.conf.json`), `.github/workflows/release.yml` (F8), `README.md`, `docs/SPEC.md`/`docs/LEDGER.md` updates, tag | - |

Order and handoffs: engine and mcp can build in parallel; mcp relies only on the signatures in section 3 (compile errors against an unfinished engine are expected until the engine agent lands; the mcp agent should unit-test with a stub or after engine merges). The ui agent depends only on `types.ts`/`ipc.ts`. Frozen contract files: `types.rs`, `types.ts`, `ipc.ts` signatures, this document.

## 10. Verification checklist (integration)

1. `cargo test` and `cargo check` in `src-tauri`; `npm run build` (no dev server running).
2. Fresh profile: no `config.json` -> app starts, MCP listening on 47551, `aztray_mcp_status` shows `running:true`.
3. Occupy 47551 first -> status `running:true, fallbackUsed:true, port:47552`, log line in `aztray.log`, UI shows the fallback URL; release 47551 and **Retry** keeps the working endpoint.
4. v1 `config.json` (host/ports/dataDirectory) -> one `default` instance with the same values, `config.v1.json.bak` created, Start works on 10000-10002.
5. `aztray_create_instance {name:"b"}` -> ports 10003-10005, running, unique data dir; `default` and `b` run concurrently; the two connection strings point at different ports; stop `b`, delete `b`, data dir remains on disk.
6. Port conflict between instance configs is rejected with the owning instance named; two instances cannot share a data dir.
7. Release exe contains `aztray_mcp_status` (CI assertion), and `releases/latest` serves the new tag.

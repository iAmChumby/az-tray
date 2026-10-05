//! A small, loopback-only MCP Streamable HTTP endpoint for the tray engine.
//!
//! `McpServer::start` never fails: it spawns a supervisor thread that binds the
//! configured port (with an optional fallback range), verifies the listener by
//! connecting to it and running an `initialize` probe, publishes every state
//! change through `AppEngine::set_mcp_status`, writes lifecycle events to the
//! persistent app log, and retries with backoff when binding or serving fails.
//!
//! The transport serves the 2026-07-28 request-metadata protocol and the
//! 2025-11-25 / 2025-06-18 / 2025-03-26 initialize handshake for older clients
//! (Claude Code, Codex). Tool calls use the same `AppEngine` as the UI.

use crate::engine::AppEngine;
use crate::logs;
use crate::types::{
    CreateInstanceRequest, GlobalSettings, LogLevel, LogsQuery, McpConfig, PortOwnerExpectation,
    QuitMode, SaveLogsArgs, ServiceName, UpdateInstanceRequest,
};
pub use crate::types::McpStatus;
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

pub const MCP_PORT: u16 = crate::types::DEFAULT_MCP_PORT;
pub const MCP_PATH: &str = "/mcp";
pub const MODERN_PROTOCOL_VERSION: &str = "2026-07-28";
pub const LEGACY_PROTOCOL_VERSION: &str = "2025-11-25";
const LEGACY_PROTOCOL_VERSIONS: [&str; 3] = [LEGACY_PROTOCOL_VERSION, "2025-06-18", "2025-03-26"];
/// Protocol assumed for legacy requests that omit `MCP-Protocol-Version`.
const ASSUMED_LEGACY_PROTOCOL_VERSION: &str = "2025-03-26";
const MAX_REQUEST_BYTES: usize = 1_048_576;
const MAX_IN_FLIGHT_REQUESTS: usize = 16;
const FALLBACK_SPAN: u16 = 9;
const RETRY_BASE_SECS: u64 = 15;
const RETRY_MAX_SECS: u64 = 60;
const SERVER_NAME: &str = "aztray";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
const INSTRUCTIONS: &str = "AzTray controls local Azurite emulator instances (each instance is one Blob + Queue + Table trio with its own ports and data directory). Call aztray_list_instances to see them, aztray_create_instance to provision a new isolated instance (it returns connection.connectionString), and aztray_stop_instance then aztray_delete_instance when done. Tools that take an optional `instance` argument accept an instance id or name; omit it to target the default instance. aztray_free_port requires the observed process PID and creation timestamp plus confirmed=true.";

pub struct McpServer {
    stop: Arc<AtomicBool>,
    supervisor: Option<JoinHandle<()>>,
}

impl McpServer {
    /// Never fails and never blocks on bind. Reads `engine.mcp_config()`; when
    /// disabled it publishes a disabled status and spawns no listener.
    pub fn start(engine: AppEngine, on_quit: Arc<dyn Fn() + Send + Sync + 'static>) -> McpServer {
        let config = engine.mcp_config();
        Self::start_with_config(engine, on_quit, config)
    }

    pub(crate) fn start_with_config(
        engine: AppEngine,
        on_quit: Arc<dyn Fn() + Send + Sync + 'static>,
        config: McpConfig,
    ) -> McpServer {
        let stop = Arc::new(AtomicBool::new(false));
        if !config.enabled {
            let mut status = McpStatus::initial(&config);
            status.enabled = false;
            status.last_event = Some("MCP endpoint is disabled in settings".into());
            logs::app_log(LogLevel::Info, "MCP: disabled in settings; not listening");
            engine.set_mcp_status(status);
            return McpServer {
                stop,
                supervisor: None,
            };
        }

        let supervisor_stop = stop.clone();
        let supervisor_engine = engine.clone();
        let spawned = thread::Builder::new()
            .name("aztray-mcp-supervisor".into())
            .spawn(move || supervise(supervisor_engine, on_quit, config, supervisor_stop));
        match spawned {
            Ok(handle) => McpServer {
                stop,
                supervisor: Some(handle),
            },
            Err(error) => {
                let message = format!("could not start the MCP supervisor thread: {error}");
                logs::app_log(LogLevel::Error, &format!("MCP: {message}"));
                let mut status = McpStatus::initial(&engine.mcp_config());
                status.error = Some(message.clone());
                status.last_event = Some(message);
                engine.set_mcp_status(status);
                McpServer {
                    stop,
                    supervisor: None,
                }
            }
        }
    }

    /// Stops the listener and joins the supervisor. Also performed by `Drop`.
    pub fn stop(self) {
        drop(self);
    }
}

impl Drop for McpServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(supervisor) = self.supervisor.take() {
            let _ = supervisor.join();
        }
    }
}

// ---------------------------------------------------------------------------
// Supervisor
// ---------------------------------------------------------------------------

struct Reporter {
    engine: AppEngine,
    requested_port: u16,
    attempts: u32,
    started_at: Option<String>,
}

impl Reporter {
    fn publish(&self, running: bool, port: u16, error: Option<String>, last_event: String) {
        self.engine.set_mcp_status(McpStatus {
            enabled: true,
            running,
            url: format!("http://127.0.0.1:{port}{MCP_PATH}"),
            port,
            requested_port: self.requested_port,
            fallback_used: running && port != self.requested_port,
            error,
            started_at: if running { self.started_at.clone() } else { None },
            last_event: Some(last_event),
            attempts: self.attempts,
        });
    }
}

fn log_info(message: &str) {
    logs::app_log(LogLevel::Info, &format!("MCP: {message}"));
}

fn log_warn(message: &str) {
    logs::app_log(LogLevel::Warn, &format!("MCP: {message}"));
}

fn log_error(message: &str) {
    logs::app_log(LogLevel::Error, &format!("MCP: {message}"));
}

fn candidate_ports(config: &McpConfig) -> Vec<u16> {
    if config.port_fallback {
        let end = config.port.saturating_add(FALLBACK_SPAN);
        (config.port..=end).collect()
    } else {
        vec![config.port]
    }
}

fn backoff_delay(failures: u32) -> Duration {
    let exponent = failures.saturating_sub(1).min(8);
    let secs = RETRY_BASE_SECS
        .saturating_mul(1u64 << exponent)
        .min(RETRY_MAX_SECS);
    Duration::from_secs(secs)
}

/// Sleeps in short slices so a stop request is honored promptly. Returns true
/// when the stop flag was raised.
fn sleep_unless_stopped(stop: &AtomicBool, duration: Duration) -> bool {
    let mut remaining = duration;
    let slice = Duration::from_millis(100);
    while !remaining.is_zero() {
        if stop.load(Ordering::Acquire) {
            return true;
        }
        let step = remaining.min(slice);
        thread::sleep(step);
        remaining -= step;
    }
    stop.load(Ordering::Acquire)
}

/// Bound sockets plus the worker threads serving them.
struct Listener {
    stop: Arc<AtomicBool>,
    primary: Option<JoinHandle<()>>,
    secondary: Option<JoinHandle<()>>,
}

impl Listener {
    fn shutdown(mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.primary.take() {
            let _ = worker.join();
        }
        if let Some(worker) = self.secondary.take() {
            let _ = worker.join();
        }
    }

    fn primary_alive(&self) -> bool {
        self.primary
            .as_ref()
            .is_some_and(|worker| !worker.is_finished())
    }
}

fn bind_first_available(config: &McpConfig) -> Result<(Server, u16), String> {
    let mut failures = Vec::new();
    let candidates = candidate_ports(config);
    for port in &candidates {
        match Server::http(SocketAddr::from(([127, 0, 0, 1], *port))) {
            Ok(server) => return Ok((server, *port)),
            Err(error) => {
                let line = format!("bind 127.0.0.1:{port} failed: {error}");
                log_warn(&line);
                failures.push(format!("{port}: {error}"));
            }
        }
    }
    let range = match (candidates.first(), candidates.last()) {
        (Some(first), Some(last)) if first != last => format!("{first}-{last}"),
        (Some(first), _) => first.to_string(),
        _ => config.port.to_string(),
    };
    Err(format!(
        "could not bind 127.0.0.1 port {range} (another program may own it, or Windows may have reserved it): {}",
        failures.join("; ")
    ))
}

fn spawn_listener(
    server: Server,
    port: u16,
    engine: &AppEngine,
    on_quit: &Arc<dyn Fn() + Send + Sync>,
) -> Result<Listener, String> {
    let stop = Arc::new(AtomicBool::new(false));
    let primary = spawn_worker("aztray-mcp-http", server, port, engine, on_quit, &stop)?;

    // Optional IPv6 loopback so clients resolving `localhost` to ::1 connect.
    let secondary = match Server::http(SocketAddr::from((
        std::net::Ipv6Addr::LOCALHOST,
        port,
    ))) {
        Ok(server) => spawn_worker("aztray-mcp-http6", server, port, engine, on_quit, &stop).ok(),
        Err(error) => {
            log_info(&format!(
                "IPv6 loopback [::1]:{port} unavailable (IPv4 only): {error}"
            ));
            None
        }
    };
    Ok(Listener {
        stop,
        primary: Some(primary),
        secondary,
    })
}

fn spawn_worker(
    name: &str,
    server: Server,
    port: u16,
    engine: &AppEngine,
    on_quit: &Arc<dyn Fn() + Send + Sync>,
    stop: &Arc<AtomicBool>,
) -> Result<JoinHandle<()>, String> {
    let engine = engine.clone();
    let on_quit = on_quit.clone();
    let stop = stop.clone();
    thread::Builder::new()
        .name(name.into())
        .spawn(move || accept_loop(server, port, engine, on_quit, stop))
        .map_err(|error| format!("could not start the MCP listener thread: {error}"))
}

fn accept_loop(
    server: Server,
    port: u16,
    engine: AppEngine,
    on_quit: Arc<dyn Fn() + Send + Sync>,
    stop: Arc<AtomicBool>,
) {
    let in_flight = Arc::new(AtomicUsize::new(0));
    while !stop.load(Ordering::Acquire) {
        match server.recv_timeout(Duration::from_millis(100)) {
            Ok(Some(request)) => {
                if !try_acquire_request_slot(&in_flight) {
                    let _ = request.respond(http_error(
                        503,
                        "MCP request capacity is full; retry shortly",
                        None,
                    ));
                    continue;
                }
                let engine = engine.clone();
                let on_quit = on_quit.clone();
                let slot_counter = in_flight.clone();
                let failure_slot = in_flight.clone();
                if thread::Builder::new()
                    .name("aztray-mcp-request".into())
                    .spawn(move || {
                        let _slot = RequestSlot(slot_counter);
                        serve_request(request, &engine, &on_quit, port);
                    })
                    .is_err()
                {
                    failure_slot.fetch_sub(1, Ordering::Release);
                }
            }
            Ok(None) => {}
            Err(error) => {
                log_error(&format!("HTTP listener on port {port} stopped: {error}"));
                break;
            }
        }
    }
}

fn supervise(
    engine: AppEngine,
    on_quit: Arc<dyn Fn() + Send + Sync>,
    config: McpConfig,
    stop: Arc<AtomicBool>,
) {
    let mut reporter = Reporter {
        engine: engine.clone(),
        requested_port: config.port,
        attempts: 0,
        started_at: None,
    };
    let mut failures: u32 = 0;

    while !stop.load(Ordering::Acquire) {
        reporter.attempts += 1;
        reporter.started_at = None;
        let attempt = reporter.attempts;
        let attempting = format!("Attempt {attempt}: binding 127.0.0.1:{}", config.port);
        log_info(&attempting);
        reporter.publish(false, config.port, None, attempting);

        let failure = match bind_first_available(&config) {
            Err(message) => Some(message),
            Ok((server, port)) => match spawn_listener(server, port, &engine, &on_quit) {
                Err(message) => Some(message),
                Ok(listener) => match probe_endpoint(port) {
                    Err(message) => {
                        listener.shutdown();
                        Some(format!(
                            "bound 127.0.0.1:{port} but the self-check failed: {message}"
                        ))
                    }
                    Ok(()) => {
                        failures = 0;
                        reporter.started_at = Some(logs::now_timestamp());
                        let event = if port == config.port {
                            format!("Listening on 127.0.0.1:{port}")
                        } else {
                            format!(
                                "Listening on 127.0.0.1:{port} (requested port {} was unavailable)",
                                config.port
                            )
                        };
                        if port == config.port {
                            log_info(&event);
                        } else {
                            log_warn(&event);
                        }
                        reporter.publish(true, port, None, event);

                        while !stop.load(Ordering::Acquire) && listener.primary_alive() {
                            thread::sleep(Duration::from_millis(100));
                        }
                        let died = !stop.load(Ordering::Acquire);
                        listener.shutdown();
                        if died {
                            Some(format!("the listener on 127.0.0.1:{port} stopped unexpectedly"))
                        } else {
                            log_info(&format!("stopped listening on 127.0.0.1:{port}"));
                            None
                        }
                    }
                },
            },
        };

        let Some(message) = failure else { break };
        if stop.load(Ordering::Acquire) {
            break;
        }
        failures += 1;
        let delay = backoff_delay(failures);
        log_error(&format!("{message}; retrying in {}s", delay.as_secs()));
        reporter.publish(
            false,
            config.port,
            Some(message),
            format!("Retrying in {}s (attempt {attempt} failed)", delay.as_secs()),
        );
        if sleep_unless_stopped(&stop, delay) {
            break;
        }
    }
}

/// Connects to the freshly bound socket and performs a real `initialize`
/// round trip so `running` is only reported when the endpoint answers.
fn probe_endpoint(port: u16) -> Result<(), String> {
    let mut last_error = String::new();
    for _ in 0..5 {
        match probe_once(port) {
            Ok(()) => return Ok(()),
            Err(error) => last_error = error,
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err(last_error)
}

fn probe_once(port: u16) -> Result<(), String> {
    let body = json!({
        "jsonrpc": "2.0",
        "id": "aztray-selfcheck",
        "method": "initialize",
        "params": {
            "protocolVersion": LEGACY_PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": {"name": "aztray-selfcheck", "version": SERVER_VERSION}
        }
    })
    .to_string();
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2))
        .map_err(|error| format!("connect failed: {error}"))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(3)));
    let request = format!(
        "POST {MCP_PATH} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept: application/json, text/event-stream\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("write failed: {error}"))?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|error| format!("read failed: {error}"))?;
    let text = String::from_utf8_lossy(&response);
    let status_line = text.lines().next().unwrap_or_default();
    if !status_line.contains(" 200") {
        return Err(format!("unexpected response: {status_line}"));
    }
    if !text.contains("\"serverInfo\"") {
        return Err("initialize response did not contain serverInfo".into());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// HTTP transport
// ---------------------------------------------------------------------------

struct RequestSlot(Arc<AtomicUsize>);

impl Drop for RequestSlot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Release);
    }
}

fn try_acquire_request_slot(active: &AtomicUsize) -> bool {
    let mut current = active.load(Ordering::Acquire);
    loop {
        if current >= MAX_IN_FLIGHT_REQUESTS {
            return false;
        }
        match active.compare_exchange_weak(
            current,
            current + 1,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return true,
            Err(observed) => current = observed,
        }
    }
}

static SESSION_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Opaque, unguessable-enough session id. Sessions are advisory: AzTray holds
/// no per-session state, so any id (or none) is accepted on later requests.
fn new_session_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let counter = SESSION_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id() as u128;
    format!("{:032x}{:016x}", nanos ^ (pid << 96), counter)
}

fn serve_request(
    mut request: Request,
    engine: &AppEngine,
    on_quit: &Arc<dyn Fn() + Send + Sync>,
    port: u16,
) {
    let cors_origin = match validate_request_origin_and_host(&request, port) {
        Ok(origin) => origin,
        Err((status, message)) => {
            let _ = request.respond(http_error(status, message, None));
            return;
        }
    };

    let path = request.url().split('?').next().unwrap_or_default();
    if path != MCP_PATH {
        let _ = request.respond(http_error(
            404,
            "MCP endpoint not found",
            cors_origin.as_deref(),
        ));
        return;
    }

    if request.method() == &Method::Options {
        let mut response = Response::empty(StatusCode(204));
        response = with_cors(response, cors_origin.as_deref());
        response.add_header(header(
            "Access-Control-Allow-Methods",
            "POST, GET, DELETE, OPTIONS",
        ));
        response.add_header(header(
            "Access-Control-Allow-Headers",
            "Accept, Content-Type, MCP-Protocol-Version, Mcp-Method, Mcp-Name, Mcp-Session-Id, Last-Event-ID",
        ));
        response.add_header(header("Access-Control-Expose-Headers", "Mcp-Session-Id"));
        response.add_header(header("Access-Control-Max-Age", "600"));
        let _ = request.respond(response);
        return;
    }

    if request.method() == &Method::Get {
        let mut response =
            Response::from_string("This MCP endpoint does not provide a standalone event stream.")
                .with_status_code(StatusCode(405));
        response.add_header(header("Allow", "POST, DELETE, OPTIONS"));
        response = with_cors(response, cors_origin.as_deref());
        let _ = request.respond(response);
        return;
    }

    if request.method() == &Method::Delete {
        // Session termination: nothing is stored per session.
        let response = with_cors(Response::empty(StatusCode(200)), cors_origin.as_deref());
        let _ = request.respond(response);
        return;
    }

    if request.method() != &Method::Post {
        let mut response =
            Response::from_string("Method not allowed").with_status_code(StatusCode(405));
        response.add_header(header("Allow", "POST, DELETE, OPTIONS"));
        response = with_cors(response, cors_origin.as_deref());
        let _ = request.respond(response);
        return;
    }

    if !accepts_mcp_json(header_value(&request, "accept")) {
        let _ = request.respond(http_error(
            406,
            "Accept must include application/json",
            cors_origin.as_deref(),
        ));
        return;
    }
    if !is_json_content_type(header_value(&request, "content-type")) {
        let _ = request.respond(http_error(
            415,
            "Content-Type must be application/json",
            cors_origin.as_deref(),
        ));
        return;
    }
    if request
        .body_length()
        .is_some_and(|length| length > MAX_REQUEST_BYTES)
    {
        let _ = request.respond(http_error(
            413,
            "MCP request body is too large",
            cors_origin.as_deref(),
        ));
        return;
    }

    let mut body = Vec::new();
    let read_result = request
        .as_reader()
        .take((MAX_REQUEST_BYTES + 1) as u64)
        .read_to_end(&mut body);
    if read_result.is_err() {
        let _ = request.respond(http_error(
            400,
            "Could not read MCP request body",
            cors_origin.as_deref(),
        ));
        return;
    }
    if body.len() > MAX_REQUEST_BYTES {
        let _ = request.respond(http_error(
            413,
            "MCP request body is too large",
            cors_origin.as_deref(),
        ));
        return;
    }

    let rpc: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            let body = rpc_error(Value::Null, -32700, "Parse error", None);
            let _ = request.respond(json_response(400, body, cors_origin.as_deref()));
            return;
        }
    };

    if rpc
        .get("id")
        .is_some_and(|id| !id.is_string() && !id.is_number())
    {
        let error = rpc_error(Value::Null, -32600, "Invalid Request", None);
        let _ = request.respond(json_response(400, error, cors_origin.as_deref()));
        return;
    }

    let method = rpc.get("method").and_then(Value::as_str);
    let modern = request_is_modern(&rpc, header_value(&request, "mcp-protocol-version"));
    if let Err((status, error)) = validate_rpc_transport(&request, &rpc, modern) {
        let _ = request.respond(json_response(status, error, cors_origin.as_deref()));
        return;
    }

    let Some(method) = method else {
        let error = rpc_error(
            rpc.get("id").cloned().unwrap_or(Value::Null),
            -32600,
            "Invalid Request",
            None,
        );
        let _ = request.respond(json_response(400, error, cors_origin.as_deref()));
        return;
    };
    let has_id = rpc.get("id").is_some();
    if !has_id {
        // Notifications have no response body.
        let accepted = method.starts_with("notifications/");
        let mut response = Response::empty(StatusCode(if accepted { 202 } else { 400 }));
        response = with_cors(response, cors_origin.as_deref());
        let _ = request.respond(response);
        return;
    }

    let id = rpc.get("id").cloned().unwrap_or(Value::Null);
    let is_initialize = method == "initialize";
    let dispatched = dispatch_rpc(engine, &rpc, modern);
    let (status, response_body, exit_after_response) = match dispatched {
        Ok((result, should_quit)) => (200, rpc_result(id, result), should_quit),
        Err(error) => {
            let status = if modern && error.code == -32601 {
                404
            } else if error.code == -32022 || error.code == -32020 {
                400
            } else {
                200
            };
            (
                status,
                rpc_error(id, error.code, &error.message, error.data),
                false,
            )
        }
    };

    let mut response = json_response(status, response_body, cors_origin.as_deref());
    if is_initialize && status == 200 && !modern {
        response.add_header(header("Mcp-Session-Id", &new_session_id()));
    }
    let responded = request.respond(response).is_ok();
    if responded && exit_after_response {
        let on_quit = on_quit.clone();
        let _ = thread::Builder::new()
            .name("aztray-mcp-quit".into())
            .spawn(move || {
                thread::sleep(Duration::from_millis(100));
                on_quit();
            });
    }
}

fn validate_request_origin_and_host(
    request: &Request,
    port: u16,
) -> Result<Option<String>, (u16, &'static str)> {
    if header_count(request, "host") != 1 {
        return Err((400, "Exactly one Host header is required"));
    }
    if header_count(request, "origin") > 1 {
        return Err((403, "Only one Origin header is allowed"));
    }
    let Some(host) = header_value(request, "host") else {
        return Err((400, "Host header is required"));
    };
    let host = host.trim().to_ascii_lowercase();
    let allowed_hosts = [
        format!("127.0.0.1:{port}"),
        format!("localhost:{port}"),
        format!("[::1]:{port}"),
    ];
    if !allowed_hosts.iter().any(|allowed| allowed == &host) {
        return Err((403, "Host is not allowed for this local MCP endpoint"));
    }
    if !request
        .remote_addr()
        .map(|address| address.ip().is_loopback())
        .unwrap_or(false)
    {
        return Err((403, "MCP endpoint accepts loopback clients only"));
    }
    let Some(origin) = header_value(request, "origin") else {
        return Ok(None);
    };
    if !is_loopback_origin(origin) {
        return Err((403, "Origin is not allowed for this local MCP endpoint"));
    }
    Ok(Some(origin.to_string()))
}

fn is_loopback_origin(origin: &str) -> bool {
    if origin == "null" || origin.trim() != origin {
        return false;
    }
    let Some((scheme, authority)) = origin.split_once("://") else {
        return false;
    };
    if !["http", "https", "tauri"].contains(&scheme.to_ascii_lowercase().as_str())
        || authority.is_empty()
        || authority.contains(['/', '?', '#', '@'])
    {
        return false;
    }
    let host = if let Some(rest) = authority.strip_prefix('[') {
        let Some((host, suffix)) = rest.split_once(']') else {
            return false;
        };
        if !suffix.is_empty() {
            let Some(port) = suffix.strip_prefix(':') else {
                return false;
            };
            if port.is_empty() || port.parse::<u16>().is_err() {
                return false;
            }
        }
        host
    } else if let Some((host, port)) = authority.rsplit_once(':') {
        if host.contains(':') || port.is_empty() || port.parse::<u16>().is_err() {
            return false;
        }
        host
    } else {
        authority
    };
    host.eq_ignore_ascii_case("localhost")
        || host.eq_ignore_ascii_case("localhost.localdomain")
        || host
            .parse::<IpAddr>()
            .map(|address| address.is_loopback())
            .unwrap_or(false)
}

fn invalid_request(rpc: &Value, code: i64, message: &str) -> (u16, Value) {
    (
        400,
        rpc_error(
            rpc.get("id").cloned().unwrap_or(Value::Null),
            code,
            message,
            None,
        ),
    )
}

fn validate_rpc_transport(
    request: &Request,
    rpc: &Value,
    modern: bool,
) -> Result<(), (u16, Value)> {
    if rpc.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(invalid_request(rpc, -32600, "Invalid Request"));
    }
    let Some(method) = rpc.get("method").and_then(Value::as_str) else {
        return Err(invalid_request(rpc, -32600, "Invalid Request"));
    };
    if rpc.get("params").is_some_and(|params| !params.is_object()) {
        return Err(invalid_request(rpc, -32602, "Invalid params"));
    }
    if modern {
        let params = rpc.get("params").and_then(Value::as_object);
        let meta = params
            .and_then(|params| params.get("_meta"))
            .and_then(Value::as_object);
        let version = meta
            .and_then(|meta| meta.get("io.modelcontextprotocol/protocolVersion"))
            .and_then(Value::as_str);
        let client_capabilities = meta
            .and_then(|meta| meta.get("io.modelcontextprotocol/clientCapabilities"))
            .and_then(Value::as_object);
        if client_capabilities.is_none() {
            return Err(invalid_request(
                rpc,
                -32602,
                "Modern requests require clientCapabilities in params._meta",
            ));
        }
        if ["mcp-protocol-version", "mcp-method", "mcp-name"]
            .iter()
            .any(|name| header_count(request, name) > 1)
        {
            return Err(invalid_request(rpc, -32020, "Duplicate routing header"));
        }
        let protocol_header = header_value(request, "mcp-protocol-version");
        let method_header = header_value(request, "mcp-method");
        let header_name = header_value(request, "mcp-name");
        let params_name = match method {
            "tools/call" | "prompts/get" => params
                .and_then(|params| params.get("name"))
                .and_then(Value::as_str),
            "resources/read" => params
                .and_then(|params| params.get("uri"))
                .and_then(Value::as_str),
            _ => None,
        };
        if ["resources/read", "prompts/get"].contains(&method) && params_name.is_none() {
            return Err(invalid_request(
                rpc,
                -32602,
                "Resource reads require params.uri and prompt requests require params.name",
            ));
        }
        let subject_method = matches!(method, "tools/call" | "resources/read" | "prompts/get");
        let mismatch = protocol_header != version
            || method_header != Some(method)
            || (subject_method && header_name != params_name)
            || (!subject_method && header_name.is_some());
        if mismatch || version.is_none() {
            return Err(invalid_request(
                rpc,
                -32020,
                "Header mismatch or missing modern request metadata",
            ));
        }
        if version != Some(MODERN_PROTOCOL_VERSION) {
            return Err((
                400,
                rpc_error(
                    rpc.get("id").cloned().unwrap_or(Value::Null),
                    -32022,
                    "Unsupported protocol version",
                    Some(
                        json!({"supported": [MODERN_PROTOCOL_VERSION, LEGACY_PROTOCOL_VERSION], "requested": version}),
                    ),
                ),
            ));
        }
    } else if let Some(version) = header_value(request, "mcp-protocol-version") {
        if !LEGACY_PROTOCOL_VERSIONS.contains(&version) {
            return Err((
                400,
                rpc_error(
                    rpc.get("id").cloned().unwrap_or(Value::Null),
                    -32022,
                    "Unsupported protocol version",
                    Some(
                        json!({"supported": [MODERN_PROTOCOL_VERSION, LEGACY_PROTOCOL_VERSION], "requested": version}),
                    ),
                ),
            ));
        }
    }
    // A legacy request without MCP-Protocol-Version is treated as
    // ASSUMED_LEGACY_PROTOCOL_VERSION, as the specification recommends.
    let _ = ASSUMED_LEGACY_PROTOCOL_VERSION;
    Ok(())
}

fn request_is_modern(rpc: &Value, protocol_header: Option<&str>) -> bool {
    protocol_header == Some(MODERN_PROTOCOL_VERSION)
        || rpc
            .pointer("/params/_meta/io.modelcontextprotocol~1protocolVersion")
            .and_then(Value::as_str)
            == Some(MODERN_PROTOCOL_VERSION)
}

// ---------------------------------------------------------------------------
// JSON-RPC dispatch
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct RpcError {
    code: i64,
    message: String,
    data: Option<Value>,
}

fn dispatch_rpc(
    engine: &AppEngine,
    rpc: &Value,
    modern: bool,
) -> Result<(Value, bool), RpcError> {
    let method = rpc
        .get("method")
        .and_then(Value::as_str)
        .ok_or_else(|| rpc_err(-32600, "Invalid Request"))?;
    let params = rpc
        .get("params")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    match method {
        "initialize" if !modern => {
            let client_info = params.get("clientInfo").and_then(Value::as_object);
            if params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .is_none()
                || params
                    .get("capabilities")
                    .and_then(Value::as_object)
                    .is_none()
                || client_info
                    .and_then(|client| client.get("name"))
                    .and_then(Value::as_str)
                    .is_none()
                || client_info
                    .and_then(|client| client.get("version"))
                    .and_then(Value::as_str)
                    .is_none()
            {
                return Err(rpc_err(
                    -32602,
                    "initialize requires protocolVersion, capabilities, and clientInfo.name/version",
                ));
            }
            let requested = params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or(LEGACY_PROTOCOL_VERSION);
            let protocol_version = if LEGACY_PROTOCOL_VERSIONS.contains(&requested) {
                requested
            } else {
                LEGACY_PROTOCOL_VERSION
            };
            Ok((
                json!({
                    "protocolVersion": protocol_version,
                    "capabilities": {"tools": {"listChanged": false}},
                    "serverInfo": {"name": SERVER_NAME, "version": SERVER_VERSION},
                    "instructions": INSTRUCTIONS
                }),
                false,
            ))
        }
        "server/discover" if modern => Ok((
            modern_result(json!({
                "supportedVersions": [MODERN_PROTOCOL_VERSION, LEGACY_PROTOCOL_VERSION],
                "capabilities": {"tools": {"listChanged": false}},
                "instructions": INSTRUCTIONS,
                "ttlMs": 0,
                "cacheScope": "private"
            })),
            false,
        )),
        "ping" => Ok((
            if modern {
                modern_result(json!({}))
            } else {
                json!({})
            },
            false,
        )),
        "tools/list" => {
            let mut result = json!({"tools": tool_definitions()});
            if modern {
                result["ttlMs"] = json!(0);
                result["cacheScope"] = json!("private");
                result = modern_result(result);
            }
            Ok((result, false))
        }
        "tools/call" => {
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| rpc_err(-32602, "tools/call requires a string name"))?;
            if params
                .get("arguments")
                .is_some_and(|arguments| !arguments.is_object())
            {
                return Err(rpc_err(-32602, "tools/call arguments must be an object"));
            }
            let arguments = params
                .get("arguments")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            let executed = execute_tool(engine, name, &arguments);
            let mut result = match executed {
                Ok(mut value) => {
                    if name != "aztray_get_logs" {
                        strip_log_arrays(&mut value);
                    }
                    tool_result(value, false)
                }
                Err(error) => tool_result(json!({"error": error}), true),
            };
            if modern {
                result["resultType"] = json!("complete");
                stamp_server_info(&mut result);
            }
            let should_quit = name == "aztray_quit"
                && result
                    .get("structuredContent")
                    .and_then(|value| value.get("value"))
                    .and_then(|value| value.get("mode"))
                    .and_then(Value::as_str)
                    .is_some_and(|mode| mode == "leave_running" || mode == "stop_and_quit")
                && result.get("isError").and_then(Value::as_bool) != Some(true);
            Ok((result, should_quit))
        }
        _ => Err(rpc_err(-32601, "Method not found")),
    }
}

fn modern_result(mut result: Value) -> Value {
    result["resultType"] = json!("complete");
    stamp_server_info(&mut result);
    result
}

fn stamp_server_info(result: &mut Value) {
    if !result.is_object() {
        *result = json!({});
    }
    result["_meta"] = json!({
        "io.modelcontextprotocol/serverInfo": {
            "name": SERVER_NAME,
            "title": "AzTray",
            "version": SERVER_VERSION,
            "description": "Local tray controller for Azurite instances"
        }
    });
}

/// Hard cap on the serialized size of one tool result's JSON value. The value
/// is emitted twice (content text + structuredContent), so the worst case is
/// about twice this, well under Claude Code's ~25k-token MCP output cap.
const MAX_TOOL_RESULT_BYTES: usize = 32_000;
/// Default and maximum number of log lines `aztray_get_logs` returns.
const DEFAULT_LOG_LIMIT: usize = 200;
const MAX_LOG_LIMIT: usize = 1000;

/// Remove bulky log arrays (`logs`, `mergedLogs`) at any depth. Logs remain
/// available through `aztray_get_logs`.
fn strip_log_arrays(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("logs");
            map.remove("mergedLogs");
            map.values_mut().for_each(strip_log_arrays);
        }
        Value::Array(items) => items.iter_mut().for_each(strip_log_arrays),
        _ => {}
    }
}

fn truncate_chars(text: &str, max_bytes: usize) -> &str {
    let mut end = max_bytes.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Apply the hard size cap. Oversized arrays keep their newest entries;
/// oversized anything else becomes a truncation marker with a preview.
fn cap_value(value: Value) -> Value {
    let size = |value: &Value| serde_json::to_string(value).map(|s| s.len()).unwrap_or(0);
    let original = size(&value);
    if original <= MAX_TOOL_RESULT_BYTES {
        return value;
    }
    if let Value::Array(items) = &value {
        let total = items.len();
        let mut keep = total;
        let mut trimmed = items.clone();
        while keep > 0 && size(&Value::Array(trimmed.clone())) > MAX_TOOL_RESULT_BYTES {
            keep /= 2;
            trimmed = items[total - keep..].to_vec();
        }
        return json!({
            "truncated": true,
            "note": format!("Result exceeded {MAX_TOOL_RESULT_BYTES} bytes; returned the newest {keep} of {total} entries. Request a smaller limit or a specific serviceName."),
            "entries": trimmed
        });
    }
    let serialized = serde_json::to_string(&value).unwrap_or_default();
    json!({
        "truncated": true,
        "originalBytes": original,
        "note": format!("Result exceeded {MAX_TOOL_RESULT_BYTES} bytes and was truncated; use a narrower tool such as aztray_get_instance or aztray_connection_string."),
        "preview": truncate_chars(&serialized, MAX_TOOL_RESULT_BYTES / 2)
    })
}

fn tool_result(value: Value, is_error: bool) -> Value {
    let value = cap_value(value);
    let serialized = serde_json::to_string(&value).unwrap_or_else(|_| "{}".into());
    json!({
        "content": [{"type": "text", "text": serialized}],
        "structuredContent": {"value": value},
        "isError": is_error
    })
}

// ---------------------------------------------------------------------------
// Tool execution
// ---------------------------------------------------------------------------

fn optional_string(arguments: &Map<String, Value>, key: &str) -> Result<Option<String>, String> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(format!("{key} must be a string")),
    }
}

fn required_string(arguments: &Map<String, Value>, key: &str) -> Result<String, String> {
    match optional_string(arguments, key)? {
        Some(value) if !value.trim().is_empty() => Ok(value),
        _ => Err(format!("{key} is required and must be a non-empty string")),
    }
}

fn optional_bool(arguments: &Map<String, Value>, key: &str) -> Result<Option<bool>, String> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(format!("{key} must be a boolean")),
    }
}

fn instance_selector(arguments: &Map<String, Value>) -> Result<Option<String>, String> {
    match optional_string(arguments, "instance")? {
        Some(value) if value.trim().is_empty() => Err("instance must not be empty".into()),
        other => Ok(other),
    }
}

fn optional_service(arguments: &Map<String, Value>) -> Result<Option<ServiceName>, String> {
    match arguments.get("serviceName") {
        None | Some(Value::Null) => Ok(None),
        Some(value) => serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|_| "serviceName must be blob, queue, or table".to_string()),
    }
}

fn required_service(arguments: &Map<String, Value>) -> Result<ServiceName, String> {
    optional_service(arguments)?
        .ok_or_else(|| "serviceName must be blob, queue, or table".to_string())
}

fn parse_ports(
    arguments: &Map<String, Value>,
) -> Result<Option<BTreeMap<ServiceName, u16>>, String> {
    let Some(value) = arguments.get("ports") else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let object = value
        .as_object()
        .ok_or_else(|| "ports must be an object with blob, queue, and table".to_string())?;
    let mut ports = BTreeMap::new();
    for (key, service) in [
        ("blob", ServiceName::Blob),
        ("queue", ServiceName::Queue),
        ("table", ServiceName::Table),
    ] {
        let port = object
            .get(key)
            .and_then(Value::as_u64)
            .and_then(|port| u16::try_from(port).ok())
            .filter(|port| *port >= 1)
            .ok_or_else(|| format!("ports.{key} must be an integer from 1 to 65535"))?;
        ports.insert(service, port);
    }
    if let Some(unknown) = object
        .keys()
        .find(|key| !["blob", "queue", "table"].contains(&key.as_str()))
    {
        return Err(format!("ports.{unknown} is not a known service"));
    }
    Ok(Some(ports))
}

fn require_confirmed(arguments: &Map<String, Value>, action: &str) -> Result<(), String> {
    if arguments.get("confirmed").and_then(Value::as_bool) != Some(true) {
        return Err(format!("{action} requires confirmed=true"));
    }
    Ok(())
}

fn execute_tool(
    engine: &AppEngine,
    name: &str,
    arguments: &Map<String, Value>,
) -> Result<Value, String> {
    let instance = || instance_selector(arguments);
    match name {
        "aztray_snapshot" => as_json(engine.get_snapshot()),
        "aztray_mcp_status" => as_json(engine.mcp_status()),
        "aztray_list_instances" => as_json(engine.list_instances()),
        "aztray_get_instance" => {
            let selector = required_string(arguments, "instance")?;
            as_json(engine.get_instance(Some(&selector))?)
        }
        "aztray_create_instance" => {
            let request = CreateInstanceRequest {
                name: required_string(arguments, "name")?,
                id: optional_string(arguments, "id")?,
                host: optional_string(arguments, "host")?,
                ports: parse_ports(arguments)?,
                data_directory: optional_string(arguments, "dataDirectory")?,
                loose: optional_bool(arguments, "loose")?.unwrap_or(false),
                skip_api_version_check: optional_bool(arguments, "skipApiVersionCheck")?
                    .unwrap_or(false),
                start: optional_bool(arguments, "start")?.unwrap_or(true),
            };
            as_json(engine.create_instance(request)?)
        }
        "aztray_update_instance" => {
            let request = UpdateInstanceRequest {
                instance_id: required_string(arguments, "instance")?,
                name: optional_string(arguments, "name")?,
                host: optional_string(arguments, "host")?,
                ports: parse_ports(arguments)?,
                data_directory: optional_string(arguments, "dataDirectory")?,
                loose: optional_bool(arguments, "loose")?,
                skip_api_version_check: optional_bool(arguments, "skipApiVersionCheck")?,
            };
            as_json(engine.update_instance(request)?)
        }
        "aztray_delete_instance" => {
            let selector = required_string(arguments, "instance")?;
            require_confirmed(arguments, "delete_instance")?;
            as_json(engine.delete_instance(&selector)?)
        }
        "aztray_start_instance" => as_json(engine.start_instance(instance()?.as_deref())?),
        "aztray_stop_instance" => as_json(engine.stop_instance(instance()?.as_deref())?),
        "aztray_restart_instance" => as_json(engine.restart_instance(instance()?.as_deref())?),
        "aztray_start_service" => as_json(
            engine.start_service(instance()?.as_deref(), required_service(arguments)?)?,
        ),
        "aztray_stop_service" => as_json(
            engine.stop_service(instance()?.as_deref(), required_service(arguments)?)?,
        ),
        "aztray_restart_service" => as_json(
            engine.restart_service(instance()?.as_deref(), required_service(arguments)?)?,
        ),
        "aztray_start_all" => as_json(engine.start_all()?),
        "aztray_stop_all" => as_json(engine.stop_all()?),
        "aztray_restart_all" => as_json(engine.restart_all()?),
        "aztray_connection_string" => {
            let selector = instance()?;
            let instance_id = engine.resolve_instance_id(selector.as_deref())?;
            match optional_service(arguments)? {
                Some(service) => {
                    let connection_string =
                        engine.connection_string(selector.as_deref(), Some(service.clone()))?;
                    Ok(json!({
                        "instance": instance_id,
                        "serviceName": service,
                        "connectionString": connection_string
                    }))
                }
                None => {
                    let mut value = as_json(engine.connection_info(selector.as_deref())?)?;
                    value["instance"] = json!(instance_id);
                    Ok(value)
                }
            }
        }
        "aztray_get_config" => as_json(engine.get_snapshot().config),
        "aztray_set_settings" => {
            let current = engine.get_snapshot().config;
            let path_setting = |key: &str, existing: Option<String>| match arguments.get(key) {
                None => Ok(existing),
                Some(Value::Null) => Ok(None),
                Some(Value::String(value)) => Ok(Some(value.clone())),
                Some(_) => Err(format!("{key} must be a string or null")),
            };
            let settings = GlobalSettings {
                executable_path: path_setting("executablePath", current.executable_path)?,
                node_path: path_setting("nodePath", current.node_path)?,
                // MCP enabled/port are dashboard-only so a client cannot cut
                // off its own connection.
                mcp: engine.mcp_config(),
            };
            as_json(engine.set_settings(settings)?)
        }
        "aztray_check_engine" => as_json(engine.check_engine()),
        "aztray_identify_port_owner" => as_json(
            engine.identify_port_owner(instance()?.as_deref(), required_service(arguments)?),
        ),
        "aztray_free_port" => {
            if arguments.get("confirmed").and_then(Value::as_bool) != Some(true) {
                return Err("free_port requires confirmed=true after the process identity has been reviewed".into());
            }
            if !arguments.contains_key("startedAt") {
                return Err("free_port requires the observed startedAt value; pass null only when the owner reports null".into());
            }
            let pid = arguments
                .get("pid")
                .and_then(Value::as_u64)
                .and_then(|pid| u32::try_from(pid).ok())
                .filter(|pid| *pid > 0)
                .ok_or_else(|| "pid must be a positive process ID".to_string())?;
            let started_at = match arguments.get("startedAt") {
                Some(Value::Null) | None => None,
                Some(Value::String(value)) => Some(value.clone()),
                Some(_) => return Err("startedAt must be a string or null".into()),
            };
            as_json(engine.free_port(PortOwnerExpectation {
                instance_id: instance()?,
                service_name: required_service(arguments)?,
                pid,
                started_at,
            })?)
        }
        "aztray_get_logs" => {
            let limit = match arguments.get("limit") {
                None | Some(Value::Null) => Some(DEFAULT_LOG_LIMIT),
                Some(value) => Some(
                    value
                        .as_u64()
                        .and_then(|limit| usize::try_from(limit).ok())
                        .ok_or_else(|| "limit must be a non-negative integer".to_string())?
                        .min(MAX_LOG_LIMIT),
                ),
            };
            as_json(engine.get_logs(LogsQuery {
                instance_id: instance()?,
                service_name: optional_service(arguments)?,
                limit,
            }))
        }
        "aztray_save_logs" => as_json(engine.save_logs(SaveLogsArgs {
            instance_id: instance()?,
            service_name: optional_service(arguments)?,
            path: optional_string(arguments, "path")?,
        })?),
        "aztray_clear_logs" => as_json(
            engine.clear_logs(instance()?.as_deref(), optional_service(arguments)?),
        ),
        "aztray_get_app_log" => {
            let limit = match arguments.get("limit") {
                None | Some(Value::Null) => 200,
                Some(value) => value
                    .as_u64()
                    .and_then(|limit| usize::try_from(limit).ok())
                    .filter(|limit| (1..=2000).contains(limit))
                    .ok_or_else(|| "limit must be an integer from 1 to 2000".to_string())?,
            };
            let path = engine.get_snapshot().app.log_path;
            Ok(json!({"path": path, "lines": engine.app_log_tail(limit)}))
        }
        "aztray_quit" => {
            let mode = match arguments.get("mode").and_then(Value::as_str) {
                Some("stop_and_quit") => QuitMode::StopAndQuit,
                Some("leave_running") => QuitMode::LeaveRunning,
                Some("cancel") => QuitMode::Cancel,
                _ => return Err("mode must be stop_and_quit, leave_running, or cancel".into()),
            };
            as_json(engine.quit(mode)?)
        }
        _ => Err(format!("Unknown AzTray tool: {name}")),
    }
}

fn as_json<T: Serialize>(value: T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|_| "Could not encode AzTray result".to_string())
}

// ---------------------------------------------------------------------------
// Tool catalog
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Hint {
    /// readOnly
    Read,
    /// mutating, non-destructive (idempotent flag)
    Write(bool),
    /// destructive (idempotent flag)
    Destructive(bool),
}

fn tool(name: &str, description: &str, properties: Value, required: &[&str], hint: Hint) -> Value {
    let (read_only, destructive, idempotent) = match hint {
        Hint::Read => (true, false, true),
        Hint::Write(idempotent) => (false, false, idempotent),
        Hint::Destructive(idempotent) => (false, true, idempotent),
    };
    json!({
        "name": name,
        "description": description,
        "inputSchema": {
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false
        },
        "annotations": {
            "readOnlyHint": read_only,
            "destructiveHint": destructive,
            "idempotentHint": idempotent,
            "openWorldHint": false
        }
    })
}

fn instance_prop() -> Value {
    json!({"type": "string", "minLength": 1, "description": "Instance id or name (case-insensitive). Omit for the default instance (or the only instance)."})
}

fn service_prop() -> Value {
    json!({"type": "string", "enum": ["blob", "queue", "table"]})
}

fn ports_prop() -> Value {
    json!({
        "type": "object",
        "description": "Host ports for the three services. All three are required when ports is given.",
        "properties": {
            "blob": {"type": "integer", "minimum": 1, "maximum": 65535},
            "queue": {"type": "integer", "minimum": 1, "maximum": 65535},
            "table": {"type": "integer", "minimum": 1, "maximum": 65535}
        },
        "required": ["blob", "queue", "table"],
        "additionalProperties": false
    })
}

fn tool_definitions() -> Vec<Value> {
    let instance_only = || json!({"instance": instance_prop()});
    let instance_service = || json!({"instance": instance_prop(), "serviceName": service_prop()});
    let confirmed = |description: &str| {
        json!({"type": "boolean", "const": true, "description": description})
    };
    vec![
        tool(
            "aztray_snapshot",
            "Read the full AzTray state: app version, configuration, engine status, MCP status, and every Azurite instance with service states, process identities, and connection strings. Log lines are omitted; use aztray_get_logs.",
            json!({}), &[], Hint::Read,
        ),
        tool(
            "aztray_mcp_status",
            "Read this MCP endpoint's status: {enabled, running, url, port, requestedPort, fallbackUsed, error, startedAt, lastEvent, attempts}. Use url to register the server when fallbackUsed is true.",
            json!({}), &[], Hint::Read,
        ),
        tool(
            "aztray_list_instances",
            "List all Azurite instances with their state, ports, data directory, and connection strings.",
            json!({}), &[], Hint::Read,
        ),
        tool(
            "aztray_get_instance",
            "Read one Azurite instance: config, per-service state, and connection info. Log lines are omitted; use aztray_get_logs.",
            json!({"instance": instance_prop()}), &["instance"], Hint::Read,
        ),
        tool(
            "aztray_create_instance",
            "Create a new isolated Azurite instance (Blob + Queue + Table) with its own ports and data directory, and by default start it. Ports and data directory are auto-assigned when omitted. The response includes connection.connectionString ready to use. Use aztray_stop_instance then aztray_delete_instance when finished.",
            json!({
                "name": {"type": "string", "minLength": 1, "maxLength": 48, "description": "Display name, unique case-insensitively."},
                "id": {"type": "string", "pattern": "^[a-z0-9][a-z0-9-]{0,31}$", "description": "Optional immutable slug; derived from name when omitted."},
                "host": {"type": "string", "minLength": 1, "description": "Bind host. Default 127.0.0.1."},
                "ports": ports_prop(),
                "dataDirectory": {"type": "string", "minLength": 1, "description": "Azurite data directory. Default is under %LOCALAPPDATA%\\AzTray\\instances\\<id>\\data."},
                "loose": {"type": "boolean", "description": "Pass --loose to Azurite."},
                "skipApiVersionCheck": {"type": "boolean", "description": "Pass --skipApiVersionCheck to Azurite."},
                "start": {"type": "boolean", "default": true, "description": "Start the instance after creating it. Default true."}
            }),
            &["name"], Hint::Write(false),
        ),
        tool(
            "aztray_update_instance",
            "Change an instance's name, host, ports, data directory, or Azurite flags. Everything except name requires the instance to be fully stopped. Existing Azurite data is not touched.",
            json!({
                "instance": instance_prop(),
                "name": {"type": "string", "minLength": 1, "maxLength": 48},
                "host": {"type": "string", "minLength": 1},
                "ports": ports_prop(),
                "dataDirectory": {"type": "string", "minLength": 1},
                "loose": {"type": "boolean"},
                "skipApiVersionCheck": {"type": "boolean"}
            }),
            &["instance"], Hint::Write(true),
        ),
        tool(
            "aztray_delete_instance",
            "Remove an instance from AzTray. The instance must be stopped and cannot be the last one. Stored Azurite data on disk is never deleted.",
            json!({
                "instance": instance_prop(),
                "confirmed": confirmed("Set true only after the user approved removing this instance.")
            }),
            &["instance", "confirmed"], Hint::Destructive(false),
        ),
        tool(
            "aztray_start_instance",
            "Start the Blob, Queue, and Table services of one instance.",
            instance_only(), &[], Hint::Write(true),
        ),
        tool(
            "aztray_stop_instance",
            "Stop the Azurite service processes of one instance owned by AzTray. This terminates those processes.",
            instance_only(), &[], Hint::Destructive(true),
        ),
        tool(
            "aztray_restart_instance",
            "Stop and start all services of one instance. This terminates and relaunches the processes.",
            instance_only(), &[], Hint::Destructive(false),
        ),
        tool(
            "aztray_start_service",
            "Start one Azurite service of an instance using its current configuration.",
            instance_service(), &["serviceName"], Hint::Write(true),
        ),
        tool(
            "aztray_stop_service",
            "Stop one Azurite service of an instance. This terminates that service process.",
            instance_service(), &["serviceName"], Hint::Destructive(true),
        ),
        tool(
            "aztray_restart_service",
            "Stop and start one Azurite service of an instance. This terminates and relaunches that process.",
            instance_service(), &["serviceName"], Hint::Destructive(false),
        ),
        tool(
            "aztray_start_all",
            "Start every Azurite instance.",
            json!({}), &[], Hint::Write(true),
        ),
        tool(
            "aztray_stop_all",
            "Stop every Azurite instance's processes owned by AzTray.",
            json!({}), &[], Hint::Destructive(true),
        ),
        tool(
            "aztray_restart_all",
            "Stop and start every Azurite instance.",
            json!({}), &[], Hint::Destructive(false),
        ),
        tool(
            "aztray_connection_string",
            "Read connection info for an instance. With serviceName, returns that service's connection string; without it, returns the combined connection string plus per-service strings and endpoints.",
            instance_service(), &[], Hint::Read,
        ),
        tool(
            "aztray_get_config",
            "Read AzTray configuration: global engine paths, MCP settings, and every instance's host, ports, and data directory.",
            json!({}), &[], Hint::Read,
        ),
        tool(
            "aztray_set_settings",
            "Set the global Node.js and Azurite executable paths (null clears an override). Services of all instances must be stopped. The MCP port and enabled flag can only be changed in the AzTray dashboard.",
            json!({
                "executablePath": {"type": ["string", "null"], "description": "Azurite executable override, or null to clear."},
                "nodePath": {"type": ["string", "null"], "description": "Node.js executable override, or null to clear."}
            }),
            &[], Hint::Write(true),
        ),
        tool(
            "aztray_check_engine",
            "Check whether the configured Node.js and Azurite engine are available.",
            json!({}), &[], Hint::Read,
        ),
        tool(
            "aztray_identify_port_owner",
            "Read the process identity owning a service port of an instance. Use these observed PID and startedAt values when requesting a port release.",
            instance_service(), &["serviceName"], Hint::Read,
        ),
        tool(
            "aztray_free_port",
            "Terminate the current process owning a configured service port. First inspect aztray_identify_port_owner and pass its exact pid and startedAt values, then set confirmed=true after the user explicitly approves releasing that process. The engine rechecks the process identity before termination and refuses if it changed.",
            json!({
                "instance": instance_prop(),
                "serviceName": service_prop(),
                "pid": {"type": "integer", "minimum": 1, "maximum": 4294967295u64},
                "startedAt": {"type": ["string", "null"], "description": "Exact startedAt value from aztray_identify_port_owner, including null when the owner reports null."},
                "confirmed": confirmed("Set true only after the user has approved terminating the observed port owner.")
            }),
            &["serviceName", "pid", "startedAt", "confirmed"], Hint::Destructive(false),
        ),
        tool(
            "aztray_get_logs",
            "Read the newest Azurite log lines for one service of an instance, or the instance's merged log stream. This is the only tool that returns log lines. Defaults to the newest 200 lines; limit is capped at 1000 and output is size-capped.",
            json!({
                "instance": instance_prop(),
                "serviceName": service_prop(),
                "limit": {"type": "integer", "minimum": 0, "maximum": 1000, "default": 200}
            }),
            &[], Hint::Read,
        ),
        tool(
            "aztray_save_logs",
            "Write an instance's AzTray logs to a text file. When path is omitted AzTray chooses its default file; an existing supplied path may be overwritten.",
            json!({
                "instance": instance_prop(),
                "serviceName": service_prop(),
                "path": {"type": "string", "minLength": 1}
            }),
            &[], Hint::Destructive(false),
        ),
        tool(
            "aztray_clear_logs",
            "Clear the in-memory logs of an instance (one service or all). Does not delete Azurite data files.",
            instance_service(), &[], Hint::Destructive(true),
        ),
        tool(
            "aztray_get_app_log",
            "Read the tail of AzTray's own diagnostic log (aztray.log), including MCP bind/fallback/retry events. Returns {path, lines}.",
            json!({"limit": {"type": "integer", "minimum": 1, "maximum": 2000, "default": 200}}),
            &[], Hint::Read,
        ),
        tool(
            "aztray_quit",
            "Close the AzTray controller. stop_and_quit stops AzTray-owned Azurite services of every instance before closing; leave_running closes AzTray while services continue; cancel keeps AzTray open. The MCP response is sent before the application closes.",
            json!({"mode": {"type": "string", "enum": ["stop_and_quit", "leave_running", "cancel"]}}),
            &["mode"], Hint::Destructive(false),
        ),
    ]
}

// ---------------------------------------------------------------------------
// HTTP / JSON-RPC helpers
// ---------------------------------------------------------------------------

fn rpc_result(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn rpc_err(code: i64, message: &str) -> RpcError {
    RpcError {
        code,
        message: message.to_string(),
        data: None,
    }
}

fn rpc_error(id: Value, code: i64, message: &str, data: Option<Value>) -> Value {
    let mut error = json!({"code": code, "message": message});
    if let Some(data) = data {
        error["data"] = data;
    }
    json!({"jsonrpc": "2.0", "id": id, "error": error})
}

fn header_value<'a>(request: &'a Request, name: &str) -> Option<&'a str> {
    request.headers().iter().find_map(|header| {
        header
            .field
            .as_str()
            .as_str()
            .eq_ignore_ascii_case(name)
            .then(|| header.value.as_str())
    })
}

fn header_count(request: &Request, name: &str) -> usize {
    request
        .headers()
        .iter()
        .filter(|header| header.field.as_str().as_str().eq_ignore_ascii_case(name))
        .count()
}

/// Responses are always `application/json`, so a client that accepts JSON (or
/// anything) is served. Clients that list only `text/event-stream` are refused.
fn accepts_mcp_json(accept: Option<&str>) -> bool {
    let Some(accept) = accept else { return true };
    accept
        .split(',')
        .map(|item| item.trim().split(';').next().unwrap_or_default().trim())
        .any(|item| {
            item.eq_ignore_ascii_case("application/json")
                || item == "*/*"
                || item.eq_ignore_ascii_case("application/*")
        })
}

fn is_json_content_type(content_type: Option<&str>) -> bool {
    content_type
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .is_some_and(|value| value.eq_ignore_ascii_case("application/json"))
}

fn json_response(
    status: u16,
    body: Value,
    origin: Option<&str>,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = serde_json::to_vec(&body).unwrap_or_else(|_| b"{}".to_vec());
    let response = Response::from_data(body)
        .with_status_code(StatusCode(status))
        .with_header(header("Content-Type", "application/json; charset=utf-8"));
    with_cors(response, origin)
}

fn http_error(
    status: u16,
    message: &str,
    origin: Option<&str>,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let mut response =
        Response::from_string(message.to_string()).with_status_code(StatusCode(status));
    response.add_header(header("Content-Type", "text/plain; charset=utf-8"));
    with_cors(response, origin)
}

fn with_cors<R: Read>(mut response: Response<R>, origin: Option<&str>) -> Response<R> {
    response.add_header(header("X-Content-Type-Options", "nosniff"));
    response.add_header(header("Cache-Control", "no-store"));
    if let Some(origin) = origin {
        response.add_header(header("Access-Control-Allow-Origin", origin));
        response.add_header(header("Access-Control-Expose-Headers", "Mcp-Session-Id"));
        response.add_header(header("Vary", "Origin"));
    }
    response
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("static HTTP header is valid")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{TcpListener, TcpStream};

    fn free_port() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        port
    }

    fn test_config(port: u16, fallback: bool) -> McpConfig {
        McpConfig {
            enabled: true,
            port,
            port_fallback: fallback,
        }
    }

    fn wait_running(engine: &AppEngine) -> McpStatus {
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        loop {
            let status = engine.mcp_status();
            if status.running || std::time::Instant::now() > deadline {
                return status;
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    fn start_test_server(port: u16, fallback: bool) -> (McpServer, AppEngine, McpStatus) {
        let engine = AppEngine::new();
        let server = McpServer::start_with_config(
            engine.clone(),
            Arc::new(|| {}),
            test_config(port, fallback),
        );
        let status = wait_running(&engine);
        (server, engine, status)
    }

    fn raw_post(port: u16, body: Value, extra_headers: &str) -> (u16, Value, String) {
        let body = serde_json::to_vec(&body).unwrap();
        let headers = format!(
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept: application/json, text/event-stream\r\nContent-Type: application/json\r\n{extra_headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        raw_exchange(port, &headers, &body)
    }

    fn raw_exchange(port: u16, headers: &str, body: &[u8]) -> (u16, Value, String) {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.write_all(headers.as_bytes()).unwrap();
        stream.write_all(body).unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).unwrap();
        let response = String::from_utf8(response).unwrap();
        let (head, body) = response.split_once("\r\n\r\n").unwrap();
        let status = head
            .lines()
            .next()
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap()
            .parse::<u16>()
            .unwrap();
        let body = serde_json::from_str(body).unwrap_or(Value::Null);
        (status, body, head.to_string())
    }

    fn modern_meta() -> Value {
        json!({
            "io.modelcontextprotocol/protocolVersion": MODERN_PROTOCOL_VERSION,
            "io.modelcontextprotocol/clientCapabilities": {}
        })
    }

    fn initialize_request() -> Value {
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "protocolVersion": LEGACY_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": "test-client", "version": "1.0"}
            }
        })
    }

    // ---- JSON-RPC dispatch (no network, no Azurite) ----

    #[test]
    fn dispatch_initialize_negotiates_version_and_reports_server_info() {
        let engine = AppEngine::new();
        let (result, quit) = dispatch_rpc(&engine, &initialize_request(), false).unwrap();
        assert!(!quit);
        assert_eq!(result["protocolVersion"], LEGACY_PROTOCOL_VERSION);
        assert_eq!(result["serverInfo"]["name"], "aztray");
        assert_eq!(result["serverInfo"]["version"], env!("CARGO_PKG_VERSION"));
        assert!(result["capabilities"]["tools"].is_object());

        let mut older = initialize_request();
        older["params"]["protocolVersion"] = json!("2025-06-18");
        let (result, _) = dispatch_rpc(&engine, &older, false).unwrap();
        assert_eq!(result["protocolVersion"], "2025-06-18");

        let mut unknown = initialize_request();
        unknown["params"]["protocolVersion"] = json!("1999-01-01");
        let (result, _) = dispatch_rpc(&engine, &unknown, false).unwrap();
        assert_eq!(result["protocolVersion"], LEGACY_PROTOCOL_VERSION);
    }

    #[test]
    fn dispatch_initialize_requires_client_info() {
        let engine = AppEngine::new();
        let rpc = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}});
        assert_eq!(dispatch_rpc(&engine, &rpc, false).unwrap_err().code, -32602);
    }

    #[test]
    fn dispatch_tools_list_includes_instance_tools() {
        let engine = AppEngine::new();
        let rpc = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"});
        let (result, _) = dispatch_rpc(&engine, &rpc, false).unwrap();
        let names: Vec<&str> = result["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect();
        for expected in [
            "aztray_list_instances",
            "aztray_get_instance",
            "aztray_create_instance",
            "aztray_update_instance",
            "aztray_delete_instance",
            "aztray_start_instance",
            "aztray_stop_instance",
            "aztray_restart_instance",
            "aztray_connection_string",
            "aztray_get_app_log",
            "aztray_set_settings",
            "aztray_mcp_status",
        ] {
            assert!(names.contains(&expected), "missing tool {expected}");
        }
        assert!(!names.contains(&"aztray_set_config"));
    }

    fn fake_snapshot_with_logs(lines: usize) -> Value {
        let entry = |i: usize| {
            json!({"id": format!("e{i}"), "sequence": i, "instanceId": "default",
                   "service": "blob", "message": "x".repeat(120)})
        };
        let many: Vec<Value> = (0..lines).map(entry).collect();
        let instance = json!({
            "config": {"id": "default"},
            "connection": {"connectionString": "UseDevelopmentStorage=true"},
            "logs": {"blob": many.clone(), "queue": many.clone(), "table": many.clone()},
            "mergedLogs": many.iter().cloned().cycle().take(lines * 3).collect::<Vec<_>>()
        });
        json!({"instances": [instance.clone(), instance], "mcp": {"url": "http://127.0.0.1:1/mcp"}})
    }

    #[test]
    fn snapshot_style_outputs_omit_logs_and_stay_bounded() {
        let mut value = fake_snapshot_with_logs(500);
        assert!(serde_json::to_string(&value).unwrap().len() > 500_000);
        strip_log_arrays(&mut value);
        let result = tool_result(value, false);
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(!text.contains("mergedLogs") && !text.contains("\"logs\""));
        assert!(text.len() < MAX_TOOL_RESULT_BYTES);
        assert!(!text.contains('\n'), "text must be compact JSON");
        assert!(text.contains("UseDevelopmentStorage=true"));
        let structured = serde_json::to_string(&result["structuredContent"]).unwrap();
        assert!(!structured.contains("mergedLogs"));
        assert!(structured.len() < MAX_TOOL_RESULT_BYTES + 100);
    }

    #[test]
    fn oversized_results_are_truncated_with_a_note() {
        let entries: Vec<Value> = (0..5000)
            .map(|i| json!({"sequence": i, "message": "y".repeat(100)}))
            .collect();
        let result = tool_result(Value::Array(entries), false);
        let value = &result["structuredContent"]["value"];
        assert_eq!(value["truncated"], true);
        let kept = value["entries"].as_array().unwrap();
        assert_eq!(kept.last().unwrap()["sequence"], 4999);
        assert!(result["content"][0]["text"].as_str().unwrap().len() <= MAX_TOOL_RESULT_BYTES + 1000);

        let huge = json!({"blob": "z".repeat(200_000)});
        let result = tool_result(huge, false);
        assert_eq!(result["structuredContent"]["value"]["truncated"], true);
        assert!(result["content"][0]["text"].as_str().unwrap().len() < MAX_TOOL_RESULT_BYTES);
    }

    #[test]
    fn dispatched_snapshot_tools_contain_no_log_arrays() {
        let engine = AppEngine::new();
        for name in ["aztray_snapshot", "aztray_list_instances", "aztray_clear_logs"] {
            let call = json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": {"name": name, "arguments": {}}
            });
            let (result, _) = dispatch_rpc(&engine, &call, false).unwrap();
            let text = result["content"][0]["text"].as_str().unwrap();
            assert!(!text.contains("mergedLogs"), "{name}");
            assert!(!text.contains("\"logs\""), "{name}");
            assert!(text.len() < MAX_TOOL_RESULT_BYTES, "{name}");
        }
    }

    #[test]
    fn dispatch_unknown_method_is_method_not_found() {
        let engine = AppEngine::new();
        let rpc = json!({"jsonrpc": "2.0", "id": 3, "method": "does/not/exist"});
        assert_eq!(dispatch_rpc(&engine, &rpc, false).unwrap_err().code, -32601);
    }

    #[test]
    fn dispatch_ping_and_unknown_tool() {
        let engine = AppEngine::new();
        let ping = json!({"jsonrpc": "2.0", "id": 1, "method": "ping"});
        assert_eq!(dispatch_rpc(&engine, &ping, false).unwrap().0, json!({}));
        let call = json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": {"name": "aztray_nope", "arguments": {}}
        });
        let (result, quit) = dispatch_rpc(&engine, &call, false).unwrap();
        assert_eq!(result["isError"], true);
        assert!(!quit);
    }

    #[test]
    fn tool_catalog_is_deterministic_valid_and_marks_mutation() {
        let tools = tool_definitions();
        assert_eq!(
            tools.first().and_then(|tool| tool["name"].as_str()),
            Some("aztray_snapshot")
        );
        let mut seen = std::collections::HashSet::new();
        for tool in &tools {
            let name = tool["name"].as_str().unwrap();
            assert!(seen.insert(name.to_string()), "duplicate tool {name}");
            assert_eq!(tool["inputSchema"]["additionalProperties"], false, "{name}");
            let properties = tool["inputSchema"]["properties"].as_object().unwrap();
            for required in tool["inputSchema"]["required"].as_array().unwrap() {
                assert!(
                    properties.contains_key(required.as_str().unwrap()),
                    "{name} requires an undeclared property"
                );
            }
            if tool["annotations"]["readOnlyHint"] == true {
                assert_eq!(tool["annotations"]["destructiveHint"], false, "{name}");
            }
        }
        let free_port = tools
            .iter()
            .find(|tool| tool["name"] == "aztray_free_port")
            .unwrap();
        assert_eq!(free_port["annotations"]["destructiveHint"], true);
        assert_eq!(
            free_port["inputSchema"]["required"],
            json!(["serviceName", "pid", "startedAt", "confirmed"])
        );
        let delete = tools
            .iter()
            .find(|tool| tool["name"] == "aztray_delete_instance")
            .unwrap();
        assert_eq!(delete["annotations"]["destructiveHint"], true);
    }

    #[test]
    fn free_port_and_delete_require_confirmation() {
        let engine = AppEngine::new();
        let mut arguments = Map::new();
        arguments.insert("serviceName".into(), json!("blob"));
        arguments.insert("pid".into(), json!(123));
        arguments.insert("startedAt".into(), Value::Null);
        arguments.insert("confirmed".into(), json!(false));
        let error = execute_tool(&engine, "aztray_free_port", &arguments).unwrap_err();
        assert!(error.contains("confirmed=true"));

        let mut arguments = Map::new();
        arguments.insert("instance".into(), json!("default"));
        let error = execute_tool(&engine, "aztray_delete_instance", &arguments).unwrap_err();
        assert!(error.contains("confirmed=true"));
    }

    #[test]
    fn port_and_argument_parsing() {
        let mut arguments = Map::new();
        arguments.insert("ports".into(), json!({"blob": 1, "queue": 2, "table": 3}));
        let ports = parse_ports(&arguments).unwrap().unwrap();
        assert_eq!(ports[&ServiceName::Table], 3);

        arguments.insert("ports".into(), json!({"blob": 1, "queue": 2}));
        assert!(parse_ports(&arguments).is_err());
        arguments.insert("ports".into(), json!({"blob": 0, "queue": 2, "table": 3}));
        assert!(parse_ports(&arguments).is_err());
        arguments.insert("ports".into(), json!({"blob": 70000, "queue": 2, "table": 3}));
        assert!(parse_ports(&arguments).is_err());

        let mut arguments = Map::new();
        arguments.insert("instance".into(), json!(""));
        assert!(instance_selector(&arguments).is_err());
        arguments.insert("instance".into(), json!(5));
        assert!(instance_selector(&arguments).is_err());
        assert_eq!(instance_selector(&Map::new()).unwrap(), None);
    }

    #[test]
    fn candidate_ports_and_backoff() {
        assert_eq!(candidate_ports(&test_config(47551, false)), vec![47551]);
        let range = candidate_ports(&test_config(47551, true));
        assert_eq!(range.first(), Some(&47551));
        assert_eq!(range.last(), Some(&47560));
        assert_eq!(candidate_ports(&test_config(65535, true)), vec![65535]);
        assert_eq!(backoff_delay(1), Duration::from_secs(15));
        assert_eq!(backoff_delay(2), Duration::from_secs(30));
        assert_eq!(backoff_delay(3), Duration::from_secs(60));
        assert_eq!(backoff_delay(30), Duration::from_secs(60));
    }

    #[test]
    fn origin_guard_accepts_local_webview_and_rejects_remote_hosts() {
        assert!(is_loopback_origin("http://localhost:3000"));
        assert!(is_loopback_origin("http://127.0.0.1:3000"));
        assert!(is_loopback_origin("http://[::1]:3000"));
        assert!(is_loopback_origin("tauri://localhost"));
        assert!(!is_loopback_origin("http://localhost.attacker.example"));
        assert!(!is_loopback_origin("http://8.8.8.8"));
        assert!(!is_loopback_origin("null"));
    }

    #[test]
    fn accept_header_is_lenient_for_json_clients() {
        assert!(accepts_mcp_json(None));
        assert!(accepts_mcp_json(Some("application/json, text/event-stream")));
        assert!(accepts_mcp_json(Some("*/*")));
        assert!(!accepts_mcp_json(Some("text/event-stream")));
    }

    // ---- loopback transport + supervisor ----

    #[test]
    fn streamable_http_serves_modern_and_legacy_clients() {
        let port = free_port();
        let (server, engine, status) = start_test_server(port, false);
        assert!(status.running, "status: {status:?}");
        assert_eq!(status.port, port);
        assert_eq!(status.url, format!("http://127.0.0.1:{port}/mcp"));
        assert!(!status.fallback_used);
        assert!(status.started_at.is_some());

        let discover = json!({
            "jsonrpc": "2.0", "id": 1, "method": "server/discover",
            "params": {"_meta": modern_meta()}
        });
        let (code, result, _) = raw_post(
            port,
            discover,
            &format!("MCP-Protocol-Version: {MODERN_PROTOCOL_VERSION}\r\nMcp-Method: server/discover\r\n"),
        );
        assert_eq!(code, 200);
        assert_eq!(result["result"]["resultType"], "complete");

        let list = json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/list",
            "params": {"_meta": modern_meta()}
        });
        let (code, result, _) = raw_post(
            port,
            list,
            &format!("MCP-Protocol-Version: {MODERN_PROTOCOL_VERSION}\r\nMcp-Method: tools/list\r\n"),
        );
        assert_eq!(code, 200);
        assert!(result["result"]["tools"].as_array().unwrap().len() >= 25);

        let mismatch = json!({
            "jsonrpc": "2.0", "id": 4, "method": "tools/call",
            "params": {"name": "aztray_snapshot", "arguments": {}, "_meta": modern_meta()}
        });
        let (code, result, _) = raw_post(
            port,
            mismatch,
            &format!("MCP-Protocol-Version: {MODERN_PROTOCOL_VERSION}\r\nMcp-Method: tools/call\r\nMcp-Name: different_tool\r\n"),
        );
        assert_eq!(code, 400);
        assert_eq!(result["error"]["code"], -32020);

        let (code, result, head) = raw_post(port, initialize_request(), "");
        assert_eq!(code, 200);
        assert_eq!(result["result"]["protocolVersion"], LEGACY_PROTOCOL_VERSION);
        assert!(head.to_ascii_lowercase().contains("mcp-session-id:"));
        assert!(head.to_ascii_lowercase().contains("content-type: application/json"));

        let initialized = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        let (code, _, _) = raw_post(
            port,
            initialized,
            &format!("MCP-Protocol-Version: {LEGACY_PROTOCOL_VERSION}\r\n"),
        );
        assert_eq!(code, 202);

        // Legacy request without MCP-Protocol-Version is accepted.
        let legacy_list = json!({"jsonrpc": "2.0", "id": 6, "method": "tools/list"});
        let (code, result, _) = raw_post(port, legacy_list, "");
        assert_eq!(code, 200);
        assert!(result["result"]["tools"].is_array());
        assert!(result["result"].get("resultType").is_none());

        let (code, _, head) = raw_exchange(
            port,
            &format!("GET /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"),
            b"",
        );
        assert_eq!(code, 405);
        assert!(head.to_ascii_lowercase().contains("allow:"));

        let status_call = json!({
            "jsonrpc": "2.0", "id": 30, "method": "tools/call",
            "params": {"name": "aztray_mcp_status", "arguments": {}}
        });
        let (code, result, _) = raw_post(port, status_call, "");
        assert_eq!(code, 200);
        assert_eq!(
            result["result"]["structuredContent"]["value"]["running"],
            true
        );
        assert_eq!(
            result["result"]["structuredContent"]["value"]["port"],
            port
        );

        drop(server);
        assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
        drop(engine);
    }

    #[test]
    fn supervisor_falls_back_when_requested_port_is_busy() {
        let holder = TcpListener::bind("127.0.0.1:0").unwrap();
        let busy = holder.local_addr().unwrap().port();
        let (server, _engine, status) = start_test_server(busy, true);
        assert!(status.running, "status: {status:?}");
        assert!(status.fallback_used);
        assert_ne!(status.port, busy);
        assert_eq!(status.requested_port, busy);
        assert!(status.last_event.unwrap().contains("unavailable"));
        drop(server);
        drop(holder);
    }

    #[test]
    fn supervisor_reports_failure_without_fallback_and_stops_promptly() {
        let holder = TcpListener::bind("127.0.0.1:0").unwrap();
        let busy = holder.local_addr().unwrap().port();
        let engine = AppEngine::new();
        let server = McpServer::start_with_config(
            engine.clone(),
            Arc::new(|| {}),
            test_config(busy, false),
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while engine.mcp_status().error.is_none() && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(50));
        }
        let status = engine.mcp_status();
        assert!(!status.running);
        assert!(status.error.is_some(), "status: {status:?}");
        assert!(status.attempts >= 1);
        let started = std::time::Instant::now();
        drop(server);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn disabled_config_publishes_status_and_does_not_listen() {
        let port = free_port();
        let engine = AppEngine::new();
        let mut config = test_config(port, false);
        config.enabled = false;
        let server =
            McpServer::start_with_config(engine.clone(), Arc::new(|| {}), config);
        let status = engine.mcp_status();
        assert!(!status.enabled);
        assert!(!status.running);
        assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
        server.stop();
    }

    #[test]
    fn rejects_non_loopback_origins_and_hosts_before_rpc_dispatch() {
        let port = free_port();
        let (_server, _engine, status) = start_test_server(port, false);
        assert!(status.running);
        let body = json!({"jsonrpc": "2.0", "id": 1, "method": "ping"});
        let (code, _, _) = raw_post(port, body.clone(), "Origin: https://attacker.example\r\n");
        assert_eq!(code, 403);

        let bytes = serde_json::to_vec(&body).unwrap();
        let headers = format!(
            "POST /mcp HTTP/1.1\r\nHost: evil.example:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            bytes.len()
        );
        let (code, _, _) = raw_exchange(port, &headers, &bytes);
        assert_eq!(code, 403);
    }

    #[test]
    fn rejects_invalid_json_rpc_identifiers_before_dispatch() {
        let port = free_port();
        let (_server, _engine, status) = start_test_server(port, false);
        assert!(status.running);
        for id in [json!(null), json!(true), json!({"bad": 1}), json!([1])] {
            let body = json!({
                "jsonrpc": "2.0", "id": id, "method": "ping",
                "params": {"_meta": modern_meta()}
            });
            let (code, response, _) = raw_post(
                port,
                body,
                &format!("MCP-Protocol-Version: {MODERN_PROTOCOL_VERSION}\r\nMcp-Method: ping\r\n"),
            );
            assert_eq!(code, 400);
            assert_eq!(response["error"]["code"], -32600);
            assert_eq!(response["id"], Value::Null);
        }
    }

    #[test]
    fn missing_modern_client_capabilities_is_invalid_params() {
        let port = free_port();
        let (_server, _engine, status) = start_test_server(port, false);
        assert!(status.running);
        let body = json!({
            "jsonrpc": "2.0", "id": 1, "method": "ping",
            "params": {"_meta": {"io.modelcontextprotocol/protocolVersion": MODERN_PROTOCOL_VERSION}}
        });
        let (code, response, _) = raw_post(
            port,
            body,
            &format!("MCP-Protocol-Version: {MODERN_PROTOCOL_VERSION}\r\nMcp-Method: ping\r\n"),
        );
        assert_eq!(code, 400);
        assert_eq!(response["error"]["code"], -32602);
    }

    #[test]
    fn stalled_body_does_not_block_other_requests_or_server_drop() {
        let port = free_port();
        let (server, _engine, status) = start_test_server(port, false);
        assert!(status.running);

        let mut stalled = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stalled
            .write_all(
                format!(
                    "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept: application/json, text/event-stream\r\nContent-Type: application/json\r\nContent-Length: 100\r\nConnection: close\r\n\r\n{{\"jsonrpc\":\"2.0\""
                )
                .as_bytes(),
            )
            .unwrap();
        thread::sleep(Duration::from_millis(50));

        let ping = json!({"jsonrpc": "2.0", "id": 1, "method": "ping"});
        let (code, response, _) = raw_post(
            port,
            ping,
            &format!("MCP-Protocol-Version: {LEGACY_PROTOCOL_VERSION}\r\n"),
        );
        assert_eq!(code, 200);
        assert_eq!(response["result"], json!({}));

        let started = std::time::Instant::now();
        drop(server);
        assert!(started.elapsed() < Duration::from_secs(2));
        drop(stalled);
    }
}

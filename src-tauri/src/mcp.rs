//! A small, loopback-only MCP Streamable HTTP endpoint for the tray engine.
//!
//! The endpoint is stateless at the transport layer. It serves the current
//! 2026-07-28 request-metadata protocol and the 2025-11-25 initialize handshake
//! for older clients. Tool calls use the same `AppEngine` instance as the UI.

use crate::engine::AppEngine;
use crate::types::{Config, LogsQuery, PortOwnerExpectation, QuitMode, SaveLogsArgs, ServiceName};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::io::Read;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

pub const MCP_PORT: u16 = 47_551;
pub const MCP_PATH: &str = "/mcp";
pub const MCP_ENDPOINT: &str = "http://127.0.0.1:47551/mcp";
pub const MODERN_PROTOCOL_VERSION: &str = "2026-07-28";
pub const LEGACY_PROTOCOL_VERSION: &str = "2025-11-25";
const MAX_REQUEST_BYTES: usize = 1_048_576;
const MAX_IN_FLIGHT_REQUESTS: usize = 16;
const SERVER_NAME: &str = "aztray";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpStatus {
    pub endpoint: String,
    pub active: bool,
    pub error: Option<String>,
}

impl McpStatus {
    pub fn stopped() -> Self {
        Self {
            endpoint: MCP_ENDPOINT.to_string(),
            active: false,
            error: None,
        }
    }
}

pub struct McpServer {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl McpServer {
    /// Binds before returning, so callers can report an occupied port while
    /// keeping the tray application alive.
    pub fn start(
        engine: AppEngine,
        status: Arc<RwLock<McpStatus>>,
        on_quit: Arc<dyn Fn() + Send + Sync + 'static>,
    ) -> Result<Self, String> {
        Self::start_at(
            engine,
            status,
            on_quit,
            SocketAddr::from(([127, 0, 0, 1], MCP_PORT)),
            MCP_ENDPOINT.to_string(),
        )
    }

    fn start_at(
        engine: AppEngine,
        status: Arc<RwLock<McpStatus>>,
        on_quit: Arc<dyn Fn() + Send + Sync + 'static>,
        bind_address: SocketAddr,
        endpoint: String,
    ) -> Result<Self, String> {
        let port = bind_address.port();
        let server = Server::http(bind_address).map_err(|error| {
            format!(
                "could not start the MCP endpoint at {endpoint}: {error}; port {port} may already be in use"
            )
        })?;
        if let Ok(mut current) = status.write() {
            *current = McpStatus {
                endpoint: endpoint.clone(),
                active: true,
                error: None,
            };
        }

        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker_status = status.clone();
        let worker = thread::Builder::new()
            .name("aztray-mcp-http".into())
            .spawn(move || {
                let in_flight = Arc::new(AtomicUsize::new(0));
                while !worker_stop.load(Ordering::Acquire) {
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
                            let status = worker_status.clone();
                            let in_flight = in_flight.clone();
                            let failure_slot = in_flight.clone();
                            if thread::Builder::new()
                                .name("aztray-mcp-request".into())
                                .spawn(move || {
                                    let _slot = RequestSlot(in_flight);
                                    serve_request(request, &engine, &status, &on_quit, port);
                                })
                                .is_err()
                            {
                                failure_slot.fetch_sub(1, Ordering::Release);
                            }
                        }
                        Ok(None) => {}
                        Err(error) => {
                            if let Ok(mut current) = worker_status.write() {
                                current.active = false;
                                current.error = Some(format!("MCP HTTP listener stopped: {error}"));
                            }
                            break;
                        }
                    }
                }
                if let Ok(mut current) = worker_status.write() {
                    current.active = false;
                }
            })
            .map_err(|error| {
                if let Ok(mut current) = status.write() {
                    current.active = false;
                    current.error =
                        Some(format!("could not start the MCP listener thread: {error}"));
                }
                format!("could not start the MCP listener thread: {error}")
            })?;

        Ok(Self {
            stop,
            worker: Some(worker),
        })
    }
}

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

impl Drop for McpServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn serve_request(
    mut request: Request,
    engine: &AppEngine,
    status: &Arc<RwLock<McpStatus>>,
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
        response.add_header(header("Access-Control-Allow-Methods", "POST, GET, OPTIONS"));
        response.add_header(header(
            "Access-Control-Allow-Headers",
            "Accept, Content-Type, MCP-Protocol-Version, Mcp-Method, Mcp-Name, Mcp-Session-Id, Last-Event-ID",
        ));
        response.add_header(header("Access-Control-Max-Age", "600"));
        let _ = request.respond(response);
        return;
    }

    if request.method() == &Method::Get {
        let mut response =
            Response::from_string("This MCP endpoint does not provide a standalone event stream.")
                .with_status_code(StatusCode(405));
        response.add_header(header("Allow", "POST, OPTIONS"));
        response = with_cors(response, cors_origin.as_deref());
        let _ = request.respond(response);
        return;
    }

    if request.method() != &Method::Post {
        let mut response =
            Response::from_string("Method not allowed").with_status_code(StatusCode(405));
        response.add_header(header("Allow", "POST, OPTIONS"));
        response = with_cors(response, cors_origin.as_deref());
        let _ = request.respond(response);
        return;
    }

    if !accepts_mcp_json(header_value(&request, "accept")) {
        let _ = request.respond(http_error(
            406,
            "Accept must include application/json and text/event-stream",
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
        // Notifications have no response body. `notifications/initialized`
        // is the only core lifecycle notification used by the legacy era.
        let accepted = method == "notifications/initialized"
            || method == "notifications/cancelled"
            || method.starts_with("notifications/");
        let mut response = Response::empty(StatusCode(if accepted { 202 } else { 400 }));
        response = with_cors(response, cors_origin.as_deref());
        let _ = request.respond(response);
        return;
    }

    let id = rpc.get("id").cloned().unwrap_or(Value::Null);
    let mcp_status = status
        .read()
        .map(|status| status.clone())
        .unwrap_or_else(|_| McpStatus {
            endpoint: MCP_ENDPOINT.to_string(),
            active: false,
            error: Some("MCP status is unavailable because its state lock was poisoned".into()),
        });
    let dispatched = dispatch_rpc(engine, &mcp_status, &rpc, modern);
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

    let response = json_response(status, response_body, cors_origin.as_deref());
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
    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        let Some((host, suffix)) = rest.split_once(']') else {
            return false;
        };
        let port = if suffix.is_empty() {
            None
        } else if let Some(port) = suffix.strip_prefix(':') {
            if port.is_empty() || port.parse::<u16>().is_err() {
                return false;
            }
            Some(port)
        } else {
            return false;
        };
        (host, port)
    } else if let Some((host, port)) = authority.rsplit_once(':') {
        if host.contains(':') || port.is_empty() || port.parse::<u16>().is_err() {
            return false;
        }
        (host, Some(port))
    } else {
        (authority, None)
    };
    let _ = port;
    host.eq_ignore_ascii_case("localhost")
        || host.eq_ignore_ascii_case("localhost.localdomain")
        || host
            .parse::<IpAddr>()
            .map(|address| address.is_loopback())
            .unwrap_or(false)
}

fn validate_rpc_transport(
    request: &Request,
    rpc: &Value,
    modern: bool,
) -> Result<(), (u16, Value)> {
    if rpc.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err((
            400,
            rpc_error(
                rpc.get("id").cloned().unwrap_or(Value::Null),
                -32600,
                "Invalid Request",
                None,
            ),
        ));
    }
    let Some(method) = rpc.get("method").and_then(Value::as_str) else {
        return Err((
            400,
            rpc_error(
                rpc.get("id").cloned().unwrap_or(Value::Null),
                -32600,
                "Invalid Request",
                None,
            ),
        ));
    };
    if rpc.get("params").is_some_and(|params| !params.is_object()) {
        return Err((
            400,
            rpc_error(
                rpc.get("id").cloned().unwrap_or(Value::Null),
                -32602,
                "Invalid params",
                None,
            ),
        ));
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
            return Err((
                400,
                rpc_error(
                    rpc.get("id").cloned().unwrap_or(Value::Null),
                    -32602,
                    "Modern requests require clientCapabilities in params._meta",
                    None,
                ),
            ));
        }
        if ["mcp-protocol-version", "mcp-method", "mcp-name"]
            .iter()
            .any(|name| header_count(request, name) > 1)
        {
            return Err((
                400,
                rpc_error(
                    rpc.get("id").cloned().unwrap_or(Value::Null),
                    -32020,
                    "Duplicate routing header",
                    None,
                ),
            ));
        }
        let protocol_header = header_value(request, "mcp-protocol-version");
        let method_header = header_value(request, "mcp-method");
        let header_name = header_value(request, "mcp-name");
        let params_name = match method {
            "tools/call" => params
                .and_then(|params| params.get("name"))
                .and_then(Value::as_str),
            "resources/read" => params
                .and_then(|params| params.get("uri"))
                .and_then(Value::as_str),
            "prompts/get" => params
                .and_then(|params| params.get("name"))
                .and_then(Value::as_str),
            _ => None,
        };
        if ["resources/read", "prompts/get"].contains(&method) && params_name.is_none() {
            return Err((
                400,
                rpc_error(
                    rpc.get("id").cloned().unwrap_or(Value::Null),
                    -32602,
                    "Resource reads require params.uri and prompt requests require params.name",
                    None,
                ),
            ));
        }
        let mismatch = protocol_header != version
            || method_header != Some(method)
            || (matches!(method, "tools/call" | "resources/read" | "prompts/get")
                && header_name != params_name)
            || (!matches!(method, "tools/call" | "resources/read" | "prompts/get")
                && header_name.is_some());
        if mismatch || version.is_none() {
            return Err((
                400,
                rpc_error(
                    rpc.get("id").cloned().unwrap_or(Value::Null),
                    -32020,
                    "Header mismatch or missing modern request metadata",
                    None,
                ),
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
        if ![LEGACY_PROTOCOL_VERSION, "2025-06-18", "2025-03-26"].contains(&version) {
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
    } else if method != "initialize" {
        return Err((
            400,
            rpc_error(
                rpc.get("id").cloned().unwrap_or(Value::Null),
                -32022,
                "MCP-Protocol-Version header is required after initialize",
                Some(json!({"supported": [MODERN_PROTOCOL_VERSION, LEGACY_PROTOCOL_VERSION]})),
            ),
        ));
    }
    Ok(())
}

fn request_is_modern(rpc: &Value, protocol_header: Option<&str>) -> bool {
    protocol_header == Some(MODERN_PROTOCOL_VERSION)
        || rpc
            .pointer("/params/_meta/io.modelcontextprotocol~1protocolVersion")
            .and_then(Value::as_str)
            == Some(MODERN_PROTOCOL_VERSION)
}

#[derive(Debug)]
struct RpcError {
    code: i64,
    message: String,
    data: Option<Value>,
}

fn dispatch_rpc(
    engine: &AppEngine,
    mcp_status: &McpStatus,
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
            let protocol_version =
                if [LEGACY_PROTOCOL_VERSION, "2025-06-18", "2025-03-26"].contains(&requested) {
                    requested
                } else {
                    LEGACY_PROTOCOL_VERSION
                };
            Ok((
                json!({
                    "protocolVersion": protocol_version,
                    "capabilities": {"tools": {"listChanged": false}},
                    "serverInfo": {"name": SERVER_NAME, "version": SERVER_VERSION},
                    "instructions": "AzTray controls the local Azurite Blob, Queue, and Table services. Use tools to inspect state and control only those configured services. Free port requires the observed process PID and creation timestamp plus confirmed=true."
                }),
                false,
            ))
        }
        "server/discover" if modern => Ok((
            modern_result(json!({
                "supportedVersions": [MODERN_PROTOCOL_VERSION, LEGACY_PROTOCOL_VERSION],
                "capabilities": {"tools": {"listChanged": false}},
                "instructions": "AzTray controls the local Azurite Blob, Queue, and Table services. Use tools to inspect state and control only those configured services. Free port requires the observed process PID and creation timestamp plus confirmed=true.",
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
            let executed = execute_tool(engine, mcp_status, name, &arguments);
            let mut result = match executed {
                Ok(value) => tool_result(value, false),
                Err(error) => tool_result(json!({"error": error}), true),
            };
            if modern {
                result["resultType"] = json!("complete");
                stamp_server_info(&mut result);
            }
            let should_quit = result
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
            "description": "Local tray controller for Azurite services"
        }
    });
}

fn tool_result(value: Value, is_error: bool) -> Value {
    let serialized = serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".into());
    json!({
        "content": [{"type": "text", "text": serialized}],
        "structuredContent": {"value": value},
        "isError": is_error
    })
}

fn execute_tool(
    engine: &AppEngine,
    mcp_status: &McpStatus,
    name: &str,
    arguments: &Map<String, Value>,
) -> Result<Value, String> {
    let arg = |key: &str| arguments.get(key).cloned().unwrap_or(Value::Null);
    let service = || parse_service(arguments, "serviceName");
    match name {
        "aztray_snapshot" => as_json(engine.get_snapshot()),
        "aztray_mcp_status" => as_json(mcp_status),
        "aztray_get_config" => as_json(engine.get_snapshot().config),
        "aztray_set_config" => {
            let config = serde_json::from_value::<Config>(arg("config")).map_err(|_| {
                "config must contain host, ports, dataDirectory, executablePath, and nodePath"
                    .to_string()
            })?;
            as_json(engine.set_config(config)?)
        }
        "aztray_check_engine" => as_json(engine.check_engine()),
        "aztray_start_service" => as_json(engine.start_service(service()?)?),
        "aztray_stop_service" => as_json(engine.stop_service(service()?)?),
        "aztray_restart_service" => as_json(engine.restart_service(service()?)?),
        "aztray_start_all" => as_json(engine.start_all()?),
        "aztray_stop_all" => as_json(engine.stop_all()?),
        "aztray_restart_all" => as_json(engine.restart_all()?),
        "aztray_identify_port_owner" => as_json(engine.identify_port_owner(service()?)),
        "aztray_free_port" => {
            if arg("confirmed").as_bool() != Some(true) {
                return Err("free_port requires confirmed=true after the process identity has been reviewed".into());
            }
            if !arguments.contains_key("startedAt") {
                return Err("free_port requires the observed startedAt value; pass null only when the owner reports null".into());
            }
            let pid = arg("pid")
                .as_u64()
                .and_then(|pid| u32::try_from(pid).ok())
                .ok_or_else(|| "pid must be a positive process ID".to_string())?;
            let started_at = match arg("startedAt") {
                Value::Null => None,
                Value::String(value) => Some(value),
                _ => return Err("startedAt must be a string or null".into()),
            };
            as_json(engine.free_port(PortOwnerExpectation {
                service_name: service()?,
                pid,
                started_at,
            })?)
        }
        "aztray_get_logs" => {
            let service_name = match arg("serviceName") {
                Value::Null => None,
                value => Some(
                    serde_json::from_value(value)
                        .map_err(|_| "serviceName must be blob, queue, or table".to_string())?,
                ),
            };
            let limit = match arg("limit") {
                Value::Null => None,
                value => Some(
                    value
                        .as_u64()
                        .and_then(|limit| usize::try_from(limit).ok())
                        .ok_or_else(|| "limit must be a non-negative integer".to_string())?,
                ),
            };
            as_json(engine.get_logs(LogsQuery {
                service_name,
                limit,
            }))
        }
        "aztray_save_logs" => {
            let service_name = match arg("serviceName") {
                Value::Null => None,
                value => Some(
                    serde_json::from_value(value)
                        .map_err(|_| "serviceName must be blob, queue, or table".to_string())?,
                ),
            };
            let path = match arg("path") {
                Value::Null => None,
                Value::String(path) => Some(path),
                _ => return Err("path must be a string or null".into()),
            };
            as_json(engine.save_logs(SaveLogsArgs { service_name, path })?)
        }
        "aztray_clear_logs" => {
            let service_name = match arg("serviceName") {
                Value::Null => None,
                value => Some(
                    serde_json::from_value(value)
                        .map_err(|_| "serviceName must be blob, queue, or table".to_string())?,
                ),
            };
            as_json(engine.clear_logs(service_name))
        }
        "aztray_connection_string" => {
            Ok(json!({"connectionString": engine.connection_string(service()?)}))
        }
        "aztray_quit" => {
            let mode = match arg("mode").as_str() {
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

fn parse_service(arguments: &Map<String, Value>, key: &str) -> Result<ServiceName, String> {
    serde_json::from_value(arguments.get(key).cloned().unwrap_or(Value::Null))
        .map_err(|_| format!("{key} must be blob, queue, or table"))
}

fn as_json<T: Serialize>(value: T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|_| "Could not encode AzTray result".to_string())
}

fn tool_definitions() -> Vec<Value> {
    let service_schema = || json!({"type": "string", "enum": ["blob", "queue", "table"]});
    let service_tool = |name: &str, description: &str, destructive: bool, idempotent: bool| {
        json!({
            "name": name,
            "description": description,
            "inputSchema": {"type": "object", "properties": {"serviceName": service_schema()}, "required": ["serviceName"], "additionalProperties": false},
            "annotations": {"readOnlyHint": false, "destructiveHint": destructive, "idempotentHint": idempotent, "openWorldHint": false}
        })
    };
    let read_service_tool = |name: &str, description: &str| {
        json!({
            "name": name,
            "description": description,
            "inputSchema": {"type": "object", "properties": {"serviceName": service_schema()}, "required": ["serviceName"], "additionalProperties": false},
            "annotations": {"readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false}
        })
    };
    let no_args = |name: &str,
                   description: &str,
                   read_only: bool,
                   destructive: bool,
                   idempotent: bool| {
        json!({
            "name": name,
            "description": description,
            "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false},
            "annotations": {"readOnlyHint": read_only, "destructiveHint": destructive, "idempotentHint": idempotent, "openWorldHint": false}
        })
    };
    vec![
        no_args("aztray_snapshot", "Read the current AzTray configuration, engine and service states, process identities, and bounded logs.", true, false, true),
        no_args("aztray_mcp_status", "Read the MCP endpoint URL and whether its local HTTP listener started successfully.", true, false, true),
        no_args("aztray_get_config", "Read the current AzTray service host, ports, data directory, and executable overrides.", true, false, true),
        json!({
            "name": "aztray_set_config",
            "description": "Persist a new AzTray host, port, data directory, and executable configuration. All Azurite services must be stopped. This changes AzTray settings and does not alter existing Azurite data.",
            "inputSchema": {
                "type": "object",
                "properties": {"config": {
                    "type": "object",
                    "properties": {
                        "host": {"type": "string", "minLength": 1},
                        "ports": {"type": "object", "properties": {
                            "blob": {"type": "integer", "minimum": 1, "maximum": 65535},
                            "queue": {"type": "integer", "minimum": 1, "maximum": 65535},
                            "table": {"type": "integer", "minimum": 1, "maximum": 65535}
                        }, "required": ["blob", "queue", "table"], "additionalProperties": false},
                        "dataDirectory": {"type": "string", "minLength": 1},
                        "executablePath": {"type": ["string", "null"]},
                        "nodePath": {"type": ["string", "null"]}
                    },
                    "required": ["host", "ports", "dataDirectory", "executablePath", "nodePath"],
                    "additionalProperties": false
                }},
                "required": ["config"], "additionalProperties": false
            },
            "annotations": {"readOnlyHint": false, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false}
        }),
        no_args("aztray_check_engine", "Check whether the configured Node.js and Azurite engine are available.", true, false, true),
        service_tool("aztray_start_service", "Start the selected Azurite service using the current AzTray configuration.", false, true),
        service_tool("aztray_stop_service", "Stop the selected Azurite service owned by AzTray. This terminates that service process.", true, true),
        service_tool("aztray_restart_service", "Stop and start the selected Azurite service. This terminates and relaunches that service process.", true, false),
        no_args("aztray_start_all", "Start Blob, Queue, and Table services using the current AzTray configuration.", false, false, true),
        no_args("aztray_stop_all", "Stop all Azurite service processes owned by AzTray.", false, true, true),
        no_args("aztray_restart_all", "Stop and start all Azurite services owned by AzTray.", false, true, false),
        read_service_tool("aztray_identify_port_owner", "Read the current process identity for the selected Azurite service port. Use these observed PID and startedAt values when requesting a port release."),
        json!({
            "name": "aztray_free_port",
            "description": "Terminate the current process owning a configured service port. First inspect aztray_identify_port_owner and pass its exact pid and startedAt values, then set confirmed=true after the user explicitly approves releasing that process. The engine rechecks the process identity before termination and refuses if it changed.",
            "inputSchema": {"type": "object", "properties": {
                "serviceName": service_schema(),
                "pid": {"type": "integer", "minimum": 1, "maximum": 4294967295u64},
                "startedAt": {"type": ["string", "null"], "description": "Exact startedAt value from aztray_identify_port_owner, including null when the owner reports null."},
                "confirmed": {"type": "boolean", "const": true, "description": "Set true only after the user has approved terminating the observed port owner."}
            }, "required": ["serviceName", "pid", "startedAt", "confirmed"], "additionalProperties": false},
            "annotations": {"readOnlyHint": false, "destructiveHint": true, "idempotentHint": false, "openWorldHint": false}
        }),
        json!({
            "name": "aztray_get_logs",
            "description": "Read recent bounded logs for one service or the merged AzTray log stream.",
            "inputSchema": {"type": "object", "properties": {"serviceName": service_schema(), "limit": {"type": "integer", "minimum": 0, "maximum": 6000}}, "additionalProperties": false},
            "annotations": {"readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false}
        }),
        json!({
            "name": "aztray_save_logs",
            "description": "Write the selected AzTray logs to a text file. When path is omitted, AzTray chooses its default log file; an existing supplied path may be overwritten.",
            "inputSchema": {"type": "object", "properties": {"serviceName": service_schema(), "path": {"type": "string", "minLength": 1}}, "additionalProperties": false},
            "annotations": {"readOnlyHint": false, "destructiveHint": true, "idempotentHint": false, "openWorldHint": false}
        }),
        json!({
            "name": "aztray_clear_logs",
            "description": "Clear the in-memory AzTray logs for one service or for all services. This does not delete Azurite data files.",
            "inputSchema": {"type": "object", "properties": {"serviceName": service_schema()}, "additionalProperties": false},
            "annotations": {"readOnlyHint": false, "destructiveHint": true, "idempotentHint": true, "openWorldHint": false}
        }),
        read_service_tool("aztray_connection_string", "Read the local Azurite connection string for the selected service."),
        json!({
            "name": "aztray_quit",
            "description": "Close the AzTray controller. stop_and_quit stops AzTray-owned Azurite services before closing; leave_running closes AzTray while services continue; cancel keeps AzTray open. The MCP response is sent before the application closes.",
            "inputSchema": {"type": "object", "properties": {"mode": {"type": "string", "enum": ["stop_and_quit", "leave_running", "cancel"]}}, "required": ["mode"], "additionalProperties": false},
            "annotations": {"readOnlyHint": false, "destructiveHint": true, "idempotentHint": false, "openWorldHint": false}
        }),
    ]
}

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
        (header.field.as_str().to_ascii_lowercase() == name.to_ascii_lowercase())
            .then(|| header.value.as_str())
    })
}

fn header_count(request: &Request, name: &str) -> usize {
    request
        .headers()
        .iter()
        .filter(|header| header.field.as_str().to_ascii_lowercase() == name.to_ascii_lowercase())
        .count()
}

fn accepts_mcp_json(accept: Option<&str>) -> bool {
    let Some(accept) = accept else { return false };
    let mut json = false;
    let mut event_stream = false;
    for item in accept
        .split(',')
        .map(|item| item.trim().split(';').next().unwrap_or_default().trim())
    {
        json |= item.eq_ignore_ascii_case("application/json");
        event_stream |= item.eq_ignore_ascii_case("text/event-stream");
    }
    json && event_stream
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
    let mut response = Response::from_data(body)
        .with_status_code(StatusCode(status))
        .with_header(header("Content-Type", "application/json; charset=utf-8"));
    response = with_cors(response, origin);
    response
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

    fn raw_post(port: u16, body: Value, extra_headers: &str) -> (u16, Value, String) {
        use std::io::Write;

        let body = serde_json::to_vec(&body).unwrap();
        let headers = format!(
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept: application/json, text/event-stream\r\nContent-Type: application/json\r\n{extra_headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.write_all(headers.as_bytes()).unwrap();
        stream.write_all(&body).unwrap();
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

    #[test]
    fn streamable_http_serves_modern_and_legacy_clients() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let endpoint = format!("http://127.0.0.1:{}/mcp", address.port());
        let status = Arc::new(RwLock::new(McpStatus::stopped()));
        let server = McpServer::start_at(
            AppEngine::new(),
            status.clone(),
            Arc::new(|| {}),
            address,
            endpoint.clone(),
        )
        .expect("start local MCP test server");
        assert_eq!(status.read().unwrap().endpoint, endpoint);

        let discover = json!({
            "jsonrpc": "2.0", "id": 1, "method": "server/discover",
            "params": {"_meta": modern_meta()}
        });
        let (status_code, discover_result, _) = raw_post(
            address.port(),
            discover,
            &format!("MCP-Protocol-Version: {MODERN_PROTOCOL_VERSION}\r\nMcp-Method: server/discover\r\n"),
        );
        assert_eq!(status_code, 200);
        assert_eq!(discover_result["result"]["resultType"], "complete");
        assert_eq!(
            discover_result["result"]["supportedVersions"],
            json!([MODERN_PROTOCOL_VERSION, LEGACY_PROTOCOL_VERSION])
        );

        let list = json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/list",
            "params": {"_meta": modern_meta()}
        });
        let (status_code, list_result, _) = raw_post(
            address.port(),
            list,
            &format!(
                "MCP-Protocol-Version: {MODERN_PROTOCOL_VERSION}\r\nMcp-Method: tools/list\r\n"
            ),
        );
        assert_eq!(status_code, 200);
        assert_eq!(list_result["result"]["resultType"], "complete");
        assert!(list_result["result"]["tools"].as_array().unwrap().len() >= 16);

        let call = json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": {"name": "aztray_snapshot", "arguments": {}, "_meta": modern_meta()}
        });
        let (status_code, call_result, _) = raw_post(
            address.port(),
            call,
            &format!("MCP-Protocol-Version: {MODERN_PROTOCOL_VERSION}\r\nMcp-Method: tools/call\r\nMcp-Name: aztray_snapshot\r\n"),
        );
        assert_eq!(status_code, 200);
        assert_eq!(call_result["result"]["resultType"], "complete");
        assert!(call_result["result"]["structuredContent"]["value"]["services"].is_object());

        let status_call = json!({
            "jsonrpc": "2.0", "id": 30, "method": "tools/call",
            "params": {"name": "aztray_mcp_status", "arguments": {}, "_meta": modern_meta()}
        });
        let (status_code, status_result, _) = raw_post(
            address.port(),
            status_call,
            &format!("MCP-Protocol-Version: {MODERN_PROTOCOL_VERSION}\r\nMcp-Method: tools/call\r\nMcp-Name: aztray_mcp_status\r\n"),
        );
        assert_eq!(status_code, 200);
        assert_eq!(
            status_result["result"]["structuredContent"]["value"]["active"],
            true
        );
        assert_eq!(
            status_result["result"]["structuredContent"]["value"]["endpoint"],
            endpoint
        );

        let mismatched_name = json!({
            "jsonrpc": "2.0", "id": 4, "method": "tools/call",
            "params": {"name": "aztray_snapshot", "arguments": {}, "_meta": modern_meta()}
        });
        let (status_code, mismatch, _) = raw_post(
            address.port(),
            mismatched_name,
            &format!("MCP-Protocol-Version: {MODERN_PROTOCOL_VERSION}\r\nMcp-Method: tools/call\r\nMcp-Name: different_tool\r\n"),
        );
        assert_eq!(status_code, 400);
        assert_eq!(mismatch["error"]["code"], -32020);

        let initialize = json!({
            "jsonrpc": "2.0", "id": 5, "method": "initialize",
            "params": {
                "protocolVersion": LEGACY_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": "test-client", "version": "1.0"}
            }
        });
        let (status_code, initialize_result, _) = raw_post(address.port(), initialize, "");
        assert_eq!(status_code, 200);
        assert_eq!(
            initialize_result["result"]["protocolVersion"],
            LEGACY_PROTOCOL_VERSION
        );

        let initialized = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        let (status_code, _, _) = raw_post(
            address.port(),
            initialized,
            &format!("MCP-Protocol-Version: {LEGACY_PROTOCOL_VERSION}\r\n"),
        );
        assert_eq!(status_code, 202);

        let legacy_list = json!({"jsonrpc": "2.0", "id": 6, "method": "tools/list"});
        let (status_code, legacy_result, _) = raw_post(
            address.port(),
            legacy_list,
            &format!("MCP-Protocol-Version: {LEGACY_PROTOCOL_VERSION}\r\n"),
        );
        assert_eq!(status_code, 200);
        assert!(legacy_result["result"]["tools"].is_array());
        assert!(legacy_result["result"].get("resultType").is_none());

        drop(server);
        assert!(!status.read().unwrap().active);
    }

    #[test]
    fn rejects_non_loopback_origins_before_rpc_dispatch() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let endpoint = format!("http://127.0.0.1:{}/mcp", address.port());
        let status = Arc::new(RwLock::new(McpStatus::stopped()));
        let _server =
            McpServer::start_at(AppEngine::new(), status, Arc::new(|| {}), address, endpoint)
                .expect("start local MCP test server");
        let body = json!({"jsonrpc": "2.0", "id": 1, "method": "ping"});
        let (status_code, _, _) =
            raw_post(address.port(), body, "Origin: https://attacker.example\r\n");
        assert_eq!(status_code, 403);
    }

    #[test]
    fn modern_tool_metadata_and_routing_headers_are_required() {
        let body = json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": "aztray_snapshot", "arguments": {}, "_meta": {
                "io.modelcontextprotocol/protocolVersion": MODERN_PROTOCOL_VERSION,
                "io.modelcontextprotocol/clientCapabilities": {}
            }}
        });
        assert!(request_is_modern(&body, Some(MODERN_PROTOCOL_VERSION)));
        assert_eq!(body["params"]["name"], "aztray_snapshot");
    }

    #[test]
    fn rejects_invalid_json_rpc_identifiers_before_dispatch() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let status = Arc::new(RwLock::new(McpStatus::stopped()));
        let _server = McpServer::start_at(
            AppEngine::new(),
            status,
            Arc::new(|| {}),
            address,
            format!("http://127.0.0.1:{}/mcp", address.port()),
        )
        .unwrap();

        for id in [json!(null), json!(true), json!({"bad": 1}), json!([1])] {
            let body = json!({
                "jsonrpc": "2.0", "id": id, "method": "ping",
                "params": {"_meta": modern_meta()}
            });
            let (status_code, response, _) = raw_post(
                address.port(),
                body,
                &format!("MCP-Protocol-Version: {MODERN_PROTOCOL_VERSION}\r\nMcp-Method: ping\r\n"),
            );
            assert_eq!(status_code, 400);
            assert_eq!(response["error"]["code"], -32600);
            assert_eq!(response["id"], Value::Null);
        }
    }

    #[test]
    fn missing_modern_client_capabilities_is_invalid_params() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let _server = McpServer::start_at(
            AppEngine::new(),
            Arc::new(RwLock::new(McpStatus::stopped())),
            Arc::new(|| {}),
            address,
            format!("http://127.0.0.1:{}/mcp", address.port()),
        )
        .unwrap();
        let body = json!({
            "jsonrpc": "2.0", "id": 1, "method": "ping",
            "params": {"_meta": {
                "io.modelcontextprotocol/protocolVersion": MODERN_PROTOCOL_VERSION
            }}
        });
        let (status_code, response, _) = raw_post(
            address.port(),
            body,
            &format!("MCP-Protocol-Version: {MODERN_PROTOCOL_VERSION}\r\nMcp-Method: ping\r\n"),
        );
        assert_eq!(status_code, 400);
        assert_eq!(response["error"]["code"], -32602);
    }

    #[test]
    fn resource_and_prompt_subject_headers_are_checked_before_not_found() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let _server = McpServer::start_at(
            AppEngine::new(),
            Arc::new(RwLock::new(McpStatus::stopped())),
            Arc::new(|| {}),
            address,
            format!("http://127.0.0.1:{}/mcp", address.port()),
        )
        .unwrap();

        for (method, subject_key, subject) in [
            ("resources/read", "uri", "file:///unsupported"),
            ("prompts/get", "name", "unsupported_prompt"),
        ] {
            let body = json!({
                "jsonrpc": "2.0", "id": 1, "method": method,
                "params": {subject_key: subject, "_meta": modern_meta()}
            });
            let extra = format!(
                "MCP-Protocol-Version: {MODERN_PROTOCOL_VERSION}\r\nMcp-Method: {method}\r\nMcp-Name: different_subject\r\n"
            );
            let (status_code, response, _) = raw_post(address.port(), body.clone(), &extra);
            assert_eq!(status_code, 400);
            assert_eq!(response["error"]["code"], -32020);

            let extra = format!(
                "MCP-Protocol-Version: {MODERN_PROTOCOL_VERSION}\r\nMcp-Method: {method}\r\nMcp-Name: {subject}\r\n"
            );
            let (status_code, response, _) = raw_post(address.port(), body, &extra);
            assert_eq!(status_code, 404);
            assert_eq!(response["error"]["code"], -32601);
        }
    }

    #[test]
    fn stalled_body_does_not_block_other_requests_or_server_drop() {
        use std::io::Write;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let _server = McpServer::start_at(
            AppEngine::new(),
            Arc::new(RwLock::new(McpStatus::stopped())),
            Arc::new(|| {}),
            address,
            format!("http://127.0.0.1:{}/mcp", address.port()),
        )
        .unwrap();

        let mut stalled = TcpStream::connect(address).unwrap();
        stalled
            .write_all(
                format!(
                    "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAccept: application/json, text/event-stream\r\nContent-Type: application/json\r\nContent-Length: 100\r\nConnection: close\r\n\r\n{{\"jsonrpc\":\"2.0\"",
                    address.port()
                )
                .as_bytes(),
            )
            .unwrap();
        thread::sleep(Duration::from_millis(50));

        let ping = json!({"jsonrpc": "2.0", "id": 1, "method": "ping"});
        let (status_code, response, _) = raw_post(
            address.port(),
            ping,
            &format!("MCP-Protocol-Version: {LEGACY_PROTOCOL_VERSION}\r\n"),
        );
        assert_eq!(status_code, 200);
        assert_eq!(response["result"], json!({}));

        let started = std::time::Instant::now();
        drop(_server);
        assert!(started.elapsed() < Duration::from_secs(1));
        drop(stalled);
    }

    #[test]
    fn origin_guard_accepts_local_webview_and_rejects_remote_hosts() {
        assert!(is_loopback_origin("http://localhost:3000"));
        assert!(is_loopback_origin("http://127.0.0.1:3000"));
        assert!(is_loopback_origin("tauri://localhost"));
        assert!(!is_loopback_origin("http://localhost.attacker.example"));
        assert!(!is_loopback_origin("http://8.8.8.8"));
        assert!(!is_loopback_origin("null"));
    }

    #[test]
    fn free_port_requires_confirmation_and_started_at() {
        let engine = AppEngine::new();
        let mut arguments = Map::new();
        arguments.insert("serviceName".into(), json!("blob"));
        arguments.insert("pid".into(), json!(123));
        arguments.insert("startedAt".into(), Value::Null);
        arguments.insert("confirmed".into(), json!(false));
        let error = execute_tool(
            &engine,
            &McpStatus::stopped(),
            "aztray_free_port",
            &arguments,
        )
        .unwrap_err();
        assert!(error.contains("confirmed=true"));
    }

    #[test]
    fn tool_catalog_is_deterministic_and_marks_mutation() {
        let tools = tool_definitions();
        assert_eq!(
            tools.first().and_then(|tool| tool["name"].as_str()),
            Some("aztray_snapshot")
        );
        let free_port = tools
            .iter()
            .find(|tool| tool["name"] == "aztray_free_port")
            .unwrap();
        assert_eq!(free_port["annotations"]["destructiveHint"], true);
        assert_eq!(
            free_port["inputSchema"]["required"],
            json!(["serviceName", "pid", "startedAt", "confirmed"])
        );
    }
}

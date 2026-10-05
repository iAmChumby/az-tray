pub mod config;
pub mod engine;
pub mod logs;
pub mod mcp;
pub mod ports;
pub mod tray;
pub mod types;

use std::sync::{Arc, Mutex};

use engine::{AppEngine, EngineEvent};
use tauri::{async_runtime, AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_autostart::ManagerExt as AutostartManagerExt;
use types::{
    AppSnapshot, ConnectionInfo, CreateInstanceRequest, DeleteInstanceRequest, EngineSnapshot,
    FreePortResult, GlobalSettings, InstanceDraft, InstanceSnapshot, LogEntry, LogLevel, LogsQuery,
    McpStatus, PortOwner, PortOwnerExpectation, QuitMode, QuitResult, SaveLogsArgs, SaveLogsResult,
    ServiceName, UpdateInstanceRequest,
};

#[derive(Clone)]
pub struct AppState {
    /// Owns the McpStatus as well as all instances.
    pub engine: AppEngine,
    /// Dropping the server stops and joins it.
    pub mcp_server: Arc<Mutex<Option<mcp::McpServer>>>,
}

fn engine_from(app: &AppHandle) -> AppEngine {
    app.state::<AppState>().engine.clone()
}

async fn blocking_value<T, F>(engine: AppEngine, operation: F) -> T
where
    T: Send + 'static,
    F: FnOnce(AppEngine) -> T + Send + 'static,
{
    async_runtime::spawn_blocking(move || operation(engine))
        .await
        .expect("AzTray engine task panicked")
}

async fn blocking_result<T, F>(engine: AppEngine, operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(AppEngine) -> Result<T, String> + Send + 'static,
{
    async_runtime::spawn_blocking(move || operation(engine))
        .await
        .map_err(|error| format!("AzTray engine task failed: {error}"))?
}

/// (Re)start the MCP supervisor: the previous server (if any) is dropped, which
/// stops and joins it, then a fresh one is started. Never fails.
fn start_mcp(app: &AppHandle) {
    let state = app.state::<AppState>();
    let previous = state.mcp_server.lock().ok().and_then(|mut slot| slot.take());
    drop(previous);
    let quit_handle = app.clone();
    let on_quit: Arc<dyn Fn() + Send + Sync + 'static> = Arc::new(move || quit_handle.exit(0));
    let server = mcp::McpServer::start(state.engine.clone(), on_quit);
    let mut slot = match state.mcp_server.lock() {
        Ok(slot) => slot,
        Err(poisoned) => poisoned.into_inner(),
    };
    *slot = Some(server);
}

#[tauri::command]
async fn get_snapshot(app: AppHandle) -> AppSnapshot {
    blocking_value(engine_from(&app), |engine| engine.get_snapshot()).await
}

#[tauri::command]
async fn set_settings(app: AppHandle, settings: GlobalSettings) -> Result<AppSnapshot, String> {
    let engine = engine_from(&app);
    let before = engine.mcp_config();
    let snapshot = blocking_result(engine, move |engine| engine.set_settings(settings)).await?;
    if snapshot.config.mcp != before {
        let handle = app.clone();
        let _ = async_runtime::spawn_blocking(move || start_mcp(&handle)).await;
    }
    Ok(snapshot)
}

#[tauri::command]
async fn check_engine(app: AppHandle) -> EngineSnapshot {
    blocking_value(engine_from(&app), |engine| engine.check_engine()).await
}

#[tauri::command]
async fn list_instances(app: AppHandle) -> Vec<InstanceSnapshot> {
    blocking_value(engine_from(&app), |engine| engine.list_instances()).await
}

#[tauri::command]
async fn suggest_instance(app: AppHandle, name: Option<String>) -> InstanceDraft {
    blocking_value(engine_from(&app), move |engine| engine.suggest_instance(name.as_deref())).await
}

#[tauri::command]
async fn create_instance(app: AppHandle, request: CreateInstanceRequest) -> Result<InstanceSnapshot, String> {
    blocking_result(engine_from(&app), move |engine| engine.create_instance(request)).await
}

#[tauri::command]
async fn update_instance(app: AppHandle, request: UpdateInstanceRequest) -> Result<InstanceSnapshot, String> {
    blocking_result(engine_from(&app), move |engine| engine.update_instance(request)).await
}

#[tauri::command]
async fn delete_instance(app: AppHandle, request: DeleteInstanceRequest) -> Result<AppSnapshot, String> {
    blocking_result(engine_from(&app), move |engine| engine.delete_instance(&request.instance_id)).await
}

#[tauri::command]
async fn start_instance(app: AppHandle, instance_id: Option<String>) -> Result<AppSnapshot, String> {
    blocking_result(engine_from(&app), move |engine| engine.start_instance(instance_id.as_deref())).await
}

#[tauri::command]
async fn stop_instance(app: AppHandle, instance_id: Option<String>) -> Result<AppSnapshot, String> {
    blocking_result(engine_from(&app), move |engine| engine.stop_instance(instance_id.as_deref())).await
}

#[tauri::command]
async fn restart_instance(app: AppHandle, instance_id: Option<String>) -> Result<AppSnapshot, String> {
    blocking_result(engine_from(&app), move |engine| engine.restart_instance(instance_id.as_deref())).await
}

#[tauri::command]
async fn start_service(app: AppHandle, instance_id: Option<String>, service_name: ServiceName) -> Result<AppSnapshot, String> {
    blocking_result(engine_from(&app), move |engine| engine.start_service(instance_id.as_deref(), service_name)).await
}

#[tauri::command]
async fn stop_service(app: AppHandle, instance_id: Option<String>, service_name: ServiceName) -> Result<AppSnapshot, String> {
    blocking_result(engine_from(&app), move |engine| engine.stop_service(instance_id.as_deref(), service_name)).await
}

#[tauri::command]
async fn restart_service(app: AppHandle, instance_id: Option<String>, service_name: ServiceName) -> Result<AppSnapshot, String> {
    blocking_result(engine_from(&app), move |engine| engine.restart_service(instance_id.as_deref(), service_name)).await
}

#[tauri::command]
async fn start_all(app: AppHandle) -> Result<AppSnapshot, String> {
    blocking_result(engine_from(&app), |engine| engine.start_all()).await
}

#[tauri::command]
async fn stop_all(app: AppHandle) -> Result<AppSnapshot, String> {
    blocking_result(engine_from(&app), |engine| engine.stop_all()).await
}

#[tauri::command]
async fn restart_all(app: AppHandle) -> Result<AppSnapshot, String> {
    blocking_result(engine_from(&app), |engine| engine.restart_all()).await
}

#[tauri::command]
async fn identify_port_owner(app: AppHandle, instance_id: Option<String>, service_name: ServiceName) -> Option<PortOwner> {
    blocking_value(engine_from(&app), move |engine| engine.identify_port_owner(instance_id.as_deref(), service_name)).await
}

#[tauri::command]
async fn free_port(
    app: AppHandle,
    instance_id: Option<String>,
    service_name: ServiceName,
    pid: u32,
    started_at: Option<String>,
) -> Result<FreePortResult, String> {
    blocking_result(engine_from(&app), move |engine| {
        engine.free_port(PortOwnerExpectation { instance_id, service_name, pid, started_at })
    })
    .await
}

#[tauri::command]
async fn get_logs(
    app: AppHandle,
    instance_id: Option<String>,
    service_name: Option<ServiceName>,
    limit: Option<usize>,
) -> Vec<LogEntry> {
    blocking_value(engine_from(&app), move |engine| {
        engine.get_logs(LogsQuery { instance_id, service_name, limit })
    })
    .await
}

#[tauri::command]
async fn save_logs(
    app: AppHandle,
    instance_id: Option<String>,
    service_name: Option<ServiceName>,
    path: Option<String>,
) -> Result<SaveLogsResult, String> {
    blocking_result(engine_from(&app), move |engine| {
        engine.save_logs(SaveLogsArgs { instance_id, service_name, path })
    })
    .await
}

#[tauri::command]
async fn clear_logs(app: AppHandle, instance_id: Option<String>, service_name: Option<ServiceName>) -> AppSnapshot {
    blocking_value(engine_from(&app), move |engine| engine.clear_logs(instance_id.as_deref(), service_name)).await
}

#[tauri::command]
async fn get_app_log(app: AppHandle, limit: Option<usize>) -> Vec<String> {
    blocking_value(engine_from(&app), move |engine| engine.app_log_tail(limit.unwrap_or(200))).await
}

#[tauri::command]
async fn get_connection_info(app: AppHandle, instance_id: Option<String>) -> Result<ConnectionInfo, String> {
    blocking_result(engine_from(&app), move |engine| engine.connection_info(instance_id.as_deref())).await
}

#[tauri::command]
async fn get_connection_string(
    app: AppHandle,
    instance_id: Option<String>,
    service_name: Option<ServiceName>,
) -> Result<String, String> {
    blocking_result(engine_from(&app), move |engine| {
        engine.connection_string(instance_id.as_deref(), service_name)
    })
    .await
}

#[tauri::command]
async fn quit_app(app: AppHandle, mode: QuitMode) -> Result<QuitResult, String> {
    let quit_mode = mode.clone();
    let result = blocking_result(engine_from(&app), move |engine| engine.quit(quit_mode)).await?;
    if mode != QuitMode::Cancel {
        app.exit(0);
    }
    Ok(result)
}

#[tauri::command]
fn get_mcp_status(app: AppHandle) -> McpStatus {
    app.state::<AppState>().engine.mcp_status()
}

#[tauri::command]
async fn restart_mcp(app: AppHandle) -> McpStatus {
    let handle = app.clone();
    let _ = async_runtime::spawn_blocking(move || start_mcp(&handle)).await;
    app.state::<AppState>().engine.mcp_status()
}

/// Build and run the AzTray process. The controller stays alive in the tray
/// while both webviews remain hidden; window close requests are converted to
/// hide operations so service processes can keep running.
pub fn run() -> tauri::Result<()> {
    tauri::Builder::default()
        .manage(AppState {
            engine: AppEngine::new(),
            mcp_server: Arc::new(Mutex::new(None)),
        })
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            if argv.iter().any(|argument| argument == "--popover") {
                let _ = tray::toggle_popover(app);
            } else {
                let _ = tray::show_main(app);
            }
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let engine = app.state::<AppState>().engine.clone();
            let handle = app.handle().clone();

            // Forward engine events first so MCP status changes reach the UI.
            let event_handle = handle.clone();
            engine.set_event_handler(Some(Arc::new(move |event| {
                let result = match event {
                    EngineEvent::SnapshotUpdated(payload) => event_handle.emit("snapshot_updated", payload),
                    EngineEvent::InstanceUpdated(payload) => event_handle.emit("instance_updated", payload),
                    EngineEvent::ServiceUpdated(payload) => event_handle.emit("service_updated", payload),
                    EngineEvent::LogEntry(payload) => event_handle.emit("log_entry", payload),
                    EngineEvent::EngineUpdated(payload) => event_handle.emit("engine_updated", payload),
                    EngineEvent::McpUpdated(payload) => event_handle.emit("mcp_updated", payload),
                };
                let _ = result;
            })));

            // MCP starts BEFORE the tray, and a tray failure never stops it.
            start_mcp(&handle);
            engine.start_reaper();

            if let Err(error) = tray::build(&handle) {
                logs::app_log(LogLevel::Error, &format!("tray creation failed: {error}"));
            }

            // This writes only the current user's startup registration. The
            // tray process itself starts at sign-in; Azurite never does.
            let autostart = app.autolaunch();
            if !autostart.is_enabled().unwrap_or(false) {
                if let Err(error) = autostart.enable() {
                    logs::app_log(LogLevel::Warn, &format!("could not enable autostart: {error}"));
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let _ = window.hide();
            }
            WindowEvent::Focused(false) if window.label() == "popover" => {
                tray::debounce_hide(window.app_handle(),window.label());
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            set_settings,
            check_engine,
            quit_app,
            list_instances,
            suggest_instance,
            create_instance,
            update_instance,
            delete_instance,
            start_instance,
            stop_instance,
            restart_instance,
            start_service,
            stop_service,
            restart_service,
            start_all,
            stop_all,
            restart_all,
            identify_port_owner,
            free_port,
            get_logs,
            save_logs,
            clear_logs,
            get_app_log,
            get_connection_info,
            get_connection_string,
            get_mcp_status,
            restart_mcp
        ])
        .build(tauri::generate_context!())
        .map(|app| app.run(|_, _| {}))
}

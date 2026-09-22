pub mod tray;
pub mod types;
pub mod config;
pub mod engine;
pub mod logs;
pub mod ports;

use std::sync::Arc;

use engine::{AppEngine, EngineEvent};
use tauri::{async_runtime, AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_autostart::ManagerExt as AutostartManagerExt;
use types::{AppSnapshot, Config, EngineSnapshot, FreePortResult, LogEntry, LogsQuery, PortOwner, PortOwnerExpectation, QuitMode, QuitResult, SaveLogsArgs, SaveLogsResult, ServiceName};

#[derive(Clone)]
pub struct AppState {
    pub engine: AppEngine,
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

#[tauri::command]
async fn get_snapshot(app: AppHandle) -> AppSnapshot {
    blocking_value(engine_from(&app), |engine| engine.get_snapshot()).await
}

#[tauri::command]
async fn set_config(app: AppHandle, config: Config) -> Result<AppSnapshot, String> {
    blocking_result(engine_from(&app), move |engine| engine.set_config(config)).await
}

#[tauri::command]
async fn check_engine(app: AppHandle) -> EngineSnapshot {
    blocking_value(engine_from(&app), |engine| engine.check_engine()).await
}

#[tauri::command]
async fn start_service(app: AppHandle, service_name: ServiceName) -> Result<AppSnapshot, String> {
    blocking_result(engine_from(&app), move |engine| engine.start_service(service_name)).await
}

#[tauri::command]
async fn stop_service(app: AppHandle, service_name: ServiceName) -> Result<AppSnapshot, String> {
    blocking_result(engine_from(&app), move |engine| engine.stop_service(service_name)).await
}

#[tauri::command]
async fn restart_service(app: AppHandle, service_name: ServiceName) -> Result<AppSnapshot, String> {
    blocking_result(engine_from(&app), move |engine| engine.restart_service(service_name)).await
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
async fn identify_port_owner(app: AppHandle, service_name: ServiceName) -> Option<PortOwner> {
    blocking_value(engine_from(&app), move |engine| engine.identify_port_owner(service_name)).await
}

#[tauri::command]
async fn free_port(app: AppHandle, service_name: ServiceName, pid: u32, started_at: Option<String>) -> Result<FreePortResult, String> {
    blocking_result(engine_from(&app), move |engine| engine.free_port(PortOwnerExpectation { service_name, pid, started_at })).await
}

#[tauri::command]
async fn get_logs(app: AppHandle, service_name: Option<ServiceName>, limit: Option<usize>) -> Vec<LogEntry> {
    blocking_value(engine_from(&app), move |engine| engine.get_logs(LogsQuery { service_name, limit })).await
}

#[tauri::command]
async fn save_logs(app: AppHandle, service_name: Option<ServiceName>, path: Option<String>) -> Result<SaveLogsResult, String> {
    blocking_result(engine_from(&app), move |engine| engine.save_logs(SaveLogsArgs { service_name, path })).await
}

#[tauri::command]
async fn get_connection_string(app: AppHandle, service_name: ServiceName) -> String {
    blocking_value(engine_from(&app), move |engine| engine.connection_string(service_name)).await
}

#[tauri::command]
async fn clear_logs(app: AppHandle, service_name: Option<ServiceName>) -> AppSnapshot {
    blocking_value(engine_from(&app), move |engine| engine.clear_logs(service_name)).await
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

/// Build and run the AzTray process. The controller stays alive in the tray
/// while both webviews remain hidden; window close requests are converted to
/// hide operations so service processes can keep running.
pub fn run() -> tauri::Result<()> {
    tauri::Builder::default()
        .manage(AppState { engine: AppEngine::new() })
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
        .setup(|app| {
            tray::build(&app.handle())?;

            let engine = app.state::<AppState>().engine.clone();
            let handle = app.handle().clone();
            engine.set_event_handler(Some(Arc::new(move |event| {
                let result = match event {
                    EngineEvent::SnapshotUpdated(payload) => handle.emit("snapshot_updated", payload),
                    EngineEvent::ServiceUpdated(payload) => handle.emit("service_updated", payload),
                    EngineEvent::LogEntry(payload) => handle.emit("log_entry", payload),
                    EngineEvent::EngineUpdated(payload) => handle.emit("engine_updated", payload),
                };
                let _ = result;
            })));

            // This writes only the current user's startup registration. The
            // tray process itself starts at sign-in; Azurite never does.
            let autostart = app.autolaunch();
            if !autostart.is_enabled().unwrap_or(false) {
                let _ = autostart.enable();
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            match event {
                WindowEvent::CloseRequested { api, .. } => {
                    api.prevent_close();
                    let _ = window.hide();
                }
                WindowEvent::Focused(false) if window.label() == "popover" => {
                    tray::debounce_hide(&window.app_handle(), window.label());
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            set_config,
            check_engine,
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
            get_connection_string,
            clear_logs,
            quit_app
        ])
        .build(tauri::generate_context!())
        .map(|app| app.run(|_, _| {}))
}

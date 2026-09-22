use std::time::Duration;

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, Position, WebviewWindow};

const POPOVER_LABEL: &str = "popover";
const MAIN_LABEL: &str = "main";

/// Create the single native tray icon and its context menu.
pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open AzTray", true, None::<&str>)?;
    let dashboard = MenuItem::with_id(app, "dashboard", "Open dashboard", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit AzTray…", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &dashboard, &separator, &quit])?;

    let icon = Image::from_bytes(include_bytes!("../icons/32x32.png"))?;
    TrayIconBuilder::with_id("aztray-tray")
        .icon(icon)
        .tooltip("AzTray · Azurite")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => {
                let _ = toggle_popover(app);
            }
            "dashboard" => {
                let _ = show_main(app);
            }
            "quit" => {
                let _ = request_quit(app);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                position,
                ..
            } = event
            {
                let _ = toggle_popover_at(tray.app_handle(), position);
            }
        })
        .build(app)?;

    Ok(())
}

pub fn toggle_popover(app: &AppHandle) -> tauri::Result<()> {
    let window = app.get_webview_window(POPOVER_LABEL).ok_or_else(|| tauri::Error::WindowNotFound)?;
    if window.is_visible().unwrap_or(false) {
        window.hide()?;
    } else {
        show_window(&window)?;
    }
    Ok(())
}

pub fn toggle_popover_at(app: &AppHandle, tray_position: PhysicalPosition<f64>) -> tauri::Result<()> {
    let window = app.get_webview_window(POPOVER_LABEL).ok_or_else(|| tauri::Error::WindowNotFound)?;
    if window.is_visible().unwrap_or(false) {
        window.hide()?;
        return Ok(());
    }

    // Tray click coordinates are physical screen coordinates. Place the popover
    // above the tray affordance; if the taskbar is at the top this still lands
    // within the work area after the platform clamps it.
    let width = window.outer_size().map(|size| size.width as i32).unwrap_or(420);
    let height = window.outer_size().map(|size| size.height as i32).unwrap_or(640);
    let x = (tray_position.x as i32).saturating_sub(width / 2);
    let y = (tray_position.y as i32).saturating_sub(height + 10);
    window.set_position(Position::Physical(PhysicalPosition::new(x.max(0), y.max(0))))?;
    show_window(&window)
}

pub fn show_main(app: &AppHandle) -> tauri::Result<()> {
    if let Some(popover) = app.get_webview_window(POPOVER_LABEL) {
        let _ = popover.hide();
    }
    let window = app.get_webview_window(MAIN_LABEL).ok_or_else(|| tauri::Error::WindowNotFound)?;
    show_window(&window)
}

pub fn request_quit(app: &AppHandle) -> tauri::Result<()> {
    show_main(app)?;
    app.emit("quit_requested", serde_json::json!({}))?;
    Ok(())
}

fn show_window(window: &WebviewWindow) -> tauri::Result<()> {
    window.show()?;
    window.set_focus()?;
    Ok(())
}

/// Debounced blur behavior shared by the popover and dashboard. The delayed
/// check prevents a click moving focus between the tray and window from
/// producing a visible flicker.
pub fn debounce_hide(app: &AppHandle, label: &str) {
    let app = app.clone();
    let label = label.to_string();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(160));
        if let Some(window) = app.get_webview_window(&label) {
            if !window.is_focused().unwrap_or(false) {
                let _ = window.hide();
            }
        }
    });
}

#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

fn main() {
    az_tray::run().expect("failed to start AzTray");
}

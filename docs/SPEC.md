# AzTray v1 — approved behavior

AzTray is a Windows 11 x64, per-user Tauri 2 tray application for the existing
Node/npm Azurite emulator. A bundled Next.js static export renders two windows:
a 420px-class custom tray popover and a compact dashboard. The tray stays
available at sign-in; Azurite services start only on user action. Closing a
window hides it. Explicit Quit asks Stop & quit, Leave running, or Cancel.

## Visual identity

Use Winmon's near-black canvas (`#0a0a0c`), charcoal card (`#17181c`), raised
surface (`#1e2025`), luminous seams, Geist Sans/Mono, and restrained smooth
motion. Encode running/starting/broken/occupied with distinct green/amber/red/
magenta colors plus words and symbols. Stable row heights, tabular numerals,
keyboard focus, and reduced-motion support are required. The popover prioritizes
three service rows and quick actions; the dashboard prioritizes the selected
service and its live logs. Avoid monitoring charts and generic SaaS decoration.

## Services and safety

- Manage Blob, Queue, and Table on configurable host/ports (defaults 127.0.0.1,
  10000/10001/10002) with Azurite's existing user data directory. Never read,
  alter, or delete stored Blob/Queue/Table data.
- Rust owns and tracks child process identity. States are stopped, starting,
  running, broken, and port in use. Starting expires to broken after 10 seconds.
  An unowned listener remains external even when it is Azurite.
- Individual and all-service Start/Stop/Restart work from the popover or
  dashboard. Stop, Restart, Stop all, and Free port require confirmation.
  Free port names the process/PID, revalidates identity before termination,
  waits, and reports a surviving or respawned owner. Never elevate.
- Stream bounded per-service logs and a merged arrival-order view. The dashboard
  can filter/copy/save logs and copy service connection strings. External
  processes have no app-owned logs.
- Missing Node/Azurite, unavailable ports, failed launch, permission denial,
  and malformed config have explicit, actionable UI states. An executable-path
  override permits a separate per-user Azurite install.
- First launch may import host/ports/data directory from
  `%APPDATA%\azctl\config.json` without modifying it. AzTray stores its own
  settings. `azctl` installation and data remain untouched. There is no CLI.

## Packaging and acceptance

Build Next.js with `output: "export"` and package `out/` into Tauri. Release a
current-user NSIS installer with WebView2 bootstrap skipped. PowerShell and
Windows Git Bash install/uninstall scripts must work without UAC and launch the
app into the tray. Installer is app-only; Azurite is an external prerequisite.
The installed release is the acceptance target: run the scripts as a normal
user, use native UI to start all three services, see live status/logs, stop and
restart, exercise port conflict and Quit, and fix until working. Keep automated
checks to build/type-check necessities; no broad test ceremony.

## Deferred

Charts, hotkeys, notifications, advanced settings, data browsing, and scripted
CLI parity.

## Iteration 1 — work-PC readiness (2026-09-22)

An executable-path override that points to a missing or unusable Azurite
executable must produce the actionable missing-engine state before Start is
offered. The public README must explain the separate current-user Node/Azurite
prerequisite and how to set that path on a fresh Windows 11 work PC. Publish a
patch release and accept the exact GitHub-downloaded installer through the
normal install script and installed native UI.

# AzTray

AzTray is a Windows 11 tray controller for a user-local Azurite install. It
keeps the three Azurite service processes (Blob, Queue, and Table) under the
same safe ownership rules as `azctl`, with a compact tray popover and a focused
dashboard for logs, ports, connection strings, and settings.

The Tauri 2 shell packages the static Next.js export in `out/`. Closing either
window hides it and leaves the tray controller running. Azurite starts only
after an explicit action; the tray itself registers for the current user's
sign-in and never requests elevation.

## Install

PowerShell is the canonical UAC-free installer:

```powershell
irm https://raw.githubusercontent.com/iAmChumby/az-tray/main/scripts/install.ps1 | iex
```

Windows Git Bash:

```bash
bash ./scripts/install.sh
```

The scripts download the x64 current-user NSIS release, verify its SHA-256
sidecar, install under `%LOCALAPPDATA%`, and launch AzTray. Azurite remains an
external Node/npm dependency; the dashboard provides an actionable install
hint when it is missing.

## Build locally

```powershell
npm ci
npm run typecheck
npm run tauri build
```

The NSIS bundle uses `currentUser` install mode and skips the WebView2 bootstrap
installer. The release workflow publishes `az-tray-x64-setup.exe` together
with `SHA256SUMS.txt`.

## Uninstall

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\uninstall.ps1
```

or from Git Bash:

```bash
bash ./scripts/uninstall.sh
```

Uninstall removes only the AzTray application and its exact per-user startup
registration. AzTray settings, azctl configuration, and Azurite data remain.

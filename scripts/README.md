# AzTray install scripts

These scripts install and uninstall the AzTray x64 NSIS package in the current user's `%LOCALAPPDATA%` scope. They do not install Azurite, modify `azctl`, or request elevation. Azurite is a separate Node/npm prerequisite; see the [main README](../README.md#2-install-nodejs-and-azurite-in-your-user-profile).

## Install the latest release

PowerShell is the canonical entry point. From any PowerShell window:

```powershell
irm https://raw.githubusercontent.com/iAmChumby/az-tray/main/scripts/install.ps1 | iex
```

From a checkout or downloaded source archive:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install.ps1
```

The script reads the stable release metadata from `iAmChumby/az-tray`, downloads exactly one x64 installer and its unambiguous SHA-256 asset, verifies the installer, runs the NSIS package silently, and launches the installed tray app. Pin a release when needed:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install.ps1 -Version v0.1.1
```

Use `-SkipLaunch` when the install should finish with the app closed:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install.ps1 -SkipLaunch
```

## Git Bash

The Bash wrapper delegates all release, checksum, and path-safety work to PowerShell:

```bash
bash ./scripts/install.sh
bash ./scripts/install.sh --Version v0.1.1
```

Run it from a checkout or source archive so the wrapper can locate `install.ps1`. Git Bash needs Windows PowerShell (`powershell.exe`) or PowerShell 7 (`pwsh.exe`).

## Local installer harness

Use a locally built NSIS installer without contacting GitHub. Supply a SHA-256 hash or place a matching `.sha256`/`.sha256sum` sidecar beside the installer:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install.ps1 `
  -InstallerPath .\src-tauri\target\release\bundle\nsis\AzTray_0.1.1_x64-setup.exe `
  -Sha256 '<64-hex-character-hash>'
```

`-SmokeCheck` validates the requested scope and any supplied local path without downloading, installing, launching, or changing files:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install.ps1 -SmokeCheck
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\uninstall.ps1 -SmokeCheck
```

## Uninstall

PowerShell:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\uninstall.ps1
```

Git Bash:

```bash
bash ./scripts/uninstall.sh
```

The uninstaller finds an AzTray NSIS uninstaller under `%LOCALAPPDATA%`, runs it silently, and removes the exact `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\AzTray` value when it points into that user scope. It preserves `%APPDATA%\AzTray`, the existing `azctl` configuration, and all Azurite data directories; NSIS removes the `%LOCALAPPDATA%\AzTray` install directory.

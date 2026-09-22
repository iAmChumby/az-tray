# AzTray install scripts

PowerShell is the canonical installer. It downloads the latest stable x64 NSIS
release from `iAmChumby/az-tray`, verifies a matching SHA-256 release asset,
runs the installer silently, and launches AzTray in the tray. The Tauri bundle
must be configured for `currentUser` installation; these scripts never request
elevation.

Run from PowerShell:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install.ps1
```

Run from Windows Git Bash:

```bash
bash ./scripts/install.sh
```

For the fresh-install harness, pass a locally built NSIS installer. An explicit
`-Sha256` or an adjacent `.sha256`/`.sha256sum` sidecar is verified when
present; the local path remains usable for unsigned local builds when no
sidecar exists.

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install.ps1 `
  -InstallerPath .\src-tauri\target\release\bundle\nsis\AzTray_0.1.0_x64-setup.exe `
  -Sha256 '<64-hex-character-hash>'
```

The read-only pre-release smoke check validates scope and any supplied local
path without downloading, installing, or changing files:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install.ps1 -SmokeCheck
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\uninstall.ps1 -SmokeCheck
```

Uninstall is also user-scoped and silent:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\uninstall.ps1
bash ./scripts/uninstall.sh
```

The uninstall script only runs an AzTray NSIS uninstaller found under
`%LOCALAPPDATA%`, removes the exact AzTray sign-in value when it points into
that user scope, and preserves `%APPDATA%\AzTray` settings, the existing
`azctl` installation/configuration, and all Azurite data.

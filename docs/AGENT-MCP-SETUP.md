# Install and connect AzTray MCP as an agent

AzTray includes its MCP server in the Windows tray app. There is no separate MCP server package or command to keep running. When the **Local MCP** card says **Listening on this computer**, the app serves `http://127.0.0.1:47551/mcp`. Register this URL as a **Streamable HTTP** MCP server in a client on the same Windows machine.

## 1. Install the MCP-capable app

If you do not have a checkout, clone this repository in Windows PowerShell:

```powershell
git clone https://github.com/iAmChumby/az-tray.git
Set-Location .\az-tray
```

Run the remaining commands from the repository root. On a fresh machine, set up the [official Tauri Windows prerequisites](https://v2.tauri.app/start/prerequisites/) first: Node/npm, Rust with the MSVC toolchain, Microsoft C++ Build Tools, and WebView2. The commands build the current source, verify the local installer hash, install it for the current user, and launch AzTray:

```powershell
$version = (Get-Content .\package.json -Raw | ConvertFrom-Json).version
npm ci
npm run tauri build
$installer = Join-Path (Get-Location).Path "src-tauri\target\release\bundle\nsis\AzTray_${version}_x64-setup.exe"
$sha256 = (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install.ps1 -InstallerPath $installer -Sha256 $sha256
```

If an existing AzTray process is running, the install script waits for it to exit. Use **Quit AzTray → Stop & quit** in that app, then continue the installer. The app installation is separate from Node.js and Azurite; follow [the runtime setup in the README](../README.md#2-install-nodejs-and-azurite-in-your-user-profile) when the Azurite engine reports missing dependencies.

## 2. Confirm the local endpoint

Open **AzTray Dashboard → Settings → Local MCP**. It must say **Listening on this computer** and display `http://127.0.0.1:47551/mcp`. Keep AzTray running while using the client.

If the card says **Endpoint unavailable**, read its error. Another process may own port `47551`. Resolve that conflict and restart AzTray; the MCP listener binds when the app starts. The URL is loopback-only, so a client running on another computer or inside an isolated container cannot reach it through that address.

## 3. Register the server in your MCP client

For Codex, run:

```powershell
codex mcp add aztray --url http://127.0.0.1:47551/mcp
codex mcp get aztray
```

If `aztray` is already registered, run `codex mcp get aztray` first. Keep it when the URL matches. If it points elsewhere, run `codex mcp remove aztray` and then the `add` command above.

For another client, add a server named `aztray`, choose **Streamable HTTP**, and use `http://127.0.0.1:47551/mcp` as the server URL. Configure a URL-based connection, since the AzTray app hosts the server.

Open a new agent session if the client does not refresh its tool catalog immediately. Ask the agent to call `aztray_mcp_status` and `aztray_snapshot`. A working connection returns `active: true`, the same endpoint URL, and the current service states. The client can list the remaining tools and their argument schemas through `tools/list`.

## Operating boundary

The MCP tools manage AzTray's Azurite services, configuration, port ownership, logs, connection strings, and app lifecycle. They do not browse or edit stored Blob, Queue, or Table records. For `aztray_free_port`, inspect `aztray_identify_port_owner` first and get the user's approval before passing the observed PID, process start time, and `confirmed: true`. AzTray rechecks that identity before releasing the port.

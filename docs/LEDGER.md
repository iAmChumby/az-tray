# AzTray delivery ledger

## v0.1.0 — initial installed release

- Built the Next.js static export and Tauri current-user NSIS bundle.
- Published source and a public release with a SHA-256 checksum.
- Accepted the installed app in native Windows UI: all three services started,
  live logs appeared, restart and stop worked, a port conflict was handled,
  and Quit terminated app-owned services.

## Work-PC readiness iteration

- Fixed a nonexistent executable-path override that appeared Ready until
  Start was clicked. Installed v0.1.1 showed Missing Azurite, actionable
  guidance, and disabled Start; restoring the real path returned to Ready.
- Clarified separate user-space Node/Azurite installation in the public docs.
- Installed the local v0.1.1 NSIS bundle through `scripts/install.ps1` with a
  verified SHA-256 under `AdminToken=False`, without a UAC prompt.
- In the installed native UI, Start all reached 3/3 Running and merged logs
  showed Blob, Queue, and Table listeners. Restarting Blob changed its listener
  PID while Queue and Table stayed running. An external temporary Azurite Blob
  listener was identified by name/PID in the Free port confirmation and
  released. Stop & quit exited the tray and cleared all three default ports.
- Final source adds the missing-engine banner to dashboard Services as well as
  Settings and the popover. Rebuild and GitHub-downloaded release acceptance
  remain the final publication gates.

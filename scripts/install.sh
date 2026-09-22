#!/usr/bin/env bash
set -euo pipefail

# Git Bash entry point. PowerShell owns release selection, SHA-256 verification,
# repeat-run upgrade handling, and install-path/process safety.
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

if command -v pwsh.exe >/dev/null 2>&1; then
  powershell="$(command -v pwsh.exe)"
elif command -v powershell.exe >/dev/null 2>&1; then
  powershell="$(command -v powershell.exe)"
elif command -v pwsh >/dev/null 2>&1; then
  powershell="$(command -v pwsh)"
else
  echo "AzTray requires PowerShell (pwsh or Windows PowerShell)." >&2
  exit 127
fi

convert_path() {
  local value="$1"
  if command -v cygpath >/dev/null 2>&1 && [[ "$value" == /* ]]; then
    cygpath -w -- "$value"
  elif command -v wslpath >/dev/null 2>&1 && [[ "$value" == /* ]]; then
    wslpath -w -- "$value"
  else
    printf '%s' "$value"
  fi
}

ps_script="$(convert_path "$script_dir/install.ps1")"

converted_args=()
while (($# > 0)); do
  case "$1" in
    -InstallerPath|--InstallerPath|-Path|--path|-installerpath|--installerpath)
      key="$1"
      shift
      if (($# == 0)); then
        echo "$key requires a path." >&2
        exit 2
      fi
      converted_args+=("$key" "$(convert_path "$1")")
      shift
      ;;
    -InstallerPath=*|--InstallerPath=*|-installerpath=*|--installerpath=*)
      key="${1%%=*}"
      value="${1#*=}"
      converted_args+=("$key=$(convert_path "$value")")
      shift
      ;;
    *)
      converted_args+=("$1")
      shift
      ;;
  esac
done

exec "$powershell" -NoLogo -NoProfile -ExecutionPolicy Bypass -File "$ps_script" "${converted_args[@]}"

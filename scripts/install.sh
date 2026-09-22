#!/usr/bin/env bash
set -euo pipefail

# Git Bash entry point. PowerShell owns the release/download and path-safety logic.
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

if command -v cygpath >/dev/null 2>&1; then
  ps_script="$(cygpath -w -- "$script_dir/install.ps1")"
else
  ps_script="$script_dir/install.ps1"
fi

convert_path() {
  local value="$1"
  if command -v cygpath >/dev/null 2>&1 && [[ "$value" == /* ]]; then
    cygpath -w -- "$value"
  else
    printf '%s' "$value"
  fi
}

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

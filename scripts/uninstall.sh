#!/usr/bin/env bash
set -euo pipefail

# Git Bash entry point. PowerShell owns the exact per-user uninstall scope.
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
  ps_script="$(cygpath -w -- "$script_dir/uninstall.ps1")"
else
  ps_script="$script_dir/uninstall.ps1"
fi

converted_args=()
while (($# > 0)); do
  case "$1" in
    -UninstallerPath|--UninstallerPath|-uninstallerpath|--uninstallerpath)
      key="$1"
      shift
      if (($# == 0)); then
        echo "$key requires a path." >&2
        exit 2
      fi
      if command -v cygpath >/dev/null 2>&1 && [[ "$1" == /* ]]; then
        converted_args+=("$key" "$(cygpath -w -- "$1")")
      else
        converted_args+=("$key" "$1")
      fi
      shift
      ;;
    -UninstallerPath=*|--UninstallerPath=*|-uninstallerpath=*|--uninstallerpath=*)
      key="${1%%=*}"
      value="${1#*=}"
      if command -v cygpath >/dev/null 2>&1 && [[ "$value" == /* ]]; then
        value="$(cygpath -w -- "$value")"
      fi
      converted_args+=("$key=$value")
      shift
      ;;
    *)
      converted_args+=("$1")
      shift
      ;;
  esac
done

exec "$powershell" -NoLogo -NoProfile -ExecutionPolicy Bypass -File "$ps_script" "${converted_args[@]}"

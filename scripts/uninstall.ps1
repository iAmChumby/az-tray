[CmdletBinding()]
param(
    [string]$UninstallerPath,

    [switch]$SmokeCheck
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Write-Info {
    param([Parameter(Mandatory = $true)][string]$Message)

    Write-Host "[AzTray] $Message"
}

function Get-LocalAppDataRoot {
    if ([string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
        throw 'LOCALAPPDATA is not set; AzTray requires a Windows per-user install scope.'
    }

    return [IO.Path]::GetFullPath($env:LOCALAPPDATA)
}

function Test-PathWithin {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Root
    )

    $fullPath = [IO.Path]::GetFullPath($Path)
    $fullRoot = [IO.Path]::GetFullPath($Root).TrimEnd('\')
    return $fullPath.Equals($fullRoot, [StringComparison]::OrdinalIgnoreCase) -or
        $fullPath.StartsWith("$fullRoot\", [StringComparison]::OrdinalIgnoreCase)
}

function Get-InstallDirectories {
    param([Parameter(Mandatory = $true)][string]$LocalRoot)

    $directories = New-Object System.Collections.Generic.List[string]
    foreach ($directory in @(
            (Join-Path $LocalRoot 'Programs\AzTray'),
            (Join-Path $LocalRoot 'Programs\az-tray'),
            (Join-Path $LocalRoot 'AzTray'),
            (Join-Path $LocalRoot 'az-tray')
        )) {
        if (-not $directories.Contains($directory)) {
            $directories.Add($directory)
        }
    }

    $uninstallRoot = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall'
    if (Test-Path -LiteralPath $uninstallRoot) {
        foreach ($key in @(Get-ChildItem -LiteralPath $uninstallRoot -ErrorAction SilentlyContinue)) {
            $properties = Get-ItemProperty -LiteralPath $key.PSPath -ErrorAction SilentlyContinue
            if ($null -eq $properties) {
                continue
            }

            if ([string]$properties.DisplayName -notmatch '(?i)^AzTray(?:\s|$)') {
                continue
            }

            $installLocation = [string]$properties.InstallLocation
            if ([string]::IsNullOrWhiteSpace($installLocation)) {
                continue
            }

            $installLocation = [Environment]::ExpandEnvironmentVariables($installLocation.Trim('"'))
            $locationIsInScope = Test-PathWithin -Path $installLocation -Root $LocalRoot
            if ($locationIsInScope -and -not $directories.Contains($installLocation)) {
                $directories.Insert(0, $installLocation)
            }
        }
    }

    return @($directories)
}

function Get-AzTrayUninstaller {
    param([Parameter(Mandatory = $true)][string]$LocalRoot)

    $directories = Get-InstallDirectories -LocalRoot $LocalRoot
    foreach ($directory in $directories) {
        if (-not (Test-PathWithin -Path $directory -Root $LocalRoot)) {
            continue
        }

        foreach ($name in @('uninstall.exe', 'unins000.exe', 'Uninstall AzTray.exe')) {
            $candidate = Join-Path $directory $name
            if (Test-Path -LiteralPath $candidate -PathType Leaf) {
                return (Get-Item -LiteralPath $candidate).FullName
            }
        }
    }

    throw "AzTray's per-user uninstaller was not found under '$LocalRoot'."
}

function Remove-AzTrayStartupValue {
    param([Parameter(Mandatory = $true)][string]$LocalRoot)

    $runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
    if (-not (Test-Path -LiteralPath $runKey)) {
        return
    }

    $property = Get-ItemProperty -LiteralPath $runKey -Name AzTray -ErrorAction SilentlyContinue
    if ($null -eq $property) {
        return
    }

    $value = [string]$property.AzTray
    if ($value -match '(?i)az[-_]?tray\.exe' -and
        $value.IndexOf($LocalRoot, [StringComparison]::OrdinalIgnoreCase) -ge 0) {
        Remove-ItemProperty -LiteralPath $runKey -Name AzTray -ErrorAction Stop
        Write-Info 'Removed the exact AzTray per-user startup value.'
    }
}

$localRoot = Get-LocalAppDataRoot
$roamingRoot = [Environment]::GetFolderPath('ApplicationData')
if ([string]::IsNullOrWhiteSpace($roamingRoot)) {
    $roamingRoot = $env:APPDATA
}
$uninstallerFullPath = $null

try {
    Write-Info 'Using current-user scope; this script never requests elevation.'
    Write-Info "LOCALAPPDATA: $localRoot"

    if ($UninstallerPath) {
        $uninstallerFullPath = [IO.Path]::GetFullPath($UninstallerPath)
        if (-not (Test-Path -LiteralPath $uninstallerFullPath -PathType Leaf)) {
            throw "Uninstaller path does not exist: $uninstallerFullPath"
        }
        if ([IO.Path]::GetFileName($uninstallerFullPath) -notmatch '(?i)^(?:uninstall|unins\d+)\.exe$') {
            throw "Uninstaller path must be an AzTray NSIS uninstaller: $uninstallerFullPath"
        }
        if (-not (Test-PathWithin -Path $uninstallerFullPath -Root $localRoot)) {
            throw "Refusing to run an uninstaller outside LOCALAPPDATA: $uninstallerFullPath"
        }
    }
    elseif (-not $SmokeCheck) {
        $uninstallerFullPath = Get-AzTrayUninstaller -LocalRoot $localRoot
    }

    if ($SmokeCheck) {
        if ($uninstallerFullPath) {
            Write-Info "Smoke check uninstaller: $uninstallerFullPath"
        }
        else {
            Write-Info "Smoke check: no AzTray installation is present under '$localRoot'."
        }
        Write-Info "Preserved user paths: $(Join-Path $roamingRoot 'AzTray') and all Azurite data directories."
        Write-Info 'Smoke check passed. No uninstaller was executed and no files were changed.'
        exit 0
    }

    Write-Info "Running the per-user NSIS uninstaller silently: $uninstallerFullPath"
    $uninstallerProcess = Start-Process -FilePath $uninstallerFullPath -ArgumentList '/S' -Wait -PassThru
    if ($uninstallerProcess.ExitCode -ne 0) {
        throw "AzTray uninstaller exited with code $($uninstallerProcess.ExitCode). Close AzTray and retry."
    }

    Remove-AzTrayStartupValue -LocalRoot $localRoot
    Write-Info 'AzTray binaries were removed by the user-scope uninstaller.'
    Write-Info "Preserved user paths: $(Join-Path $roamingRoot 'AzTray') and all Azurite data directories."
}
catch {
    Write-Error "[AzTray] $($_.Exception.Message)"
    exit 1
}

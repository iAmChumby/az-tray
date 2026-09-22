[CmdletBinding()]
param(
    [Alias('Path', 'Installer')]
    [string]$InstallerPath,

    [string]$Sha256,

    [string]$Version = 'latest',

    [switch]$SkipLaunch,

    [switch]$SmokeCheck
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$script:Repository = 'iAmChumby/az-tray'
$script:AppName = 'AzTray'
$script:ReleaseApi = "https://api.github.com/repos/$($script:Repository)/releases"
$script:ExpectedInstallDirectoryName = 'AzTray'
$script:ExpectedExecutableName = 'az-tray.exe'

function Write-Info {
    param([Parameter(Mandatory = $true)][string]$Message)

    Write-Host "[AzTray] $Message"
}

function Write-Warn {
    param([Parameter(Mandatory = $true)][string]$Message)

    Write-Warning "[AzTray] $Message"
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

function Get-GitHubHeaders {
    return @{
        Accept = 'application/vnd.github+json'
        'User-Agent' = 'AzTrayInstaller/1.0'
    }
}

function Get-Release {
    param([Parameter(Mandatory = $true)][string]$RequestedVersion)

    if ($RequestedVersion -eq 'latest') {
        $uri = "$($script:ReleaseApi)/latest"
    }
    else {
        $encodedVersion = [Uri]::EscapeDataString($RequestedVersion)
        $uri = "$($script:ReleaseApi)/tags/$encodedVersion"
    }

    Write-Info "Reading release metadata from $uri"
    $release = Invoke-RestMethod -Uri $uri -Headers (Get-GitHubHeaders) -Method Get -UseBasicParsing

    if ($release.draft -or $release.prerelease) {
        throw "Release '$RequestedVersion' is not a stable release."
    }

    return $release
}

function Select-ReleaseAssets {
    param([Parameter(Mandatory = $true)]$Release)

    $assets = @($Release.assets)
    $installerAssets = @($assets | Where-Object {
            $_.name -match '(?i)^az[-_]?tray.*(?:x64|win64|windows).*?(?:setup|installer).*\.exe$'
        })

    if ($installerAssets.Count -eq 0) {
        $installerAssets = @($assets | Where-Object {
                $_.name -match '(?i)(?:x64|win64|windows).*?(?:setup|installer).*\.exe$'
            })
    }

    if ($installerAssets.Count -ne 1) {
        $names = ($assets | ForEach-Object { $_.name }) -join ', '
        throw "Expected one x64 NSIS installer asset for AzTray; found $($installerAssets.Count). Release assets: $names"
    }

    $installer = $installerAssets[0]
    $preferredChecksumNames = @(
        "$($installer.name).sha256",
        "$($installer.name).sha256sum"
    )
    $checksumAssets = @($assets | Where-Object {
            $_.name -match '(?i)^(?:sha256sums?|checksums?)(?:\.[a-z0-9._-]+)?$' -or
            $_.name -match '(?i)\.(?:sha256|sha256sum|sum)$'
        })

    $checksum = $null
    foreach ($preferredName in $preferredChecksumNames) {
        $preferred = @($checksumAssets | Where-Object { $_.name -ieq $preferredName })
        if ($preferred.Count -eq 1) {
            $checksum = $preferred[0]
            break
        }
    }

    if ($null -eq $checksum -and $checksumAssets.Count -eq 1) {
        $checksum = $checksumAssets[0]
    }

    if ($null -eq $checksum) {
        $names = ($assets | ForEach-Object { $_.name }) -join ', '
        throw "The release has no unambiguous SHA-256 asset for $($installer.name). Assets: $names"
    }

    return [pscustomobject]@{
        Installer = $installer
        Checksum = $checksum
    }
}

function Get-ExpectedHashFromText {
    param(
        [Parameter(Mandatory = $true)][string]$Text,
        [Parameter(Mandatory = $true)][string]$TargetName
    )

    $targetLeaf = [IO.Path]::GetFileName($TargetName)
    $lineMatches = [regex]::Matches(
        $Text,
        '(?im)^\s*([a-f0-9]{64})\s+\*?(?<name>.+?)\s*$'
    )

    foreach ($match in $lineMatches) {
        $listedName = $match.Groups['name'].Value.Trim().Trim('"')
        if ([IO.Path]::GetFileName($listedName) -ieq $targetLeaf) {
            return $match.Groups[1].Value.ToLowerInvariant()
        }
    }

    $bareHash = [regex]::Match($Text, '(?im)^\s*([a-f0-9]{64})\s*$')
    if ($bareHash.Success -and $lineMatches.Count -eq 0) {
        return $bareHash.Groups[1].Value.ToLowerInvariant()
    }

    if ($lineMatches.Count -eq 1) {
        return $lineMatches[0].Groups[1].Value.ToLowerInvariant()
    }

    throw "Could not find a SHA-256 entry for '$targetLeaf'."
}

function Assert-InstallerHash {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$ExpectedHash
    )

    if ($ExpectedHash -notmatch '^[a-fA-F0-9]{64}$') {
        throw 'Sha256 must be exactly 64 hexadecimal characters.'
    }

    $actualHash = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualHash -ne $ExpectedHash.ToLowerInvariant()) {
        throw "SHA-256 mismatch for '$Path'. Expected $ExpectedHash, received $actualHash."
    }

    Write-Info "SHA-256 verified: $actualHash"
}

function Get-ExpectedAzTrayExecutablePath {
    param([Parameter(Mandatory = $true)][string]$LocalRoot)

    $installDirectory = Join-Path $LocalRoot $script:ExpectedInstallDirectoryName
    if (-not (Test-PathWithin -Path $installDirectory -Root $LocalRoot)) {
        throw "The expected AzTray install directory is outside LOCALAPPDATA: $installDirectory"
    }

    try {
        $directoryExists = Test-Path -LiteralPath $installDirectory -PathType Container -ErrorAction Stop
    }
    catch {
        throw "Unable to inspect the expected AzTray install directory safely: $($_.Exception.Message)"
    }

    if ($directoryExists) {
        try {
            $directoryInfo = Get-Item -LiteralPath $installDirectory -ErrorAction Stop
        }
        catch {
            throw "Unable to inspect the expected AzTray install directory safely: $($_.Exception.Message)"
        }

        if ($directoryInfo.Attributes -band [IO.FileAttributes]::ReparsePoint) {
            throw "Refusing to use a reparse-point AzTray install directory: $installDirectory"
        }
    }

    return [IO.Path]::GetFullPath((Join-Path $installDirectory $script:ExpectedExecutableName))
}

function Assert-ExpectedAzTrayRegistration {
    param(
        [Parameter(Mandatory = $true)][string]$LocalRoot,
        [Parameter(Mandatory = $true)][string]$ExpectedInstallDirectory
    )

    $uninstallRoot = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall'
    if (-not (Test-Path -LiteralPath $uninstallRoot)) {
        return
    }

    try {
        $keys = @(Get-ChildItem -LiteralPath $uninstallRoot -ErrorAction Stop)
    }
    catch {
        throw "Unable to inspect the current-user AzTray installation registration safely: $($_.Exception.Message)"
    }

    $registrations = @()
    foreach ($key in $keys) {
        try {
            $properties = Get-ItemProperty -LiteralPath $key.PSPath -ErrorAction Stop
        }
        catch {
            throw "Unable to inspect uninstall registration '$($key.PSChildName)' safely: $($_.Exception.Message)"
        }

        $displayName = if ($null -ne $properties.PSObject.Properties['DisplayName']) {
            [string]$properties.DisplayName
        }
        else {
            ''
        }
        if ($displayName -notmatch '(?i)^AzTray(?:\s|$)') {
            continue
        }

        $installLocation = if ($null -ne $properties.PSObject.Properties['InstallLocation']) {
            [string]$properties.InstallLocation
        }
        else {
            ''
        }
        if ([string]::IsNullOrWhiteSpace($installLocation)) {
            throw "AzTray uninstall registration '$($key.PSChildName)' has no install location; refusing an ambiguous upgrade."
        }

        try {
            $normalizedLocation = [IO.Path]::GetFullPath([Environment]::ExpandEnvironmentVariables($installLocation.Trim('"'))).TrimEnd('\')
        }
        catch {
            throw "AzTray uninstall registration '$($key.PSChildName)' has an invalid install location; refusing an ambiguous upgrade."
        }

        $normalizedExpected = [IO.Path]::GetFullPath($ExpectedInstallDirectory).TrimEnd('\')
        if (-not $normalizedLocation.Equals($normalizedExpected, [StringComparison]::OrdinalIgnoreCase) -or
            -not (Test-PathWithin -Path $normalizedLocation -Root $LocalRoot)) {
            throw "AzTray is registered at '$normalizedLocation', but this installer only accepts '$normalizedExpected'; refusing to update an ambiguous installation."
        }

        $registrations += $key.PSChildName
    }

    if ($registrations.Count -gt 1) {
        throw "Multiple AzTray uninstall registrations were found ($($registrations -join ', ')); refusing an ambiguous upgrade."
    }
}

function Get-AzTrayExecutable {
    param([Parameter(Mandatory = $true)][string]$LocalRoot)

    $candidate = Get-ExpectedAzTrayExecutablePath -LocalRoot $LocalRoot
    if (-not (Test-Path -LiteralPath $candidate -PathType Leaf)) {
        throw "AzTray installed, but its executable was not found at the expected path '$candidate'."
    }

    $fileInfo = Get-Item -LiteralPath $candidate -ErrorAction Stop
    if ($fileInfo.Attributes -band [IO.FileAttributes]::ReparsePoint) {
        throw "Refusing to launch a reparse-point AzTray executable: $candidate"
    }

    return $fileInfo.FullName
}

function Get-AzTrayRunningProcess {
    param([Parameter(Mandatory = $true)][string]$ExpectedExecutablePath)

    $expectedPath = [IO.Path]::GetFullPath($ExpectedExecutablePath)
    try {
        $candidates = @(Get-CimInstance -ClassName Win32_Process -Filter "Name = '$($script:ExpectedExecutableName)'" -ErrorAction Stop)
    }
    catch {
        throw "Unable to inspect AzTray processes safely: $($_.Exception.Message). Close AzTray from its tray menu and retry."
    }

    $found = @()
    foreach ($candidate in $candidates) {
        $processId = [int]$candidate.ProcessId
        $rawPath = [string]$candidate.ExecutablePath
        if ([string]::IsNullOrWhiteSpace($rawPath)) {
            throw "Windows did not report the executable path for az-tray.exe (PID $processId); refusing to guess which process is safe to update."
        }

        try {
            $processPath = [IO.Path]::GetFullPath([Environment]::ExpandEnvironmentVariables($rawPath.Trim('"')))
        }
        catch {
            throw "Windows reported an invalid executable path for az-tray.exe (PID $processId); refusing to guess which process is safe to update."
        }

        if (-not $processPath.Equals($expectedPath, [StringComparison]::OrdinalIgnoreCase)) {
            throw "Found az-tray.exe (PID $processId) at '$processPath', but the expected AzTray path is '$expectedPath'; close the ambiguous process and retry."
        }

        $found += $candidate
    }

    if ($found.Count -gt 1) {
        $processIds = ($found | ForEach-Object { [int]$_.ProcessId }) -join ', '
        throw "Multiple AzTray processes are running at '$expectedPath' (PIDs $processIds); refusing an ambiguous upgrade."
    }

    return @($found)
}

function Wait-ForAzTrayExit {
    param([Parameter(Mandatory = $true)][string]$ExpectedExecutablePath)

    $running = @(Get-AzTrayRunningProcess -ExpectedExecutablePath $ExpectedExecutablePath)
    if ($running.Count -eq 0) {
        return
    }

    $processId = [int]$running[0].ProcessId
    Write-Warn "AzTray is running from the expected user install path (PID $processId)."
    Write-Host 'Use the AzTray tray menu and choose Quit AzTray, then select Stop & quit so the app-owned Azurite processes close through their ownership-aware shutdown path.'
    Write-Host 'After the tray process exits, return here and press Enter. This installer waits for a clean exit and leaves AzTray and its child processes intact.'
    $answer = Read-Host 'Press Enter to continue, or type Q to cancel the upgrade'
    if ($answer -match '^(?i:q|quit|cancel)$') {
        throw 'AzTray upgrade cancelled. The running app and its Azurite services were left untouched.'
    }

    $deadline = (Get-Date).AddSeconds(30)
    do {
        $running = @(Get-AzTrayRunningProcess -ExpectedExecutablePath $ExpectedExecutablePath)
        if ($running.Count -eq 0) {
            Write-Info 'AzTray exited cleanly; continuing with the update.'
            return
        }

        if ((Get-Date) -ge $deadline) {
            break
        }

        Start-Sleep -Milliseconds 250
    } while ($true)

    throw "AzTray is still running at '$ExpectedExecutablePath'. Quit it from the tray menu and rerun the install command; the installer did not start and no process was terminated."
}

function Remove-WorkDirectory {
    param([string]$Path)

    if ([string]::IsNullOrWhiteSpace($Path) -or -not (Test-Path -LiteralPath $Path)) {
        return
    }

    $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    $leaf = [IO.Path]::GetFileName($Path)
    if ((Test-PathWithin -Path $Path -Root $tempRoot) -and
        $leaf -match '^AzTrayInstall-[0-9a-fA-F-]{36}$') {
        Remove-Item -LiteralPath $Path -Recurse -Force -ErrorAction SilentlyContinue
    }
}

$localRoot = Get-LocalAppDataRoot
$expectedExecutablePath = Get-ExpectedAzTrayExecutablePath -LocalRoot $localRoot
$expectedInstallDirectory = [IO.Path]::GetDirectoryName($expectedExecutablePath)
$workDirectory = $null
$installerFullPath = $null

try {
    Write-Info 'Using current-user scope; this script requires no elevation.'
    Write-Info "LOCALAPPDATA: $localRoot"

    if ($SmokeCheck) {
        if ($InstallerPath) {
            $smokePath = [IO.Path]::GetFullPath($InstallerPath)
            if (-not (Test-Path -LiteralPath $smokePath -PathType Leaf)) {
                throw "Smoke check installer path does not exist: $smokePath"
            }
            if ([IO.Path]::GetExtension($smokePath) -ine '.exe') {
                throw "Smoke check installer path is not an executable: $smokePath"
            }
            Write-Info "Smoke check installer: $smokePath"
        }

        Write-Info 'Smoke check passed. No installer was executed and no files were changed.'
        exit 0
    }

    Assert-ExpectedAzTrayRegistration -LocalRoot $localRoot -ExpectedInstallDirectory $expectedInstallDirectory

    if ($InstallerPath) {
        $installerFullPath = [IO.Path]::GetFullPath($InstallerPath)
        if (-not (Test-Path -LiteralPath $installerFullPath -PathType Leaf)) {
            throw "Installer path does not exist: $installerFullPath"
        }
        if ([IO.Path]::GetExtension($installerFullPath) -ine '.exe') {
            throw "Installer path must point to an NSIS .exe: $installerFullPath"
        }

        if ($Sha256) {
            Assert-InstallerHash -Path $installerFullPath -ExpectedHash $Sha256
        }
        else {
            foreach ($sidecar in @("$installerFullPath.sha256", "$installerFullPath.sha256sum")) {
                if (Test-Path -LiteralPath $sidecar -PathType Leaf) {
                    $expected = Get-ExpectedHashFromText -Text (Get-Content -LiteralPath $sidecar -Raw) -TargetName $installerFullPath
                    Assert-InstallerHash -Path $installerFullPath -ExpectedHash $expected
                    break
                }
            }
        }

        Write-Info "Using local installer: $installerFullPath"
    }
    else {
        $release = Get-Release -RequestedVersion $Version
        $selectedAssets = Select-ReleaseAssets -Release $release
        $workDirectory = Join-Path ([IO.Path]::GetTempPath()) ("AzTrayInstall-" + [guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $workDirectory -Force | Out-Null
        $installerFullPath = Join-Path $workDirectory 'AzTray-setup.exe'
        $checksumPath = Join-Path $workDirectory 'checksums.txt'

        Write-Info "Downloading $($selectedAssets.Installer.name)"
        Invoke-WebRequest -Uri $selectedAssets.Installer.browser_download_url -Headers (Get-GitHubHeaders) -OutFile $installerFullPath -UseBasicParsing
        Write-Info "Downloading $($selectedAssets.Checksum.name)"
        Invoke-WebRequest -Uri $selectedAssets.Checksum.browser_download_url -Headers (Get-GitHubHeaders) -OutFile $checksumPath -UseBasicParsing

        $expected = Get-ExpectedHashFromText -Text (Get-Content -LiteralPath $checksumPath -Raw) -TargetName $selectedAssets.Installer.name
        Assert-InstallerHash -Path $installerFullPath -ExpectedHash $expected
    }

    Wait-ForAzTrayExit -ExpectedExecutablePath $expectedExecutablePath
    Write-Info 'Running the NSIS installer silently in the current-user scope.'
    $installerProcess = Start-Process -FilePath $installerFullPath -ArgumentList '/S' -Wait -PassThru
    if ($installerProcess.ExitCode -ne 0) {
        throw "AzTray installer exited with code $($installerProcess.ExitCode)."
    }

    if ($SkipLaunch) {
        Write-Info 'Install completed. Launch skipped by request.'
        exit 0
    }

    $executable = Get-AzTrayExecutable -LocalRoot $localRoot
    Write-Info "Launching installed app: $executable"
    Start-Process -FilePath $executable -WorkingDirectory ([IO.Path]::GetDirectoryName($executable)) | Out-Null
    Write-Info 'AzTray is running in the tray.'
}
catch {
    Write-Error "[AzTray] $($_.Exception.Message)"
    throw
}
finally {
    Remove-WorkDirectory -Path $workDirectory
}

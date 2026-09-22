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

function Get-AzTrayExecutable {
    param([Parameter(Mandatory = $true)][string]$LocalRoot)

    $candidateDirectories = New-Object System.Collections.Generic.List[string]
    foreach ($directory in @(
            (Join-Path $LocalRoot 'Programs\AzTray'),
            (Join-Path $LocalRoot 'Programs\az-tray'),
            (Join-Path $LocalRoot 'AzTray'),
            (Join-Path $LocalRoot 'az-tray')
        )) {
        if (-not $candidateDirectories.Contains($directory)) {
            $candidateDirectories.Add($directory)
        }
    }

    $uninstallRoot = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall'
    if (Test-Path -LiteralPath $uninstallRoot) {
        foreach ($key in @(Get-ChildItem -LiteralPath $uninstallRoot -ErrorAction SilentlyContinue)) {
            $properties = Get-ItemProperty -LiteralPath $key.PSPath -ErrorAction SilentlyContinue
            if ($null -eq $properties) {
                continue
            }

            $displayName = [string]$properties.DisplayName
            if ($displayName -notmatch '(?i)^AzTray(?:\s|$)') {
                continue
            }

            $installLocation = [string]$properties.InstallLocation
            if ([string]::IsNullOrWhiteSpace($installLocation)) {
                continue
            }

            $installLocation = [Environment]::ExpandEnvironmentVariables($installLocation.Trim('"'))
            $locationIsInScope = Test-PathWithin -Path $installLocation -Root $LocalRoot
            if ($locationIsInScope -and -not $candidateDirectories.Contains($installLocation)) {
                $candidateDirectories.Insert(0, $installLocation)
            }
        }
    }

    foreach ($directory in $candidateDirectories) {
        if (-not (Test-PathWithin -Path $directory -Root $LocalRoot)) {
            continue
        }

        foreach ($name in @('AzTray.exe', 'az-tray.exe', 'aztray.exe')) {
            $candidate = Join-Path $directory $name
            if (Test-Path -LiteralPath $candidate -PathType Leaf) {
                return (Get-Item -LiteralPath $candidate).FullName
            }
        }
    }

    throw "AzTray installed, but its executable could not be found safely under '$LocalRoot'."
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
$workDirectory = $null
$installerFullPath = $null

try {
    Write-Info 'Using current-user scope; this script never requests elevation.'
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

    Write-Info 'Running the NSIS installer silently.'
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
finally {
    Remove-WorkDirectory -Path $workDirectory
}

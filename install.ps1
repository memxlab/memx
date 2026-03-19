param(
    [string]$InstallDir = $(if ($env:MEMX_INSTALL_DIR) {
            $env:MEMX_INSTALL_DIR
        }
        elseif ($env:MEMX_HOME) {
            Join-Path $env:MEMX_HOME "bin"
        }
        else {
            Join-Path $env:LOCALAPPDATA "MemX\bin"
        }),
    [switch]$SkipSetup,
    [switch]$Uninstall,
    [switch]$Yes
)

$ErrorActionPreference = "Stop"

$Repo = if ($env:MEMX_REPO) { $env:MEMX_REPO } else { "memxlab/memx" }
$Version = if ($env:MEMX_VERSION) { $env:MEMX_VERSION } else { "latest" }
$DownloadBaseUrl = $env:MEMX_DOWNLOAD_BASE_URL
$BinaryName = "memx.exe"
$MemxHome = if ($env:MEMX_HOME) { $env:MEMX_HOME } else { Join-Path $HOME ".memx" }
$ShouldUninstall = $Uninstall -or ($env:MEMX_UNINSTALL -eq "1")
$AssumeYes = $Yes -or ($env:MEMX_YES -eq "1") -or ($env:MEMX_UNINSTALL_YES -eq "1")

function Write-Step {
    param([string]$Message)
    Write-Host "== $Message" -ForegroundColor Green
}

function Get-AssetName {
    $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()

    switch ($arch) {
        "x64" { return "memx-windows-x86_64.zip" }
        "arm64" { throw "Windows arm64 artifacts are not published yet." }
        default { throw "Unsupported Windows architecture: $arch" }
    }
}

function Get-DownloadUrl {
    param([string]$AssetName)

    if ($DownloadBaseUrl) {
        return "$($DownloadBaseUrl.TrimEnd('/'))/$AssetName"
    }

    if ($Version -eq "latest") {
        return "https://github.com/$Repo/releases/latest/download/$AssetName"
    }

    return "https://github.com/$Repo/releases/download/$Version/$AssetName"
}

function Add-UserPath {
    param([string]$PathEntry)

    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $entries = @()
    if ($userPath) {
        $entries = $userPath -split ";" | Where-Object { $_ }
    }

    if ($entries -contains $PathEntry) {
        if (($env:Path -split ";") -notcontains $PathEntry) {
            $env:Path = "$PathEntry;$env:Path"
        }
        return $false
    }

    $entries += $PathEntry
    [Environment]::SetEnvironmentVariable("Path", ($entries -join ";"), "User")
    $env:Path = "$PathEntry;$env:Path"
    return $true
}

function Remove-UserPath {
    param([string]$PathEntry)

    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if (-not $userPath) {
        return $false
    }

    $originalEntries = $userPath -split ";" | Where-Object { $_ }
    $entries = $originalEntries | Where-Object { $_ -ne $PathEntry }
    if (($entries.Count -eq $originalEntries.Count) -and (($env:Path -split ";") -notcontains $PathEntry)) {
        return $false
    }

    [Environment]::SetEnvironmentVariable("Path", ($entries -join ";"), "User")
    $env:Path = (($env:Path -split ";" | Where-Object { $_ -and $_ -ne $PathEntry }) -join ";")
    return $true
}

function Test-InteractiveConsole {
    try {
        return (-not [Console]::IsInputRedirected) -and (-not [Console]::IsOutputRedirected)
    }
    catch {
        return $true
    }
}

function Get-InstallDirCandidates {
    param([string]$PreferredDir)

    $candidates = New-Object System.Collections.Generic.List[string]

    foreach ($candidate in @(
            $PreferredDir,
            $env:MEMX_INSTALL_DIR,
            $(if ($env:MEMX_HOME) { Join-Path $env:MEMX_HOME "bin" } else { $null }),
            (Join-Path $env:LOCALAPPDATA "MemX\bin"),
            (Join-Path $HOME ".local\bin")
        )) {
        if (-not [string]::IsNullOrWhiteSpace($candidate) -and -not $candidates.Contains($candidate)) {
            $candidates.Add($candidate)
        }
    }

    try {
        $command = Get-Command $BinaryName -ErrorAction Stop
        if ($command.CommandType -eq "Application") {
            $commandDir = Split-Path $command.Path -Parent
            if (-not $candidates.Contains($commandDir)) {
                $candidates.Add($commandDir)
            }
        }
    }
    catch {
    }

    return $candidates
}

function Find-InstalledBinary {
    param([string]$PreferredDir)

    foreach ($dir in Get-InstallDirCandidates -PreferredDir $PreferredDir) {
        $candidate = Join-Path $dir $BinaryName
        if (Test-Path $candidate) {
            return $candidate
        }
    }

    return $null
}

function Remove-BackgroundService {
    param([string]$BinaryPath)

    if (-not $BinaryPath -or -not (Test-Path $BinaryPath)) {
        return
    }

    try {
        $env:MEMX_HOME = $MemxHome
        & $BinaryPath service remove | Out-Null
        Write-Host "Removed MemX background service"
    }
    catch {
    }
}

function Confirm-Uninstall {
    param(
        [string]$BinaryPath,
        [string]$BackupPath
    )

    Write-Warning "This will permanently delete your local MemX data."
    Write-Host "Paths to be removed:"
    Write-Host "  - $MemxHome"
    Write-Host "    This includes config.toml, memory.db, and any backups under ~/.memx."

    if ($BinaryPath) {
        Write-Host "  - $BinaryPath"
        if ($BackupPath -and (Test-Path $BackupPath)) {
            Write-Host "  - $BackupPath"
        }
    }
    else {
        Write-Host "  - memx.exe in common install paths (not found)"
    }

    Write-Host ""
    Write-Warning "Your local MemX data will be lost."

    if ($AssumeYes) {
        return $true
    }

    if (-not (Test-InteractiveConsole)) {
        throw "No interactive terminal detected. Re-run with -Yes or set MEMX_YES=1."
    }

    $answer = Read-Host "Continue uninstall? [y/N]"
    if ([string]::IsNullOrWhiteSpace($answer)) {
        return $false
    }

    switch ($answer.Trim().ToLowerInvariant()) {
        "y" { return $true }
        "yes" { return $true }
        default { return $false }
    }
}

function Maybe-InstallBackgroundService {
    param([string]$BinaryPath)

    if ($env:MEMX_INSTALL_SKIP_SERVICE -eq "1" -or $env:MEMX_INSTALL_SKIP_SETUP -eq "1") {
        return
    }

    $shouldInstall = $false
    if ($env:MEMX_INSTALL_START_SERVICE -eq "1") {
        $shouldInstall = $true
    }
    elseif (Test-InteractiveConsole) {
        $answer = Read-Host "Install and start MemX as a background service now? [Y/n]"
        if ([string]::IsNullOrWhiteSpace($answer)) {
            $shouldInstall = $true
        }
        else {
            switch ($answer.Trim().ToLowerInvariant()) {
                "y" { $shouldInstall = $true }
                "yes" { $shouldInstall = $true }
            }
        }
    }
    else {
        Write-Warning "No interactive terminal detected. Run this next to start MemX in the background:"
        Write-Host "  `$env:MEMX_HOME=""$MemxHome""; & ""$BinaryPath"" service install"
    }

    if (-not $shouldInstall) {
        return
    }

    Write-Host ""
    Write-Step "Installing and starting the MemX background service"
    $env:MEMX_HOME = $MemxHome
    & $BinaryPath service install
}

function Invoke-Uninstall {
    param([string]$PreferredDir)

    $targetPath = Find-InstalledBinary -PreferredDir $PreferredDir
    $backupPath = if ($targetPath) { "$targetPath.bak" } else { $null }

    Write-Host ""
    Write-Step "MemX Uninstall"
    Write-Host ""

    if (-not (Confirm-Uninstall -BinaryPath $targetPath -BackupPath $backupPath)) {
        Write-Host "Uninstall canceled."
        return
    }

    Remove-BackgroundService -BinaryPath $targetPath

    if ($targetPath -and (Test-Path $targetPath)) {
        Remove-Item -Path $targetPath -Force
        Write-Host "Removed $targetPath"
    }
    else {
        Write-Warning "Skipped binary removal (not found)."
    }

    if ($backupPath -and (Test-Path $backupPath)) {
        Remove-Item -Path $backupPath -Force
        Write-Host "Removed $backupPath"
    }

    if (Test-Path $MemxHome) {
        Remove-Item -Path $MemxHome -Recurse -Force
        Write-Host "Removed $MemxHome"
    }
    else {
        Write-Warning "Skipped $MemxHome (not found)."
    }

    if ($targetPath) {
        $installDir = Split-Path $targetPath -Parent
        $pathRemoved = Remove-UserPath -PathEntry $installDir
        if ($pathRemoved) {
            Write-Host "Removed $installDir from your user PATH."
        }
    }

    Write-Host ""
    Write-Host "MemX uninstall complete." -ForegroundColor Green
}

if ($ShouldUninstall) {
    Invoke-Uninstall -PreferredDir $InstallDir
    return
}

$assetName = Get-AssetName
$downloadUrl = Get-DownloadUrl -AssetName $assetName
$tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("memx-install-" + [System.Guid]::NewGuid().ToString("N"))
$archivePath = Join-Path $tempDir $assetName
$extractDir = Join-Path $tempDir "extract"
$targetPath = Join-Path $InstallDir $BinaryName

try {
    New-Item -ItemType Directory -Force -Path $tempDir | Out-Null
    New-Item -ItemType Directory -Force -Path $extractDir | Out-Null

    Write-Step "Downloading $downloadUrl"
    Invoke-WebRequest -Uri $downloadUrl -OutFile $archivePath

    Write-Step "Extracting archive"
    Expand-Archive -Path $archivePath -DestinationPath $extractDir -Force

    $sourceBinary = Get-ChildItem -Path $extractDir -Filter $BinaryName -Recurse | Select-Object -First 1
    if (-not $sourceBinary) {
        throw "Could not find $BinaryName inside archive."
    }

    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    if (Test-Path $targetPath) {
        Copy-Item $targetPath "$targetPath.bak" -Force
        Write-Host "Backed up existing binary to $targetPath.bak"
    }

    Copy-Item $sourceBinary.FullName $targetPath -Force

    Write-Step "Verifying binary"
    & $targetPath --help | Out-Null

    $pathUpdated = Add-UserPath -PathEntry $InstallDir
    $env:MEMX_HOME = $MemxHome

    Write-Host ""
    Write-Host "MemX installed successfully." -ForegroundColor Green
    Write-Host "Data directory: $MemxHome"
    Write-Host "Binary path: $targetPath"
    if ($pathUpdated) {
        Write-Host "Added $InstallDir to your user PATH. New terminals will pick it up."
    }

    $shouldSkipSetup = $SkipSetup -or ($env:MEMX_INSTALL_SKIP_SETUP -eq "1")
    if (-not $shouldSkipSetup) {
        if (Test-InteractiveConsole) {
            Write-Host ""
            Write-Step "Launching memx setup"
            & $targetPath setup
        }
        else {
            Write-Warning "No interactive terminal detected. Run this next:"
            Write-Host "  `$env:MEMX_HOME=""$MemxHome""; & ""$targetPath"" setup"
        }
    }
    else {
        Write-Warning "Skipping memx setup."
    }

    Maybe-InstallBackgroundService -BinaryPath $targetPath

    Write-Host ""
    Write-Host "Next steps:"
    Write-Host "  1. Validate config:"
    Write-Host "     `$env:MEMX_HOME=""$MemxHome""; & ""$targetPath"" doctor"
    Write-Host "  2. Check service status:"
    Write-Host "     `$env:MEMX_HOME=""$MemxHome""; & ""$targetPath"" service status"
    Write-Host "  3. Remove the background service:"
    Write-Host "     `$env:MEMX_HOME=""$MemxHome""; & ""$targetPath"" service remove"
    Write-Host "  4. Uninstall MemX and delete local data:"
    Write-Host "     `$env:MEMX_HOME=""$MemxHome""; & ""$targetPath"" uninstall"
}
finally {
    if (Test-Path $tempDir) {
        Remove-Item -Path $tempDir -Recurse -Force
    }
}

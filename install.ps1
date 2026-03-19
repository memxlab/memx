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
    [switch]$SkipSetup
)

$ErrorActionPreference = "Stop"

$Repo = if ($env:MEMX_REPO) { $env:MEMX_REPO } else { "memxlab/memx" }
$Version = if ($env:MEMX_VERSION) { $env:MEMX_VERSION } else { "latest" }
$DownloadBaseUrl = $env:MEMX_DOWNLOAD_BASE_URL
$BinaryName = "memx.exe"

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

function Test-InteractiveConsole {
    try {
        return (-not [Console]::IsInputRedirected) -and (-not [Console]::IsOutputRedirected)
    }
    catch {
        return $true
    }
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

    Write-Host ""
    Write-Host "MemX installed successfully." -ForegroundColor Green
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
            Write-Host "  $targetPath setup"
        }
    }
    else {
        Write-Warning "Skipping memx setup."
    }

    Write-Host ""
    Write-Host "Next steps:"
    Write-Host "  1. Run setup if you skipped it:"
    Write-Host "     $targetPath setup"
    Write-Host "  2. Validate config:"
    Write-Host "     $targetPath doctor"
    Write-Host "  3. Start the server:"
    Write-Host "     $targetPath serve"
}
finally {
    if (Test-Path $tempDir) {
        Remove-Item -Path $tempDir -Recurse -Force
    }
}

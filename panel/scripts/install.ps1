$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw "Run this installer from an elevated PowerShell session."
}

if ([string]::IsNullOrWhiteSpace($env:HYDRA_INSTALLER_MODE)) {
    $selectedMode = Read-Host "Installation mode (first_host or managed) [first_host]"
    $env:HYDRA_INSTALLER_MODE = if ([string]::IsNullOrWhiteSpace($selectedMode)) {
        "first_host"
    } else {
        $selectedMode.Trim().ToLowerInvariant()
    }
}
if ($env:HYDRA_INSTALLER_MODE -eq "first_host") {
    throw "Windows first-host installation is staged but fail-closed until service environment, ACL, and certificate recipes are production-ready."
}
if ($env:HYDRA_INSTALLER_MODE -ne "managed") {
    throw "HYDRA_INSTALLER_MODE must be first_host or managed."
}

if ([string]::IsNullOrWhiteSpace($env:HYDRA_INSTALLER_PANEL_URL)) {
    $env:HYDRA_INSTALLER_PANEL_URL = Read-Host "Existing Hydra Panel URL (HTTPS)"
}
if ([string]::IsNullOrWhiteSpace($env:HYDRA_INSTALLER_JOB_ID)) {
    $env:HYDRA_INSTALLER_JOB_ID = Read-Host "Installer job ID"
}
if ([string]::IsNullOrWhiteSpace($env:HYDRA_INSTALLER_EXECUTOR_TOKEN)) {
    $secureToken = Read-Host "One-time installer token" -AsSecureString
    $tokenPointer = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secureToken)
    try {
        $env:HYDRA_INSTALLER_EXECUTOR_TOKEN = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($tokenPointer)
    }
    finally {
        [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($tokenPointer)
    }
}

$panelUri = [Uri]$env:HYDRA_INSTALLER_PANEL_URL
$isLocalDevelopment = $panelUri.Host -in @("127.0.0.1", "localhost", "::1")
if ($panelUri.Scheme -ne "https" -and -not ($isLocalDevelopment -and $panelUri.Scheme -eq "http")) {
    throw "Panel URL must use HTTPS except localhost development."
}

$releaseBase = if ($env:HYDRA_INSTALLER_RELEASE_BASE_URL) {
    $env:HYDRA_INSTALLER_RELEASE_BASE_URL.TrimEnd("/")
} else {
    "https://github.com/Zolotushka1/Hydra-Panel/releases/latest/download"
}
$executorUrl = if ($env:HYDRA_INSTALLER_EXECUTOR_URL) {
    $env:HYDRA_INSTALLER_EXECUTOR_URL
} else {
    "$releaseBase/panel-installer-executor-windows-x86_64.exe"
}
$checksumUrl = if ($env:HYDRA_INSTALLER_EXECUTOR_SHA256_URL) {
    $env:HYDRA_INSTALLER_EXECUTOR_SHA256_URL
} else {
    "$executorUrl.sha256"
}
if (([Uri]$executorUrl).Scheme -ne "https" -or ([Uri]$checksumUrl).Scheme -ne "https") {
    throw "Executor and checksum URLs must use HTTPS."
}

$workDir = Join-Path $env:TEMP ("hydra-installer-" + [Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $workDir | Out-Null
try {
    $executorPath = Join-Path $workDir "panel-installer-executor.exe"
    $checksumPath = Join-Path $workDir "panel-installer-executor.sha256"
    Invoke-WebRequest -UseBasicParsing -Uri $executorUrl -OutFile $executorPath
    Invoke-WebRequest -UseBasicParsing -Uri $checksumUrl -OutFile $checksumPath

    $expectedSha256 = ((Get-Content -LiteralPath $checksumPath -TotalCount 1) -split "\s+")[0].ToLowerInvariant()
    if ($expectedSha256 -notmatch "^[0-9a-f]{64}$") {
        throw "Executor checksum file is invalid."
    }
    $actualSha256 = (Get-FileHash -LiteralPath $executorPath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualSha256 -ne $expectedSha256) {
        throw "Executor SHA-256 mismatch."
    }

    if ([string]::IsNullOrWhiteSpace($env:HYDRA_INSTALLER_CONFIRM_DESTRUCTIVE)) {
        $env:HYDRA_INSTALLER_CONFIRM_DESTRUCTIVE = "1"
    }
    & $executorPath
    if ($LASTEXITCODE -ne 0) {
        throw "Hydra installer executor failed with exit code $LASTEXITCODE."
    }
}
finally {
    Remove-Item -LiteralPath $workDir -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item Env:HYDRA_INSTALLER_EXECUTOR_TOKEN -ErrorAction SilentlyContinue
}

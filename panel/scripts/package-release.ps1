param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[0-9A-Za-z._+-]+$')]
    [string]$Version,

    [Parameter(Mandatory = $true)]
    [ValidatePattern('^https://')]
    [string]$ReleaseBaseUrl,

    [string]$DistDir = "dist/windows-x86_64"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$ReleaseBaseUrl = $ReleaseBaseUrl.TrimEnd("/")
$repositoryRoot = [IO.Path]::GetFullPath((Get-Location).Path)
$allowedDistRoot = [IO.Path]::GetFullPath((Join-Path $repositoryRoot "dist"))
$resolvedDistDir = [IO.Path]::GetFullPath((Join-Path $repositoryRoot $DistDir))
if (-not $resolvedDistDir.StartsWith($allowedDistRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
    throw "Release output must stay under the repository dist directory."
}
$DistDir = $resolvedDistDir
if (Test-Path -LiteralPath $DistDir) {
    Remove-Item -LiteralPath $DistDir -Recurse -Force
}
New-Item -ItemType Directory -Path $DistDir | Out-Null

& cargo build --locked --release -p panel-app -p panel-installer-executor
if ($LASTEXITCODE -ne 0) {
    throw "Cargo release build failed with exit code $LASTEXITCODE."
}

$panelName = "hydra-panel-windows-x86_64.exe"
$executorName = "panel-installer-executor-windows-x86_64.exe"
$installerName = "install-windows-x86_64.ps1"

Copy-Item -LiteralPath "target/release/panel-app.exe" -Destination (Join-Path $DistDir $panelName)
Copy-Item -LiteralPath "target/release/panel-installer-executor.exe" -Destination (Join-Path $DistDir $executorName)
Copy-Item -LiteralPath "scripts/install.ps1" -Destination (Join-Path $DistDir $installerName)

# The frontend is read from disk at run time, not embedded, so the bundle has to
# travel with the binary. "web" beside the executable is exactly where the panel
# looks by default. Without this the operator gets a panel that serves the API
# and an empty dashboard.
if (-not (Test-Path -LiteralPath "web/dist/index.html")) {
    throw "web/dist is not built; run 'npm ci && npm run build' in web/."
}
$webDir = Join-Path $DistDir "web"
if (Test-Path -LiteralPath $webDir) {
    Remove-Item -LiteralPath $webDir -Recurse -Force
}
Copy-Item -LiteralPath "web/dist" -Destination $webDir -Recurse

foreach ($artifact in @($panelName, $executorName, $installerName)) {
    $artifactPath = Join-Path $DistDir $artifact
    $sha256 = (Get-FileHash -LiteralPath $artifactPath -Algorithm SHA256).Hash.ToLowerInvariant()
    Set-Content -LiteralPath "$artifactPath.sha256" -Value "$sha256  $artifact" -Encoding ascii
}

$panelSha = (Get-FileHash -LiteralPath (Join-Path $DistDir $panelName) -Algorithm SHA256).Hash.ToLowerInvariant()
$installerSha = (Get-FileHash -LiteralPath (Join-Path $DistDir $installerName) -Algorithm SHA256).Hash.ToLowerInvariant()
$manifest = [ordered]@{
    manifest_version = 1
    artifacts = @(
        [ordered]@{
            name = $installerName
            artifact_kind = "installer_script"
            target_os = "windows"
            target_arch = "x86_64"
            package_channel = "stable"
            version = $Version
            url = "$ReleaseBaseUrl/$installerName"
            sha256 = $installerSha
        },
        [ordered]@{
            name = $panelName
            artifact_kind = "panel_binary"
            target_os = "windows"
            target_arch = "x86_64"
            package_channel = "stable"
            version = $Version
            url = "$ReleaseBaseUrl/$panelName"
            sha256 = $panelSha
        }
    )
}
$manifestJson = $manifest | ConvertTo-Json -Depth 5
$manifestPath = Join-Path $DistDir "release-manifest-windows-x86_64.json"
[IO.File]::WriteAllText($manifestPath, $manifestJson, [Text.UTF8Encoding]::new($false))

Write-Host "Packaged Windows x86_64 release artifacts in $DistDir"

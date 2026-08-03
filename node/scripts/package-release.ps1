param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[0-9A-Za-z._+-]+$')]
    [string]$Version,

    [Parameter(Mandatory = $true)]
    [ValidatePattern('^https://github\.com/Zolotushka1/Hydra/releases/download/node-v')]
    [string]$ReleaseBaseUrl,

    [string]$DistDir = "dist/windows-x86_64"
)

# Artifact names are part of the contract: the panel install step downloads
# whatever HYDRA_NODE_ARTIFACT_URL points at, and that URL is pinned to the
# project's own node-v* release path. The ReleaseBaseUrl pattern above enforces
# the same rule, so a hand-run packaging cannot quietly produce an artifact the
# panel will refuse to install.

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

& cargo build --locked --release -p node-app -p node-session-adapter -p node-session-driver-wireguard
if ($LASTEXITCODE -ne 0) {
    throw "Cargo release build failed with exit code $LASTEXITCODE."
}

$nodeName = "hydra-node-windows-x86_64.exe"
$adapterName = "node-session-adapter-windows-x86_64.exe"
$driverName = "node-session-driver-wireguard-windows-x86_64.exe"

Copy-Item -LiteralPath "target/release/node-app.exe" -Destination (Join-Path $DistDir $nodeName)
Copy-Item -LiteralPath "target/release/node-session-adapter.exe" -Destination (Join-Path $DistDir $adapterName)
Copy-Item -LiteralPath "target/release/node-session-driver-wireguard.exe" -Destination (Join-Path $DistDir $driverName)

foreach ($artifact in @($nodeName, $adapterName, $driverName)) {
    $artifactPath = Join-Path $DistDir $artifact
    $sha256 = (Get-FileHash -LiteralPath $artifactPath -Algorithm SHA256).Hash.ToLowerInvariant()
    Set-Content -LiteralPath "$artifactPath.sha256" -Value "$sha256  $artifact" -Encoding ascii
}

$nodeSha = (Get-FileHash -LiteralPath (Join-Path $DistDir $nodeName) -Algorithm SHA256).Hash.ToLowerInvariant()
$adapterSha = (Get-FileHash -LiteralPath (Join-Path $DistDir $adapterName) -Algorithm SHA256).Hash.ToLowerInvariant()
$driverSha = (Get-FileHash -LiteralPath (Join-Path $DistDir $driverName) -Algorithm SHA256).Hash.ToLowerInvariant()

$manifest = [ordered]@{
    manifest_version = 1
    artifacts = @(
        [ordered]@{
            name = $nodeName
            artifact_kind = "node_binary"
            target_os = "windows"
            target_arch = "x86_64"
            package_channel = "stable"
            version = $Version
            url = "$ReleaseBaseUrl/$nodeName"
            sha256 = $nodeSha
        },
        [ordered]@{
            name = $adapterName
            artifact_kind = "node_session_adapter_binary"
            target_os = "windows"
            target_arch = "x86_64"
            package_channel = "stable"
            version = $Version
            url = "$ReleaseBaseUrl/$adapterName"
            sha256 = $adapterSha
        },
        [ordered]@{
            name = $driverName
            artifact_kind = "node_session_driver_wireguard_binary"
            target_os = "windows"
            target_arch = "x86_64"
            package_channel = "stable"
            version = $Version
            url = "$ReleaseBaseUrl/$driverName"
            sha256 = $driverSha
        }
    )
}

$manifestPath = Join-Path $DistDir "release-manifest-windows-x86_64.json"
$manifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $manifestPath -Encoding utf8

Write-Output "Packaged Windows x86_64 node release artifacts in $DistDir"

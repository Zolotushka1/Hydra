param(
    [string]$PanelBindAddr = "127.0.0.1:18080",
    [string]$NodeBindAddr = "127.0.0.1:18081",
    [string]$NodeRepo = "../Hydra-node",
    [string]$DataDir = ".smoke/panel-node",
    [string]$PanelTargetDir = ".target/smoke-panel-node-panel",
    [string]$NodeTargetDir = ".target/smoke-panel-node-node",
    [string]$XrayBinaryPath = "",
    [switch]$KeepData
)

$ErrorActionPreference = "Stop"

function Resolve-FsPath($path) {
    $resolved = Resolve-Path -LiteralPath $path
    if ($resolved.ProviderPath) {
        return $resolved.ProviderPath
    }
    return $resolved.Path
}

function Set-SmokePathEnv($name, $root, $fileName) {
    Set-Item -Path "Env:$name" -Value (Join-Path $root $fileName)
}

function Wait-HttpJson($uri, $description, $headers = $null, $timeoutSeconds = 45) {
    $deadline = (Get-Date).AddSeconds($timeoutSeconds)
    do {
        try {
            if ($headers) {
                return Invoke-RestMethod -Method Get -Uri $uri -Headers $headers -TimeoutSec 2
            }
            return Invoke-RestMethod -Method Get -Uri $uri -TimeoutSec 2
        } catch {
            Start-Sleep -Milliseconds 500
        }
    } while ((Get-Date) -lt $deadline)

    throw "$description did not become available at $uri within $timeoutSeconds seconds"
}

function Wait-PanelNodeSync($baseUrl, $nodeBaseUrl, $headers, $nodeId, $requireRealXray, $timeoutSeconds = 60) {
    $deadline = (Get-Date).AddSeconds($timeoutSeconds)
    $lastDetail = ""
    do {
        try {
            $nodeHealth = Invoke-RestMethod -Method Get -Uri "$nodeBaseUrl/health" -TimeoutSec 2
            $status = Invoke-RestMethod -Method Get -Uri "$baseUrl/api/nodes/$nodeId/apply-status" -Headers $headers -TimeoutSec 2
            $externalXrayStage = $status.stages | Where-Object { $_.stage -eq "xray_external_validation" } | Select-Object -First 1
            $externalXrayStatus = if ($externalXrayStage) { $externalXrayStage.status } else { "missing" }
            $safeToRestart = if ($status.lifecycle) { $status.lifecycle.safe_to_restart } else { $false }
            $lastDetail = "node_health=$($nodeHealth.status), node_revision=$($nodeHealth.applied_revision), panel_synced=$($status.synced), panel_local_state=$($status.local_state_available), panel_sync=$($status.node.sync_status), safe_to_restart=$safeToRestart, external_xray=$externalXrayStatus"

            $contractSynced = $nodeHealth.applied_revision -and $status.local_state_available -and $status.node.sync_status -eq "synced"
            $realXraySynced = $contractSynced -and $status.synced -and $safeToRestart -and $externalXrayStatus -eq "ok"
            if ((!$requireRealXray -and $contractSynced) -or ($requireRealXray -and $realXraySynced)) {
                return @{
                    node_health = $nodeHealth
                    apply_status = $status
                }
            }
        } catch {
            $lastDetail = $_.Exception.Message
        }
        Start-Sleep -Milliseconds 750
    } while ((Get-Date) -lt $deadline)

    throw "Panel-node sync did not complete within $timeoutSeconds seconds. Last state: $lastDetail"
}

$panelRepoRoot = Resolve-FsPath (Join-Path $PSScriptRoot "..")
$nodeRepoRoot = Resolve-FsPath (Join-Path $panelRepoRoot $NodeRepo)
$realXrayMode = -not [string]::IsNullOrWhiteSpace($XrayBinaryPath)
$resolvedXrayBinaryPath = $null
if ($realXrayMode) {
    $resolvedXrayBinaryPath = Resolve-FsPath $XrayBinaryPath
    if (!(Test-Path -LiteralPath $resolvedXrayBinaryPath -PathType Leaf)) {
        throw "Xray binary not found at $resolvedXrayBinaryPath"
    }
}
$smokeRoot = Join-Path $panelRepoRoot $DataDir
$panelData = Join-Path $smokeRoot "panel-data"
$nodeData = Join-Path $smokeRoot "node-data"
$panelTarget = Join-Path $panelRepoRoot $PanelTargetDir
$nodeTarget = Join-Path $nodeRepoRoot $NodeTargetDir
$panelStdout = Join-Path $smokeRoot "panel.out.log"
$panelStderr = Join-Path $smokeRoot "panel.err.log"
$nodeStdout = Join-Path $smokeRoot "node.out.log"
$nodeStderr = Join-Path $smokeRoot "node.err.log"

if (Test-Path -LiteralPath $smokeRoot) {
    Remove-Item -LiteralPath $smokeRoot -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $panelData | Out-Null
New-Item -ItemType Directory -Force -Path $nodeData | Out-Null

$panelProcess = $null
$nodeProcess = $null

try {
    $env:CARGO_TARGET_DIR = $panelTarget
    $env:HYDRA_BIND_ADDR = $PanelBindAddr
    $env:HYDRA_BOOTSTRAP_ADMIN_USERNAME = "admin"
    $env:HYDRA_BOOTSTRAP_ADMIN_PASSWORD = "admin12345"
    $env:HYDRA_XRAY_VALIDATION_TEMP_DIR = (Join-Path $panelData "xray-validation")
    $env:HYDRA_XRAY_UPDATE_WORK_DIR = (Join-Path $panelData "xray-updates")

    Set-SmokePathEnv "HYDRA_SECURITY_SETTINGS_PATH" $panelData "security-settings.json"
    Set-SmokePathEnv "HYDRA_ADMIN_PATH" $panelData "admin.json"
    Set-SmokePathEnv "HYDRA_ADMIN_SECRETS_KEY_PATH" $panelData "admin-secrets.key"
    Set-SmokePathEnv "HYDRA_AUDIT_LOG_PATH" $panelData "security-audit.ndjson"
    Set-SmokePathEnv "HYDRA_MONITORING_THRESHOLDS_PATH" $panelData "monitoring-thresholds.json"
    Set-SmokePathEnv "HYDRA_ALERT_HISTORY_PATH" $panelData "system-alerts.ndjson"
    Set-SmokePathEnv "HYDRA_CORE_CONFIG_PATH" $panelData "core-config.json"
    Set-SmokePathEnv "HYDRA_OPERATIONAL_LOG_PATH" $panelData "operational.ndjson"
    Set-SmokePathEnv "HYDRA_CORE_APPLY_HISTORY_PATH" $panelData "core-apply-history.ndjson"
    Set-SmokePathEnv "HYDRA_NODE_SYNC_HISTORY_PATH" $panelData "node-sync-history.ndjson"
    Set-SmokePathEnv "HYDRA_NODE_APPLY_RESULTS_PATH" $panelData "node-apply-results.ndjson"
    Set-SmokePathEnv "HYDRA_NODE_BOOTSTRAP_HISTORY_PATH" $panelData "node-bootstrap-history.ndjson"
    Set-SmokePathEnv "HYDRA_NODE_PROVISIONING_TASKS_PATH" $panelData "node-provisioning-tasks.json"
    Set-SmokePathEnv "HYDRA_NODE_PROVISIONING_EVENTS_PATH" $panelData "node-provisioning-events.ndjson"
    Set-SmokePathEnv "HYDRA_PANEL_INSTALLER_JOBS_PATH" $panelData "panel-installer-jobs.json"
    Set-SmokePathEnv "HYDRA_TELEGRAM_SETTINGS_PATH" $panelData "telegram-settings.json"
    Set-SmokePathEnv "HYDRA_TELEGRAM_SECRETS_KEY_PATH" $panelData "telegram-secrets.key"
    Set-SmokePathEnv "HYDRA_TELEGRAM_EVENTS_PATH" $panelData "telegram-events.ndjson"
    Set-SmokePathEnv "HYDRA_USER_ACTIVITY_LOG_PATH" $panelData "user-activity.ndjson"
    Set-SmokePathEnv "HYDRA_USERS_PATH" $panelData "users.json"
    Set-SmokePathEnv "HYDRA_USER_TEMPLATES_PATH" $panelData "user-templates.json"
    Set-SmokePathEnv "HYDRA_SUBSCRIPTION_CATALOG_PATH" $panelData "subscription-catalog.json"
    Set-SmokePathEnv "HYDRA_SUBSCRIPTION_DEVICES_KEY_PATH" $panelData "subscription-devices.key"
    Set-SmokePathEnv "HYDRA_NETWORK_RESOURCES_PATH" $panelData "network-resources.json"
    Set-SmokePathEnv "HYDRA_CLUSTERS_PATH" $panelData "clusters.json"
    Set-SmokePathEnv "HYDRA_ROUTE_MATERIALS_PATH" $panelData "route-materials.json"
    Set-SmokePathEnv "HYDRA_ROUTE_MATERIALS_KEY_PATH" $panelData "route-materials.key"
    Set-SmokePathEnv "HYDRA_NODE_SECRETS_KEY_PATH" $panelData "node-secrets.key"
    Set-SmokePathEnv "HYDRA_NODES_PATH" $panelData "nodes.json"

    Push-Location $panelRepoRoot
    try {
        cargo build -p panel-app
    } finally {
        Pop-Location
    }

    $isWindowsRuntime = [System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform(
        [System.Runtime.InteropServices.OSPlatform]::Windows
    )
    $panelExeName = if ($isWindowsRuntime) { "panel-app.exe" } else { "panel-app" }
    $panelExePath = Join-Path (Join-Path $panelTarget "debug") $panelExeName
    if (!(Test-Path -LiteralPath $panelExePath)) {
        throw "Built panel binary not found at $panelExePath"
    }

    $panelProcess = Start-Process -FilePath $panelExePath -WorkingDirectory $panelRepoRoot -PassThru -RedirectStandardOutput $panelStdout -RedirectStandardError $panelStderr
    $panelBaseUrl = "http://$PanelBindAddr"
    Wait-HttpJson "$panelBaseUrl/health" "Panel health" | Out-Null

    $loginBody = @{
        username = "admin"
        password = "admin12345"
    } | ConvertTo-Json
    $login = Invoke-RestMethod -Method Post -Uri "$panelBaseUrl/api/admin/login" -ContentType "application/json" -Body $loginBody
    $adminHeaders = @{ Authorization = "Bearer $($login.token)" }

    $createNodeBody = @{
        name = "smoke-node"
        address = "127.0.0.1"
        port = 62050
        api_port = [int]($NodeBindAddr.Split(":")[-1])
        usage_coefficient = 1.0
        enabled = $true
        local_api_token = "smoke-local-node-token"
    } | ConvertTo-Json
    $node = Invoke-RestMethod -Method Post -Uri "$panelBaseUrl/api/nodes" -Headers $adminHeaders -ContentType "application/json" -Body $createNodeBody
    $rotated = Invoke-RestMethod -Method Post -Uri "$panelBaseUrl/api/nodes/$($node.id)/auth/rotate" -Headers $adminHeaders

    if ($realXrayMode) {
        $createInboundBody = @{
            tag = "smoke-vless-tcp"
            port = 62052
            protocol = "vless"
            network = "tcp"
            tls_enabled = $false
            node_id = $node.id
            cluster_id = $null
        } | ConvertTo-Json
        Invoke-RestMethod -Method Post -Uri "$panelBaseUrl/api/inbounds" -Headers $adminHeaders -ContentType "application/json" -Body $createInboundBody | Out-Null

        $createProfileBody = @{
            name = "smoke-vless-profile"
            proxy_type = "vless"
            settings_json = '{"uuid":"11111111-1111-5111-8111-111111111111"}'
            excluded_inbound_tags = @()
        } | ConvertTo-Json
        $profile = Invoke-RestMethod -Method Post -Uri "$panelBaseUrl/api/proxy-profiles" -Headers $adminHeaders -ContentType "application/json" -Body $createProfileBody

        $createUserBody = @{
            username = "smoke-client"
            template_id = $null
            next_template_id = $null
            status = "active"
            data_limit_bytes = $null
            expire_at_unix = $null
            note = "strict smoke client"
            proxy_profile_ids = @($profile.id)
            excluded_inbound_tags = @()
        } | ConvertTo-Json
        Invoke-RestMethod -Method Post -Uri "$panelBaseUrl/api/users" -Headers $adminHeaders -ContentType "application/json" -Body $createUserBody | Out-Null
    }

    $env:CARGO_TARGET_DIR = $nodeTarget
    $env:HYDRA_PANEL_URL = $panelBaseUrl
    $env:HYDRA_NODE_TOKEN = $rotated.auth_token
    $env:HYDRA_NODE_POLL_INTERVAL_SECONDS = "1"
    $env:HYDRA_NODE_LOCAL_API_BIND = $NodeBindAddr
    $env:HYDRA_NODE_LOCAL_API_TOKEN = "smoke-local-node-token"
    if ($realXrayMode) {
        $env:HYDRA_NODE_XRAY_APPLY_MODE = "external_validate_only"
        $env:HYDRA_NODE_XRAY_BINARY_PATH = $resolvedXrayBinaryPath
    } else {
        $env:HYDRA_NODE_XRAY_APPLY_MODE = "noop"
        Remove-Item -Path "Env:HYDRA_NODE_XRAY_BINARY_PATH" -ErrorAction SilentlyContinue
    }
    $env:HYDRA_NODE_STATE_PATH = Join-Path $nodeData "node-state.json"
    $env:HYDRA_NODE_CONFIG_PATH = Join-Path $nodeData "generated-config.json"
    $env:HYDRA_NODE_RUNTIME_CONFIG_PATH = Join-Path $nodeData "node-runtime-config.json"
    $env:HYDRA_NODE_SIDECAR_RUNTIME_CONFIG_PATH = Join-Path $nodeData "sidecar-runtime-config.json"
    $env:HYDRA_NODE_XRAY_CONFIG_PATH = Join-Path $nodeData "xray.json"
    $env:HYDRA_NODE_ROUTE_CREDENTIALS_PATH = Join-Path $nodeData "route-credentials.json"
    $env:HYDRA_NODE_ROUTE_CREDENTIALS_DIR = Join-Path $nodeData "route-credentials"
    $env:HYDRA_NODE_APPLY_HISTORY_PATH = Join-Path $nodeData "apply-history.json"
    $env:HYDRA_NODE_RUNTIME_EVENTS_PATH = Join-Path $nodeData "runtime-events.json"

    Push-Location $nodeRepoRoot
    try {
        cargo build -p node-app
    } finally {
        Pop-Location
    }

    $nodeExeName = if ($isWindowsRuntime) { "node-app.exe" } else { "node-app" }
    $nodeExePath = Join-Path (Join-Path $nodeTarget "debug") $nodeExeName
    if (!(Test-Path -LiteralPath $nodeExePath)) {
        throw "Built node binary not found at $nodeExePath"
    }

    $nodeProcess = Start-Process -FilePath $nodeExePath -WorkingDirectory $nodeRepoRoot -PassThru -RedirectStandardOutput $nodeStdout -RedirectStandardError $nodeStderr
    $nodeBaseUrl = "http://$NodeBindAddr"
    Wait-HttpJson "$nodeBaseUrl/health" "Node health" | Out-Null

    $result = Wait-PanelNodeSync $panelBaseUrl $nodeBaseUrl $adminHeaders $node.id $realXrayMode
    $syncHistory = @(Invoke-RestMethod -Method Get -Uri "$panelBaseUrl/api/nodes/$($node.id)/sync-history?limit=5" -Headers $adminHeaders)
    $syncedRecord = $syncHistory | Where-Object { $_.sync_status -eq "synced" -and $_.applied_revision -eq $result.node_health.applied_revision } | Select-Object -First 1
    if (-not $syncedRecord) {
        throw "Panel sync history does not contain synced node record for revision $($result.node_health.applied_revision)"
    }

    $nodeStateHeaders = @{ "X-Hydra-Local-Token" = "smoke-local-node-token" }
    $nodeState = Invoke-RestMethod -Method Get -Uri "$nodeBaseUrl/state" -Headers $nodeStateHeaders
    if ($nodeState.node_id -ne $node.id) {
        throw "Node local state reports unexpected node id: $($nodeState.node_id)"
    }

    Write-Host "Panel + remote node smoke passed"
    Write-Host "Panel: $panelBaseUrl"
    Write-Host "Node:  $nodeBaseUrl"
    Write-Host "Node id: $($node.id)"
    Write-Host "Revision: $($result.node_health.applied_revision)"
    if ($realXrayMode) {
        Write-Host "Real Xray validation: passed"
        Write-Host "Xray binary: $resolvedXrayBinaryPath"
    } else {
        Write-Host "Real Xray validation: skipped (noop mode)"
    }
} finally {
    if ($nodeProcess -and -not $nodeProcess.HasExited) {
        Stop-Process -Id $nodeProcess.Id -Force
        $nodeProcess.WaitForExit()
    }
    if ($panelProcess -and -not $panelProcess.HasExited) {
        Stop-Process -Id $panelProcess.Id -Force
        $panelProcess.WaitForExit()
    }
    if (-not $KeepData) {
        Remove-Item -LiteralPath $smokeRoot -Recurse -Force -ErrorAction SilentlyContinue
    } else {
        Write-Host "Smoke data kept at $smokeRoot"
    }
}

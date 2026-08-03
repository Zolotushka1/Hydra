param(
    [string]$BindAddr = "127.0.0.1:18080",
    [string]$DataDir = ".smoke/panel-standalone",
    [string]$TargetDir = ".target/smoke-standalone",
    [switch]$KeepData
)

$ErrorActionPreference = "Stop"

$resolvedRepoRoot = Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "..")
$repoRoot = if ($resolvedRepoRoot.ProviderPath) {
    $resolvedRepoRoot.ProviderPath
} else {
    $resolvedRepoRoot.Path
}
$dataPath = Join-Path $repoRoot $DataDir
$targetDirPath = Join-Path $repoRoot $TargetDir
$stdoutLogPath = Join-Path $dataPath "panel.out.log"
$stderrLogPath = Join-Path $dataPath "panel.err.log"

function Set-SmokePathEnv($name, $fileName) {
    Set-Item -Path "Env:$name" -Value (Join-Path $dataPath $fileName)
}

function Wait-PanelHealth($baseUrl) {
    $deadline = (Get-Date).AddSeconds(30)
    do {
        try {
            return Invoke-RestMethod -Method Get -Uri "$baseUrl/health" -TimeoutSec 2
        } catch {
            Start-Sleep -Milliseconds 500
        }
    } while ((Get-Date) -lt $deadline)

    throw "Panel did not become healthy at $baseUrl within 30 seconds"
}

if (Test-Path $dataPath) {
    Remove-Item -LiteralPath $dataPath -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $dataPath | Out-Null

$env:CARGO_TARGET_DIR = $targetDirPath
$env:HYDRA_BIND_ADDR = $BindAddr
$env:HYDRA_BOOTSTRAP_ADMIN_USERNAME = "admin"
$env:HYDRA_BOOTSTRAP_ADMIN_PASSWORD = "admin12345"
$env:HYDRA_XRAY_VALIDATION_TEMP_DIR = (Join-Path $dataPath "xray-validation")
$env:HYDRA_XRAY_UPDATE_WORK_DIR = (Join-Path $dataPath "xray-updates")

Set-SmokePathEnv "HYDRA_SECURITY_SETTINGS_PATH" "security-settings.json"
Set-SmokePathEnv "HYDRA_ADMIN_PATH" "admin.json"
Set-SmokePathEnv "HYDRA_ADMIN_SECRETS_KEY_PATH" "admin-secrets.key"
Set-SmokePathEnv "HYDRA_AUDIT_LOG_PATH" "security-audit.ndjson"
Set-SmokePathEnv "HYDRA_MONITORING_THRESHOLDS_PATH" "monitoring-thresholds.json"
Set-SmokePathEnv "HYDRA_ALERT_HISTORY_PATH" "system-alerts.ndjson"
Set-SmokePathEnv "HYDRA_CORE_CONFIG_PATH" "core-config.json"
Set-SmokePathEnv "HYDRA_OPERATIONAL_LOG_PATH" "operational.ndjson"
Set-SmokePathEnv "HYDRA_CORE_APPLY_HISTORY_PATH" "core-apply-history.ndjson"
Set-SmokePathEnv "HYDRA_NODE_SYNC_HISTORY_PATH" "node-sync-history.ndjson"
Set-SmokePathEnv "HYDRA_NODE_APPLY_RESULTS_PATH" "node-apply-results.ndjson"
Set-SmokePathEnv "HYDRA_NODE_BOOTSTRAP_HISTORY_PATH" "node-bootstrap-history.ndjson"
Set-SmokePathEnv "HYDRA_NODE_PROVISIONING_TASKS_PATH" "node-provisioning-tasks.json"
Set-SmokePathEnv "HYDRA_NODE_PROVISIONING_EVENTS_PATH" "node-provisioning-events.ndjson"
Set-SmokePathEnv "HYDRA_PANEL_INSTALLER_JOBS_PATH" "panel-installer-jobs.json"
Set-SmokePathEnv "HYDRA_TELEGRAM_SETTINGS_PATH" "telegram-settings.json"
Set-SmokePathEnv "HYDRA_TELEGRAM_SECRETS_KEY_PATH" "telegram-secrets.key"
Set-SmokePathEnv "HYDRA_TELEGRAM_EVENTS_PATH" "telegram-events.ndjson"
Set-SmokePathEnv "HYDRA_USER_ACTIVITY_LOG_PATH" "user-activity.ndjson"
Set-SmokePathEnv "HYDRA_USERS_PATH" "users.json"
Set-SmokePathEnv "HYDRA_USER_TEMPLATES_PATH" "user-templates.json"
Set-SmokePathEnv "HYDRA_SUBSCRIPTION_CATALOG_PATH" "subscription-catalog.json"
Set-SmokePathEnv "HYDRA_SUBSCRIPTION_DEVICES_KEY_PATH" "subscription-devices.key"
Set-SmokePathEnv "HYDRA_NETWORK_RESOURCES_PATH" "network-resources.json"
Set-SmokePathEnv "HYDRA_CLUSTERS_PATH" "clusters.json"
Set-SmokePathEnv "HYDRA_ROUTE_MATERIALS_PATH" "route-materials.json"
Set-SmokePathEnv "HYDRA_ROUTE_MATERIALS_KEY_PATH" "route-materials.key"
Set-SmokePathEnv "HYDRA_NODE_SECRETS_KEY_PATH" "node-secrets.key"
Set-SmokePathEnv "HYDRA_NODES_PATH" "nodes.json"

Push-Location $repoRoot
try {
    cargo build -p panel-app

    $isWindowsRuntime = [System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform(
        [System.Runtime.InteropServices.OSPlatform]::Windows
    )
    $exeName = if ($isWindowsRuntime) { "panel-app.exe" } else { "panel-app" }
    $exePath = Join-Path (Join-Path $targetDirPath "debug") $exeName
    if (!(Test-Path $exePath)) {
        throw "Built panel binary not found at $exePath"
    }

    $process = Start-Process -FilePath $exePath -WorkingDirectory $repoRoot -PassThru -RedirectStandardOutput $stdoutLogPath -RedirectStandardError $stderrLogPath
    try {
        $baseUrl = "http://$BindAddr"
        Wait-PanelHealth $baseUrl | Out-Null

        $loginBody = @{
            username = "admin"
            password = "admin12345"
        } | ConvertTo-Json
        $login = Invoke-RestMethod -Method Post -Uri "$baseUrl/api/admin/login" -ContentType "application/json" -Body $loginBody
        $headers = @{ Authorization = "Bearer $($login.token)" }

        $xrayConfig = Invoke-RestMethod -Method Get -Uri "$baseUrl/api/core/xray-config" -Headers $headers
        if (-not $xrayConfig.raw_config_validation.valid) {
            throw "Standalone generated xray config failed internal validation"
        }

        $apply = Invoke-RestMethod -Method Post -Uri "$baseUrl/api/core/apply-generated" -Headers $headers -ContentType "application/json" -Body (@{ revision = $null } | ConvertTo-Json)
        if ($apply.result -ne "applied") {
            throw "Standalone core apply did not return applied: $($apply.result)"
        }

        $state = Invoke-RestMethod -Method Get -Uri "$baseUrl/api/core/state" -Headers $headers
        if ($state.status -ne "running") {
            throw "Standalone core state is not running: $($state.status)"
        }

        $history = @(Invoke-RestMethod -Method Get -Uri "$baseUrl/api/core/apply-history" -Headers $headers)
        $matchingAppliedRecord = $history | Where-Object { $_.revision -eq $apply.revision -and $_.result -eq "applied" } | Select-Object -First 1
        if (-not $matchingAppliedRecord) {
            throw "Standalone core apply history does not contain an applied record"
        }

        Write-Host "Standalone panel smoke passed at $baseUrl"
        Write-Host "Revision: $($apply.revision)"
    } finally {
        if ($process -and -not $process.HasExited) {
            Stop-Process -Id $process.Id -Force
            $process.WaitForExit()
        }
    }
} finally {
    Pop-Location
    if (-not $KeepData) {
        Remove-Item -LiteralPath $dataPath -Recurse -Force -ErrorAction SilentlyContinue
    } else {
        Write-Host "Smoke data kept at $dataPath"
    }
}

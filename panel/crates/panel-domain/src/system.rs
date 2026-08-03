use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskStats {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemOverview {
    pub memory_budget_mb: usize,
    pub memory_used_bytes: u64,
    pub memory_total_bytes: u64,
    pub disk: DiskStats,
    pub operational_log_lines_buffered: usize,
    pub core_status: CoreStatus,
    pub active_alerts: Vec<ActiveAlert>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResourceBudgetStatus {
    Ok,
    Warning,
    OverLimit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceBudgetItem {
    pub name: String,
    pub used: usize,
    pub limit: usize,
    pub percent_used: u8,
    pub status: ResourceBudgetStatus,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceBudgetReport {
    pub schema_version: u16,
    pub generated_at_unix: u64,
    pub memory_budget_mb: usize,
    pub process_memory_used_bytes: u64,
    pub process_memory_budget_bytes: u64,
    pub process_memory_percent_of_budget: u8,
    pub process_cpu_usage_percent: f32,
    pub target_vcpu: u8,
    pub target_disk_gb: u8,
    pub status: ResourceBudgetStatus,
    pub items: Vec<ResourceBudgetItem>,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreRuntimeState {
    pub status: CoreStatus,
    pub last_action: Option<CoreActionRecord>,
    pub applied_revision: Option<String>,
    pub last_xray_update: Option<XrayCoreUpdateReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreConfigState {
    pub config: String,
    pub saved_at_unix: Option<u64>,
    pub valid_json: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveCoreConfigRequest {
    pub config: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationalLogLine {
    pub created_at_unix: u64,
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OperationalLogsQuery {
    pub level: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemThresholds {
    pub disk_warning_percent: u8,
    pub disk_critical_percent: u8,
    pub memory_warning_percent: u8,
    pub memory_critical_percent: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSystemThresholdsRequest {
    pub disk_warning_percent: u8,
    pub disk_critical_percent: u8,
    pub memory_warning_percent: u8,
    pub memory_critical_percent: u8,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SecretKeyKind {
    Admin,
    Node,
    Telegram,
    RouteMaterials,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SecretKeySource {
    Environment,
    KeyFile,
    KeyFilePending,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SecretKeyReadinessStatus {
    Ready,
    Pending,
    Invalid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretKeyReadinessItem {
    pub kind: SecretKeyKind,
    pub env_var_name: String,
    pub key_path: String,
    pub source: SecretKeySource,
    pub status: SecretKeyReadinessStatus,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretKeyReadinessView {
    pub ready: bool,
    pub generated_at_unix: u64,
    pub items: Vec<SecretKeyReadinessItem>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum AlertKind {
    DiskUsage,
    MemoryUsage,
    PanelMemoryBudget,
    NodeOffline,
    NodeStaleHeartbeat,
    NodeConfigDrift,
    NodeProvisioningStale,
    NodeProvisioningFailed,
    NodeReportedApplyFailed,
    NodeRuntimeAlert,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AlertSeverity {
    Warning,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveAlert {
    pub kind: AlertKind,
    pub severity: AlertSeverity,
    pub message: String,
    pub observed_percent: u8,
    pub threshold_percent: u8,
    #[serde(default)]
    pub observed_value: Option<u64>,
    #[serde(default)]
    pub threshold_value: Option<u64>,
    pub first_seen_at_unix: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AlertEventStatus {
    Activated,
    Resolved,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertEvent {
    pub kind: AlertKind,
    pub severity: AlertSeverity,
    pub status: AlertEventStatus,
    pub observed_percent: u8,
    pub threshold_percent: u8,
    #[serde(default)]
    pub observed_value: Option<u64>,
    #[serde(default)]
    pub threshold_value: Option<u64>,
    pub created_at_unix: u64,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AlertHistoryQuery {
    pub kind: Option<AlertKind>,
    pub severity: Option<AlertSeverity>,
    pub status: Option<AlertEventStatus>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoreAction {
    Start,
    Stop,
    Restart,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CoreActionResult {
    Completed,
    Blocked,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreActionRecord {
    pub action: CoreAction,
    pub created_at_unix: u64,
    #[serde(default)]
    pub result: Option<CoreActionResult>,
    #[serde(default)]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreActionRequest {
    pub action: CoreAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XrayCoreUpdateRequest {
    pub target_version: Option<String>,
    #[serde(default)]
    pub allow_prerelease: bool,
    #[serde(default)]
    pub confirm_binary_swap: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XrayCoreUpdateReport {
    pub requested_at_unix: u64,
    pub status: XrayCoreUpdateStatus,
    pub target_version: Option<String>,
    pub selected_asset_name: Option<String>,
    pub selected_asset_url: Option<String>,
    pub binary_path: Option<String>,
    pub work_dir: String,
    pub downloaded_archive_path: Option<String>,
    pub downloaded_sha256: Option<String>,
    pub candidate_binary_path: Option<String>,
    pub candidate_version: Option<String>,
    pub active_backup_path: Option<String>,
    pub post_swap_version: Option<String>,
    pub detail: String,
    pub stages: Vec<XrayCoreUpdateStageRecord>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum XrayCoreUpdateStatus {
    Planned,
    Swapped,
    Failed,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XrayCoreUpdateStageRecord {
    pub stage: XrayCoreUpdateStage,
    pub status: CoreApplyStageStatus,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum XrayCoreUpdateStage {
    Preflight,
    ReleaseResolved,
    AssetSelected,
    DownloadPrepared,
    Downloaded,
    Extracted,
    CandidateVersion,
    CandidateConfigTest,
    BinarySwap,
    ConfigTest,
    RestartGate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreApplyRequest {
    pub revision: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreApplyRecord {
    pub revision: String,
    pub created_at_unix: u64,
    pub result: CoreApplyResult,
    pub detail: String,
    #[serde(default)]
    pub stages: Vec<CoreApplyStageRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreApplyStageRecord {
    pub stage: CoreApplyStage,
    pub status: CoreApplyStageStatus,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CoreApplyStage {
    Generated,
    InternalValidated,
    ExternalValidated,
    RuntimeStateUpdated,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CoreApplyStageStatus {
    Passed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CoreApplyResult {
    Applied,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CoreApplyHistoryQuery {
    pub result: Option<CoreApplyResult>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoreStatus {
    Idle,
    Running,
    Restarting,
    Failed,
}

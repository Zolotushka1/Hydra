use serde::{Deserialize, Serialize};

use crate::{network::ProtocolRuntimeOwner, xray::XrayExternalValidationReport};

use crate::configgen::GeneratedCoreConfigPreview;

/// A node runtime component.
///
/// Arrives from `/api/nodes/{node_id}/local/runtime-components/{component}/{action}`,
/// that is, straight from the request. Parsing into a type at the boundary means
/// an unknown value never reaches business logic at all.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeComponent {
    Xray,
    Hysteria2Sidecar,
    WireguardNodeNative,
}

/// An action on a runtime component. The second segment of the same path.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeComponentAction {
    Install,
    Update,
    Validate,
    Start,
    Stop,
    Restart,
    Status,
    Logs,
}

impl RuntimeComponent {
    /// Parses a path segment. The string form matches serde's because it comes
    /// from `ALL` rather than a separate list of literals.
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|candidate| candidate.as_str() == value)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Xray => "xray",
            Self::Hysteria2Sidecar => "hysteria2_sidecar",
            Self::WireguardNodeNative => "wireguard_node_native",
        }
    }
}

impl RuntimeComponentAction {
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|candidate| candidate.as_str() == value)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Update => "update",
            Self::Validate => "validate",
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
            Self::Status => "status",
            Self::Logs => "logs",
        }
    }
}

/// A stage in the node configuration apply timeline.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeApplyTimelineStage {
    FetchRuntimeConfig,
    FetchRouteCredentials,
    RenderXrayConfig,
    ValidateXrayConfig,
    WriteRuntimeState,
    RestartXray,
    ReportSync,
    ReportApplyResult,
}

impl NodeApplyTimelineStage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FetchRuntimeConfig => "fetch_runtime_config",
            Self::FetchRouteCredentials => "fetch_route_credentials",
            Self::RenderXrayConfig => "render_xray_config",
            Self::ValidateXrayConfig => "validate_xray_config",
            Self::WriteRuntimeState => "write_runtime_state",
            Self::RestartXray => "restart_xray",
            Self::ReportSync => "report_sync",
            Self::ReportApplyResult => "report_apply_result",
        }
    }
}

/// A node condition flag shown in the health centre.
///
/// These used to be free-form strings: a typo in one of the producing sites
/// yielded a flag the frontend would silently fail to render.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeHealthFlag {
    Disabled,
    Offline,
    Degraded,
    UnknownStatus,
    StaleHeartbeat,
    StaleMetrics,
    ConfigDrift,
    ApplyPending,
    RetryBackoffActive,
    RollbackAvailable,
    ReportedApplyFailed,
    ProvisioningRunning,
    ProvisioningStale,
    ProvisioningFailed,
    RuntimeAlertsActive,
    DiskHigh,
    MemoryHigh,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeStatus {
    Unknown,
    Healthy,
    Degraded,
    Offline,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeSyncStatus {
    Unknown,
    Synced,
    Drifted,
    Pending,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeProvisioningStatus {
    None,
    Pending,
    Running,
    Failed,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub name: String,
    pub address: String,
    pub port: u16,
    pub api_port: u16,
    pub usage_coefficient: f64,
    pub enabled: bool,
    #[serde(skip_serializing)]
    pub auth_token_hash: String,
    #[serde(default)]
    pub auth_token_issued_at_unix: Option<u64>,
    #[serde(skip_serializing)]
    pub local_api_token: Option<String>,
    #[serde(default)]
    pub local_api_token_configured: bool,
    pub xray_version: Option<String>,
    pub node_version: Option<String>,
    pub status: NodeStatus,
    pub sync_status: NodeSyncStatus,
    pub provisioning_status: NodeProvisioningStatus,
    pub last_applied_revision: Option<String>,
    pub last_registered_at_unix: Option<u64>,
    pub last_heartbeat_at_unix: Option<u64>,
    #[serde(default)]
    pub last_agent_heartbeat_at_unix: Option<u64>,
    pub last_metrics_at_unix: Option<u64>,
    pub reported_memory_used_bytes: Option<u64>,
    pub reported_memory_total_bytes: Option<u64>,
    pub reported_disk_used_bytes: Option<u64>,
    pub reported_disk_total_bytes: Option<u64>,
    pub last_sync_at_unix: Option<u64>,
    pub created_at_unix: u64,
    pub updated_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateNodeRequest {
    pub name: String,
    pub address: String,
    pub port: u16,
    pub api_port: u16,
    pub usage_coefficient: f64,
    pub enabled: bool,
    pub local_api_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateNodeRequest {
    pub name: Option<String>,
    pub address: Option<String>,
    pub port: Option<u16>,
    pub api_port: Option<u16>,
    pub usage_coefficient: Option<f64>,
    pub enabled: Option<bool>,
    pub local_api_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeHeartbeatRequest {
    pub xray_version: Option<String>,
    pub node_version: Option<String>,
    pub status: NodeStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeSyncRequest {
    pub sync_status: NodeSyncStatus,
    pub applied_revision: Option<String>,
    pub detail: Option<String>,
    #[serde(default)]
    pub apply_lifecycle_state: Option<NodeApplyLifecycleState>,
    #[serde(default)]
    pub last_good_revision: Option<String>,
    #[serde(default)]
    pub rollback_available: bool,
    #[serde(default)]
    pub apply_stages: Vec<NodeApplyStageView>,
    #[serde(default)]
    pub apply_issues: Vec<XrayRenderIssueView>,
    #[serde(default)]
    pub runtime_components: Vec<NodeReportedRuntimeComponentView>,
    #[serde(default)]
    pub external_xray_validation: Option<XrayExternalValidationReport>,
    #[serde(default)]
    pub retry_state: Option<NodeAgentRetryStateView>,
    #[serde(default)]
    pub runtime_alerts: Vec<NodeRuntimeAlertView>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeRuntimeAlertSeverity {
    Warning,
    Critical,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeRuntimeAlertKind {
    PollBackoff,
    RuntimeValidationFailed,
    XrayRuntimeFailed,
    XrayUpdateFailed,
    SidecarFailed,
    SidecarDegraded,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeRuntimeAlertSource {
    PollLoop,
    RuntimeValidation,
    Xray,
    Sidecar,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeRuntimeAlertView {
    pub alert_id: String,
    pub kind: NodeRuntimeAlertKind,
    pub severity: NodeRuntimeAlertSeverity,
    pub source: NodeRuntimeAlertSource,
    pub active: bool,
    pub detail: String,
    pub observed_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RotateNodeAuthTokenResponse {
    pub node_id: String,
    pub auth_token: String,
    pub generated_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeApplyRequest {
    pub revision: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeApplyRetryRequest {
    pub revision: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRollbackRequest {
    pub target_revision: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeApplyResultStatus {
    Applied,
    Failed,
    RolledBack,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeApplyResultRequest {
    pub attempt_id: String,
    pub target_revision: String,
    pub status: NodeApplyResultStatus,
    pub started_at_unix: Option<u64>,
    pub finished_at_unix: Option<u64>,
    pub applied_revision: Option<String>,
    pub last_good_revision: Option<String>,
    #[serde(default)]
    pub rollback_available: bool,
    #[serde(default)]
    pub safe_to_restart: bool,
    pub detail: Option<String>,
    #[serde(default)]
    pub apply_stages: Vec<NodeApplyStageView>,
    #[serde(default)]
    pub apply_issues: Vec<XrayRenderIssueView>,
    #[serde(default)]
    pub runtime_components: Vec<NodeReportedRuntimeComponentView>,
    #[serde(default)]
    pub external_xray_validation: Option<XrayExternalValidationReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeApplyResultEntry {
    pub node_id: String,
    pub attempt_id: String,
    pub target_revision: String,
    pub status: NodeApplyResultStatus,
    pub started_at_unix: Option<u64>,
    pub finished_at_unix: Option<u64>,
    pub applied_revision: Option<String>,
    pub last_good_revision: Option<String>,
    #[serde(default)]
    pub rollback_available: bool,
    #[serde(default)]
    pub safe_to_restart: bool,
    pub detail: Option<String>,
    #[serde(default)]
    pub apply_stages: Vec<NodeApplyStageView>,
    #[serde(default)]
    pub apply_issues: Vec<XrayRenderIssueView>,
    #[serde(default)]
    pub external_xray_validation: Option<XrayExternalValidationReport>,
    pub recorded_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeSyncHistoryEntry {
    pub node_id: String,
    pub sync_status: NodeSyncStatus,
    pub applied_revision: Option<String>,
    pub expected_revision: String,
    pub detail: Option<String>,
    #[serde(default)]
    pub apply_lifecycle_state: Option<NodeApplyLifecycleState>,
    #[serde(default)]
    pub last_good_revision: Option<String>,
    #[serde(default)]
    pub rollback_available: bool,
    #[serde(default)]
    pub apply_stages: Vec<NodeApplyStageView>,
    #[serde(default)]
    pub apply_issues: Vec<XrayRenderIssueView>,
    #[serde(default)]
    pub retry_state: Option<NodeAgentRetryStateView>,
    #[serde(default)]
    pub runtime_components: Vec<NodeReportedRuntimeComponentView>,
    #[serde(default)]
    pub external_xray_validation: Option<XrayExternalValidationReport>,
    #[serde(default)]
    pub runtime_alerts: Vec<NodeRuntimeAlertView>,
    pub created_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeAgentRetryStateView {
    pub consecutive_failures: u32,
    pub retry_backoff_seconds: Option<u64>,
    pub next_retry_not_before_unix: Option<u64>,
    pub last_transport_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeReportedRuntimeComponentView {
    pub owner: ProtocolRuntimeOwner,
    pub component: String,
    pub installed: bool,
    pub healthy: bool,
    pub version: Option<String>,
    pub last_validated_at_unix: Option<u64>,
    pub last_error: Option<String>,
    pub checked_at_unix: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeAgentIdentity {
    pub node_id: String,
    pub name: String,
    pub status: NodeStatus,
    pub sync_status: NodeSyncStatus,
    pub last_applied_revision: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeAgentConfigResponse {
    pub node_id: String,
    pub revision: String,
    pub apply: NodeAgentApplyDirective,
    pub apply_plan: NodeAgentApplyPlan,
    pub route_credential_status: NodeRouteCredentialStatusView,
    pub runtime_config: NodeRuntimeConfigDocument,
    pub generated_config: GeneratedCoreConfigPreview,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeAgentXrayConfigResponse {
    pub node_id: String,
    pub revision: String,
    pub runtime_config: NodeRuntimeConfigDocument,
    pub apply_plan: NodeAgentApplyPlan,
    pub route_credential_status: NodeRouteCredentialStatusView,
    pub render_summary: XrayRenderSummaryView,
    pub runtime_validation_report: NodeRuntimeValidationReport,
    pub xray_config: crate::xray::XrayConfigDocument,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeAgentApplyDirective {
    pub apply_required: bool,
    pub target_revision: String,
    pub current_applied_revision: Option<String>,
    pub current_sync_status: NodeSyncStatus,
    pub requested_at_unix: Option<u64>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeAgentApplyPlan {
    pub schema_version: u16,
    pub generated_at_unix: u64,
    pub target_revision: String,
    pub apply_required: bool,
    pub least_knowledge: bool,
    pub credential_ref_count: usize,
    pub requires_route_credentials: bool,
    pub requires_xray_validation: bool,
    pub xray_binary_configured: bool,
    pub safe_restart_after_successful_validation: bool,
    #[serde(default)]
    pub runtime_components: Vec<NodeAgentRuntimeComponentRequirement>,
    pub steps: Vec<NodeAgentApplyPlanStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeAgentRuntimeComponentRequirement {
    pub owner: ProtocolRuntimeOwner,
    pub component: RuntimeComponent,
    pub required: bool,
    pub production_ready: bool,
    pub required_binaries: Vec<String>,
    pub validation_strategy: String,
    pub update_strategy: String,
    pub disabled_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeAgentApplyPlanStep {
    pub step: String,
    pub required: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRuntimeValidationReport {
    pub schema_version: u16,
    pub generated_at_unix: u64,
    pub valid: bool,
    pub fail_closed: bool,
    pub safe_to_restart: bool,
    pub issue_count: usize,
    pub issues: Vec<NodeRuntimeValidationIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRuntimeValidationIssue {
    pub scope: String,
    pub severity: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRuntimeConfigDocument {
    pub schema_version: u16,
    pub generated_at_unix: u64,
    pub revision: String,
    pub node: NodeRuntimeIdentity,
    pub apply: NodeAgentApplyDirective,
    pub runtime: NodeRuntimeSettings,
    pub contract: NodeRuntimeContractDiagnostics,
    pub config: NodeRuntimeConfigProjection,
    pub route_assignments: Vec<crate::configgen::NodeRouteAssignment>,
    pub credential_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRuntimeContractDiagnostics {
    pub valid: bool,
    pub fail_closed: bool,
    pub least_knowledge: bool,
    pub node_id: String,
    pub projected_node_ids: Vec<String>,
    pub route_assignment_count: usize,
    pub credential_ref_count: usize,
    pub issue_count: usize,
    pub issues: Vec<NodeRuntimeContractIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRuntimeContractIssue {
    pub scope: String,
    pub severity: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRuntimeIdentity {
    pub node_id: String,
    pub name: String,
    pub address: String,
    pub port: Option<u16>,
    pub api_port: Option<u16>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRuntimeSettings {
    pub xray_config_path: String,
    pub xray_binary_path: Option<String>,
    pub restart_policy: String,
    pub least_knowledge: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRuntimeConfigProjection {
    pub users: Vec<crate::configgen::GeneratedUserConfig>,
    pub inbounds: Vec<crate::network::Inbound>,
    pub hosts: Vec<crate::configgen::GeneratedHost>,
    pub nodes: Vec<NodeRuntimeIdentity>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRouteCredentialBundle {
    pub node_id: String,
    pub revision: String,
    pub generated_at_unix: u64,
    pub credentials: Vec<NodeRouteCredentialMaterial>,
    /// Reality material for this node's inbounds.
    ///
    /// Carried here rather than in `node_runtime_config` because it holds private
    /// keys: this route has `node_agent` exposure, so it never touches the admin
    /// surface and is out of scope for the secret guard by construction. The
    /// field is optional, so an older agent simply does not see it.
    #[serde(default)]
    pub reality_materials: Vec<NodeRealityMaterial>,
}

/// Reality material for one inbound. Secret: for the node agent only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRealityMaterial {
    pub inbound_tag: String,
    /// x25519, base64url without padding. Never appears in admin responses, in
    /// bootstrap, in logs or in the audit trail.
    pub private_key_b64: String,
    pub short_ids: Vec<String>,
    /// Where Reality proxies unrecognised traffic, as `host:port`.
    pub dest: String,
    pub server_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRouteCredentialStatusView {
    pub required_ref_count: usize,
    pub active_ref_count: usize,
    pub revoked_required_refs: Vec<String>,
    pub missing_active_refs: Vec<String>,
    pub safe_to_apply: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRouteCredentialMaterial {
    pub credential_ref: String,
    pub kind: String,
    pub certificate_pem: String,
    pub private_key_pem: String,
    pub ca_certificate_pem: String,
    pub server_name: Option<String>,
    pub certificate_pins: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteMaterialStoreView {
    pub schema_version: u16,
    pub ca_created_at_unix: Option<u64>,
    pub ca_rotated_at_unix: Option<u64>,
    pub credential_count: usize,
    pub active_credential_count: usize,
    pub revoked_credential_count: usize,
    pub revoked_credential_refs: Vec<RouteCredentialRefRevocationView>,
    pub credentials: Vec<RouteCredentialView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteCredentialRefRevocationView {
    pub credential_ref: String,
    pub revoked_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteCredentialView {
    pub credential_ref: String,
    pub kind: String,
    pub server_name: Option<String>,
    pub created_at_unix: u64,
    pub rotated_at_unix: Option<u64>,
    pub revoked_at_unix: Option<u64>,
    pub active: bool,
    pub encrypted_private_key: bool,
    pub certificate_pin_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteCredentialActionRequest {
    pub credential_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeLogUploadLine {
    pub level: String,
    pub message: String,
    pub created_at_unix: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeLogUploadRequest {
    pub lines: Vec<NodeLogUploadLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeMetricsRequest {
    pub memory_used_bytes: u64,
    pub memory_total_bytes: u64,
    pub disk_used_bytes: u64,
    pub disk_total_bytes: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeLocalRuntimeStatus {
    Unknown,
    Stopped,
    Running,
    Failed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeLocalXrayUpdateStatus {
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRuntimeEventEntry {
    pub kind: String,
    pub detail: String,
    pub created_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeApplyHistoryEntryView {
    pub revision: Option<String>,
    pub status: NodeSyncStatus,
    pub detail: String,
    pub created_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XrayRenderSummaryView {
    pub renderer_version: u16,
    pub source_revision: String,
    pub xray_detected_version: Option<String>,
    pub feature_flags: Vec<String>,
    pub inbound_count: usize,
    pub outbound_count: usize,
    pub routing_rule_count: usize,
    pub fail_closed: bool,
    #[serde(default)]
    pub issue_count: usize,
    #[serde(default)]
    pub issues: Vec<XrayRenderIssueView>,
    pub created_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XrayRenderIssueView {
    pub route_id: String,
    pub scope: String,
    pub severity: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeLocalStateView {
    pub node_id: Option<String>,
    pub status: NodeStatus,
    pub applied_revision: Option<String>,
    pub last_successful_tick_at_unix: Option<u64>,
    pub consecutive_tick_failures: u32,
    pub last_error: Option<String>,
    pub last_apply_detail: Option<String>,
    pub xray_detected_version: Option<String>,
    pub last_xray_update_at_unix: Option<u64>,
    pub last_xray_update_detail: Option<String>,
    pub last_xray_update_status: Option<NodeLocalXrayUpdateStatus>,
    pub last_xray_update_phase: Option<String>,
    pub last_xray_update_target_version: Option<String>,
    pub last_xray_update_source_release: Option<String>,
    pub last_xray_update_backup_path: Option<String>,
    pub last_config_backup_path: Option<String>,
    pub rollback_marker_path: Option<String>,
    pub last_config_saved_at_unix: Option<u64>,
    pub last_metrics_reported_at_unix: Option<u64>,
    pub last_sync_reported_at_unix: Option<u64>,
    pub xray_runtime_status: NodeLocalRuntimeStatus,
    pub xray_last_action: Option<String>,
    pub xray_last_detail: Option<String>,
    pub xray_last_pid: Option<u32>,
    pub xray_last_exit_code: Option<i32>,
    pub xray_restart_attempts: u32,
    pub xray_next_restart_not_before_unix: Option<u64>,
    pub xray_last_started_at_unix: Option<u64>,
    pub xray_last_stopped_at_unix: Option<u64>,
    pub xray_last_validated_at_unix: Option<u64>,
    pub buffered_log_count: usize,
    pub last_xray_render_summary: Option<XrayRenderSummaryView>,
    pub apply_history: Vec<NodeApplyHistoryEntryView>,
    pub runtime_events: Vec<NodeRuntimeEventEntry>,
    #[serde(default)]
    pub runtime_components: Vec<NodeReportedRuntimeComponentView>,
    #[serde(default)]
    pub external_xray_validation: Option<XrayExternalValidationReport>,
    #[serde(default)]
    pub runtime_alerts: Vec<NodeRuntimeAlertView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeLocalHealthView {
    pub status: String,
    pub node_id: Option<String>,
    pub applied_revision: Option<String>,
    pub consecutive_tick_failures: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeLocalActionResponse {
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeDiagnosticsView {
    pub node: Node,
    pub local_health: Option<NodeLocalHealthView>,
    pub local_state: Option<NodeLocalStateView>,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeApplyStageStatus {
    Ok,
    Warning,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeApplyTimelineStatus {
    Pending,
    Active,
    Ok,
    Warning,
    Failed,
    Skipped,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeApplyStageView {
    pub stage: String,
    pub status: NodeApplyStageStatus,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeApplyTimelineItem {
    pub phase: NodeApplyTimelineStage,
    pub status: NodeApplyTimelineStatus,
    pub detail: String,
    pub source: String,
    pub observed_at_unix: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeApplyLifecycleState {
    Unknown,
    Pending,
    Downloaded,
    Rendered,
    Validated,
    Applied,
    Failed,
    RolledBack,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeApplyLifecycleView {
    pub state: NodeApplyLifecycleState,
    pub revision: String,
    pub panel_expected_revision: String,
    pub panel_last_applied_revision: Option<String>,
    pub local_applied_revision: Option<String>,
    pub last_good_revision: Option<String>,
    pub rollback_available: bool,
    pub safe_to_restart: bool,
    pub last_reported_at_unix: Option<u64>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeApplyStatusView {
    pub node: Node,
    pub generated_revision: String,
    pub panel_last_applied_revision: Option<String>,
    pub local_applied_revision: Option<String>,
    pub checked_at_unix: u64,
    pub synced: bool,
    pub local_state_available: bool,
    pub local_render_summary: Option<XrayRenderSummaryView>,
    pub recent_sync_history: Vec<NodeSyncHistoryEntry>,
    pub recent_apply_results: Vec<NodeApplyResultEntry>,
    pub lifecycle: NodeApplyLifecycleView,
    pub stages: Vec<NodeApplyStageView>,
    pub timeline: Vec<NodeApplyTimelineItem>,
    pub blocking_issues: Vec<String>,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NodeHealthCenterSummary {
    pub total: usize,
    pub enabled: usize,
    pub disabled: usize,
    pub healthy: usize,
    pub degraded: usize,
    pub offline: usize,
    pub unknown: usize,
    pub synced: usize,
    pub drifted: usize,
    pub pending: usize,
    pub provisioning_running: usize,
    pub provisioning_stale: usize,
    pub provisioning_failed: usize,
    pub reported_apply_failed: usize,
    pub retry_backoff_active: usize,
    pub apply_rollback_available: usize,
    pub runtime_alerts_active: usize,
    pub runtime_alerts_critical: usize,
    pub stale_heartbeat: usize,
    pub stale_metrics: usize,
    pub attention_required: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeHealthCenterItem {
    pub node_id: String,
    pub name: String,
    pub enabled: bool,
    pub status: NodeStatus,
    pub sync_status: NodeSyncStatus,
    pub provisioning_status: NodeProvisioningStatus,
    pub last_applied_revision: Option<String>,
    pub last_heartbeat_at_unix: Option<u64>,
    pub last_metrics_at_unix: Option<u64>,
    pub reported_memory_percent: Option<f64>,
    pub reported_disk_percent: Option<f64>,
    pub reported_apply_failed: bool,
    pub reported_apply_failed_stage_count: usize,
    pub reported_apply_error_issue_count: usize,
    pub latest_retry_state: Option<NodeAgentRetryStateView>,
    pub retry_backoff_active: bool,
    pub latest_apply_status: Option<NodeApplyResultStatus>,
    pub latest_apply_target_revision: Option<String>,
    pub latest_apply_recorded_at_unix: Option<u64>,
    pub latest_successful_apply_revision: Option<String>,
    pub latest_failed_apply_revision: Option<String>,
    pub rollback_available: bool,
    pub runtime_alert_count: usize,
    pub runtime_critical_alert_count: usize,
    pub latest_runtime_alerts: Vec<NodeRuntimeAlertView>,
    pub health_flags: Vec<NodeHealthFlag>,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeHealthCenterView {
    pub generated_revision: String,
    pub checked_at_unix: u64,
    pub summary: NodeHealthCenterSummary,
    pub nodes: Vec<NodeHealthCenterItem>,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeBootstrapStepStatus {
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeBootstrapProbeEntry {
    #[serde(default)]
    pub probe_id: String,
    pub node_id: String,
    pub step: String,
    pub status: NodeBootstrapStepStatus,
    pub detail: String,
    pub created_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeBootstrapProbeView {
    pub probe_id: String,
    pub node: Node,
    pub ready: bool,
    pub checked_at_unix: u64,
    pub steps: Vec<NodeBootstrapProbeEntry>,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeBootstrapReadinessView {
    pub node: Node,
    pub ready: bool,
    pub checked_at_unix: u64,
    pub failed_steps: Vec<String>,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedNodes {
    pub nodes: Vec<Node>,
}

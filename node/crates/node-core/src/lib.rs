use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    env::consts::{ARCH, OS},
    fs,
    io::{Cursor, Read, Write},
    net::SocketAddr,
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use anyhow::{Context, Result, bail};
use hmac::{Hmac, Mac};
use node_config::NodeConfig;
use node_domain::{
    ClusterNodeRole, CompleteLocalSubscriptionSessionEnforcementRequest,
    GeneratedClusterNodeTarget, GeneratedCoreConfig, GeneratedHost, GeneratedInbound,
    GeneratedProxyProfile, LocalSubscriptionSessionAdapterLeaseView,
    LocalSubscriptionSessionEnforcementCommand, NodeAgentConfigResponse, NodeAgentIdentity,
    NodeHeartbeatRequest, NodeLogUploadLine, NodeLogUploadRequest, NodeMetricsRequest,
    NodeReportedRuntimeComponentView, NodeRouteAssignment, NodeRouteCredentialBundle,
    NodeRouteSecurityMode, NodeStatus, NodeSyncRequest, NodeSyncStatus, ProtocolRuntimeOwner,
    RegisterLocalSubscriptionSessionAdapterRequest,
    ReportSubscriptionSessionEnforcementResultRequest, ReportSubscriptionSessionsRequest,
    ReportSubscriptionSessionsResponse, SubscriptionSessionAdapterStatus,
    SubscriptionSessionAdapterView, SubscriptionSessionEnforcementStatus,
    SubscriptionSessionObservation, SubscriptionSessionObservationSource,
    SubscriptionSessionRuntimeAdapter, SubscriptionSessionRuntimeCapability,
    WireGuardSessionInterfaceMapping, WireGuardSessionMappingDocument, WireGuardSessionPeerMapping,
    XrayExternalValidationReport, XrayExternalValidationStatus,
};
use reqwest::header::{HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sysinfo::{Disks, System};
use tokio::process::{Child, Command};
use tracing::{info, warn};

const MAX_XRAY_RENDER_ISSUES: usize = 64;
const MAX_RUNTIME_PROTOCOL_REQUIREMENTS: usize = 128;
const MAX_SIDECAR_STATE_LOGS: usize = 16;
const MAX_RUNTIME_ALERTS: usize = 32;
const MAX_RUNTIME_ALERT_DETAIL_LEN: usize = 512;
const MAX_XRAY_DOWNLOAD_BYTES: u64 = 64 * 1024 * 1024;
const MAX_RUNTIME_STATS_RESPONSE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PersistedNodeState {
    node_id: Option<String>,
    applied_revision: Option<String>,
    last_config_saved_at_unix: Option<u64>,
    last_runtime_config_saved_at_unix: Option<u64>,
    last_sidecar_runtime_config_saved_at_unix: Option<u64>,
    last_xray_config_saved_at_unix: Option<u64>,
    last_route_credentials_saved_at_unix: Option<u64>,
    last_cluster_targets_saved_at_unix: Option<u64>,
    last_metrics_reported_at_unix: Option<u64>,
    last_sync_reported_at_unix: Option<u64>,
    last_successful_tick_at_unix: Option<u64>,
    consecutive_tick_failures: u32,
    last_error: Option<String>,
    last_apply_detail: Option<String>,
    xray_detected_version: Option<String>,
    last_xray_update_at_unix: Option<u64>,
    last_xray_update_detail: Option<String>,
    last_xray_update_status: Option<XrayUpdateStatus>,
    last_xray_update_phase: Option<String>,
    last_xray_update_target_version: Option<String>,
    last_xray_update_source_release: Option<String>,
    last_xray_update_backup_path: Option<String>,
    last_config_backup_path: Option<String>,
    rollback_marker_path: Option<String>,
    xray_runtime: XrayRuntimeState,
    #[serde(default)]
    cluster_targets: Vec<GeneratedClusterNodeTarget>,
    #[serde(default)]
    cluster_runtime_intents: Vec<ClusterRuntimeIntent>,
    #[serde(default)]
    node_route_assignments: Vec<NodeRouteAssignment>,
    last_xray_render_summary: Option<XrayRenderSummary>,
    last_sidecar_runtime_summary: Option<SidecarRuntimeSummary>,
    #[serde(default)]
    last_subscription_sessions_report_at_unix: Option<u64>,
    #[serde(default)]
    last_subscription_sessions_reported_count: usize,
    #[serde(default)]
    last_subscription_sessions_blocked_count: usize,
    #[serde(default)]
    last_runtime_activity_collected_at_unix: Option<u64>,
    #[serde(default)]
    last_runtime_activity_error: Option<String>,
    #[serde(default)]
    last_runtime_protocol_requirements: Vec<RuntimeProtocolRequirement>,
    #[serde(default)]
    sidecar_states: Vec<PersistedSidecarState>,
    #[serde(default)]
    last_accepted_sidecar_executor_session: Option<PersistedSidecarExecutorSessionAcceptance>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyHistoryEntry {
    pub revision: Option<String>,
    pub status: NodeSyncStatus,
    pub detail: String,
    pub created_at_unix: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum XrayRuntimeStatus {
    Unknown,
    Stopped,
    Running,
    Failed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum XrayUpdateStatus {
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct XrayRuntimeState {
    status: Option<XrayRuntimeStatus>,
    last_action: Option<String>,
    last_detail: Option<String>,
    last_pid: Option<u32>,
    last_exit_code: Option<i32>,
    restart_attempts: u32,
    next_restart_not_before_unix: Option<u64>,
    last_started_at_unix: Option<u64>,
    last_stopped_at_unix: Option<u64>,
    last_validated_at_unix: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct PersistedSidecarState {
    sidecar: LocalSidecarKind,
    status: LocalSidecarStatus,
    supported: bool,
    binary_path: Option<String>,
    detected_version: Option<String>,
    last_action: Option<LocalSidecarAction>,
    last_detail: Option<String>,
    last_validated_at_unix: Option<u64>,
    updated_at_unix: Option<u64>,
    #[serde(default)]
    logs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct PersistedSidecarExecutorSessionAcceptance {
    session_id: String,
    source_revision: Option<String>,
    accepted_at_unix: u64,
    #[serde(default)]
    command_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SidecarHelperPreflight {
    ready: bool,
    detail: String,
}

#[derive(Debug, Clone, Copy)]
pub enum LocalRuntimeAction {
    Validate,
    Start,
    Stop,
    Restart,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LocalSidecarKind {
    Hysteria2,
    WireGuard,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LocalSidecarAction {
    Install,
    Update,
    Validate,
    Start,
    Stop,
    Restart,
    Status,
    Logs,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LocalSidecarStatus {
    Disabled,
    Missing,
    Degraded,
    Ready,
    Running,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalSidecarLifecycleResponse {
    pub sidecar: LocalSidecarKind,
    pub action: LocalSidecarAction,
    pub status: LocalSidecarStatus,
    pub supported: bool,
    pub plan: LocalSidecarCommandPlan,
    pub acceptance: LocalSidecarAcceptanceContract,
    pub binary_path: Option<String>,
    pub detected_version: Option<String>,
    pub validated_at_unix: Option<u64>,
    pub detail: String,
    pub logs: Vec<String>,
    pub updated_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalSidecarCommandPlan {
    pub executor_required: bool,
    pub command_id: String,
    pub command_kind: String,
    pub dry_run: bool,
    pub steps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalSidecarAcceptanceContract {
    pub expected_status: LocalSidecarStatus,
    pub required_checks: Vec<String>,
    pub fail_closed: bool,
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalSidecarExecutorResultRequest {
    pub command_id: String,
    pub status: LocalSidecarStatus,
    #[serde(default)]
    pub completed_checks: Vec<String>,
    pub exit_code: Option<i32>,
    pub detail: Option<String>,
    pub completed_at_unix: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalSidecarExecutorResultResponse {
    pub sidecar: LocalSidecarKind,
    pub action: LocalSidecarAction,
    pub command_id: String,
    pub accepted: bool,
    pub status: LocalSidecarStatus,
    pub failed_checks: Vec<String>,
    pub detail: String,
    pub updated_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalSidecarStateView {
    pub sidecar: LocalSidecarKind,
    pub status: LocalSidecarStatus,
    pub supported: bool,
    pub binary_path: Option<String>,
    pub detected_version: Option<String>,
    pub last_action: Option<LocalSidecarAction>,
    pub last_detail: Option<String>,
    pub last_validated_at_unix: Option<u64>,
    pub updated_at_unix: Option<u64>,
    pub logs: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalNodeSnapshot {
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
    pub last_xray_update_status: Option<XrayUpdateStatus>,
    pub last_xray_update_phase: Option<String>,
    pub last_xray_update_target_version: Option<String>,
    pub last_xray_update_source_release: Option<String>,
    pub last_xray_update_backup_path: Option<String>,
    pub last_config_backup_path: Option<String>,
    pub rollback_marker_path: Option<String>,
    pub last_config_saved_at_unix: Option<u64>,
    pub last_runtime_config_saved_at_unix: Option<u64>,
    pub local_runtime_config_path: String,
    pub last_sidecar_runtime_config_saved_at_unix: Option<u64>,
    pub local_sidecar_runtime_config_path: String,
    pub last_xray_config_saved_at_unix: Option<u64>,
    pub local_xray_config_path: String,
    pub last_route_credentials_saved_at_unix: Option<u64>,
    pub last_cluster_targets_saved_at_unix: Option<u64>,
    pub last_metrics_reported_at_unix: Option<u64>,
    pub last_sync_reported_at_unix: Option<u64>,
    pub xray_runtime_status: XrayRuntimeStatus,
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
    pub cluster_target_count: usize,
    pub cluster_targets: Vec<GeneratedClusterNodeTarget>,
    pub cluster_runtime_intent_count: usize,
    pub cluster_runtime_intents: Vec<ClusterRuntimeIntent>,
    pub node_route_assignment_count: usize,
    pub node_route_assignments: Vec<NodeRouteAssignment>,
    pub subscription_session_adapter: SubscriptionSessionAdapterView,
    pub last_xray_render_summary: Option<XrayRenderSummary>,
    pub last_sidecar_runtime_summary: Option<SidecarRuntimeSummary>,
    pub apply_history: Vec<ApplyHistoryEntry>,
    pub runtime_events: Vec<RuntimeEventEntry>,
    pub runtime_validation_report: RuntimeValidationReport,
    pub runtime_artifacts: Vec<RuntimeArtifactView>,
    pub runtime_components: Vec<NodeReportedRuntimeComponentView>,
    pub external_xray_validation: Option<XrayExternalValidationReport>,
    pub runtime_alerts: Vec<RuntimeAlert>,
    pub sidecars: Vec<LocalSidecarStateView>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RuntimeArtifactView {
    pub kind: RuntimeArtifactKind,
    pub path: String,
    pub exists: bool,
    pub last_saved_at_unix: Option<u64>,
    pub executable_runtime_input: bool,
    pub secret_sensitive: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeArtifactKind {
    GeneratedConfig,
    NodeRuntimeConfig,
    SidecarRuntimeConfig,
    Hysteria2ConfigDirectory,
    WireGuardConfigDirectory,
    WireGuardSessionMapping,
    XrayConfig,
    RouteCredentialManifest,
    RouteCredentialDirectory,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeValidationReport {
    pub generated_at_unix: u64,
    pub ready: bool,
    pub component_count: usize,
    pub components: Vec<RuntimeComponentReport>,
    pub protocol_count: usize,
    pub protocols: Vec<RuntimeProtocolReport>,
    pub required_protocol_count: usize,
    pub required_protocols: Vec<RuntimeProtocolRequirementStatus>,
    pub sidecar_runtime: SidecarRuntimeValidationReport,
    pub disabled_reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SidecarRuntimeValidationReport {
    pub config_path: String,
    pub summary: Option<SidecarRuntimeSummary>,
    pub requirement_count: usize,
    pub blocked_count: usize,
    pub executor_session: SidecarExecutorSessionSummary,
    pub requirements: Vec<SidecarRuntimeRequirement>,
    pub ready: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SidecarExecutorSession {
    pub schema_version: u16,
    pub session_id: String,
    pub source_revision: Option<String>,
    pub created_at_unix: u64,
    pub requirement_count: usize,
    pub envelope_count: usize,
    pub executable: bool,
    pub fail_closed: bool,
    pub acceptance: SidecarExecutorSessionAcceptance,
    pub requirements: Vec<SidecarRuntimeRequirement>,
    pub envelopes: Vec<SidecarRuntimeExecutorEnvelope>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SidecarExecutorSessionSummary {
    pub session_id: String,
    pub source_revision: Option<String>,
    pub requirement_count: usize,
    pub envelope_count: usize,
    pub executable: bool,
    pub fail_closed: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SidecarExecutorSessionAcceptance {
    pub required_envelope_count: usize,
    pub required_command_ids: Vec<String>,
    pub fail_closed: bool,
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SidecarExecutorSessionResultRequest {
    pub session_id: String,
    #[serde(default)]
    pub results: Vec<LocalSidecarExecutorResultRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SidecarExecutorSessionResultResponse {
    pub session_id: String,
    pub accepted: bool,
    pub expected_envelope_count: usize,
    pub accepted_count: usize,
    pub rejected_count: usize,
    pub failed_checks: Vec<String>,
    pub detail: String,
    pub updated_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RuntimeComponentReport {
    pub component: RuntimeComponentKind,
    pub required: bool,
    pub readiness: RuntimeComponentReadiness,
    pub binary_path: Option<String>,
    pub detected_version: Option<String>,
    pub last_validated_at_unix: Option<u64>,
    pub last_error: Option<String>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeProtocolReport {
    pub protocol: RuntimeProtocolKind,
    pub readiness: RuntimeProtocolReadiness,
    pub required_components: Vec<RuntimeComponentKind>,
    pub disabled_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RuntimeProtocolRequirementStatus {
    pub requirement: RuntimeProtocolRequirement,
    pub readiness: RuntimeProtocolReadiness,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeProtocolKind {
    VlessTlsWebSocket,
    Hysteria2,
    WireGuard,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeProtocolReadiness {
    Ready,
    Blocked,
    Disabled,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeComponentKind {
    Xray,
    Hysteria2,
    WireGuard,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeComponentReadiness {
    Ready,
    Missing,
    Failed,
    Disabled,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct XrayRenderSummary {
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
    pub issues: Vec<XrayRenderIssue>,
    pub created_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ClusterRuntimeIntent {
    pub cluster_id: String,
    pub cluster_name: String,
    pub cluster_revision: String,
    pub local_cluster_node_ids: Vec<String>,
    pub roles: Vec<String>,
    pub upstream_node_ids: Vec<String>,
    pub downstream_node_ids: Vec<String>,
    pub route_edge_ids: Vec<String>,
    pub accepts_client_entry: bool,
    pub relays_cluster_traffic: bool,
    pub handles_cluster_egress: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRuntimeConfigDocument {
    pub schema_version: u16,
    pub node_id: Option<String>,
    pub source_revision: String,
    pub source_generated_at_unix: u64,
    pub created_at_unix: u64,
    pub source_user_count: usize,
    pub source_node_count: usize,
    pub users: Vec<NodeRuntimeUserConfig>,
    pub inbounds: Vec<GeneratedInbound>,
    pub hosts: Vec<GeneratedHost>,
    pub cluster_intents: Vec<ClusterRuntimeIntent>,
    pub route_assignments: Vec<NodeRouteAssignment>,
    #[serde(default)]
    pub required_protocols: Vec<RuntimeProtocolRequirement>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct RuntimeProtocolRequirement {
    pub protocol: RuntimeProtocolKind,
    pub required_component: RuntimeComponentKind,
    pub source: String,
    pub source_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRuntimeUserConfig {
    pub username: String,
    pub proxy_profiles: Vec<GeneratedProxyProfile>,
    pub inbounds: Vec<GeneratedInbound>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SidecarRuntimeConfigDocument {
    pub schema_version: u16,
    pub source_revision: String,
    pub created_at_unix: u64,
    pub requirements: Vec<SidecarRuntimeRequirement>,
    #[serde(default)]
    pub hysteria2_configs: Vec<Hysteria2RuntimeConfig>,
    #[serde(default)]
    pub wireguard_configs: Vec<WireGuardRuntimeConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SidecarRuntimeRequirement {
    pub sidecar: LocalSidecarKind,
    pub protocol: RuntimeProtocolKind,
    pub source: String,
    pub source_ref: String,
    pub status: SidecarRuntimeRequirementStatus,
    pub reason: String,
    #[serde(default)]
    pub planned_envelopes: Vec<SidecarRuntimeExecutorEnvelope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SidecarRuntimeExecutorEnvelope {
    pub sidecar: LocalSidecarKind,
    pub action: LocalSidecarAction,
    pub command_id: String,
    #[serde(default)]
    pub config_path: Option<String>,
    #[serde(default)]
    pub config_exists: bool,
    pub plan: LocalSidecarCommandPlan,
    pub acceptance: LocalSidecarAcceptanceContract,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Hysteria2RuntimeConfig {
    pub tag: String,
    pub listen: String,
    pub port: u16,
    pub auth_users: Vec<Hysteria2RuntimeUser>,
    pub traffic_stats_listen: String,
    pub traffic_stats_secret: String,
    pub certificate_file: String,
    pub key_file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Hysteria2RuntimeUser {
    pub runtime_username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WireGuardRuntimeConfig {
    pub tag: String,
    pub interface_private_key: String,
    pub interface_address: String,
    pub listen_port: Option<u16>,
    pub peers: Vec<WireGuardRuntimePeer>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WireGuardRuntimePeer {
    pub runtime_username: String,
    pub public_key: String,
    pub endpoint: Option<String>,
    pub allowed_ips: Vec<String>,
    pub device_fingerprint: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SidecarRuntimeRequirementStatus {
    Ready,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SidecarRuntimeSummary {
    pub schema_version: u16,
    pub source_revision: String,
    pub requirement_count: usize,
    pub blocked_count: usize,
    pub created_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XrayRenderPlan {
    pub schema_version: u16,
    pub renderer_version: u16,
    pub source_revision: String,
    pub created_at_unix: u64,
    pub xray_detected_version: Option<String>,
    pub feature_flags: Vec<String>,
    #[serde(default)]
    pub issues: Vec<XrayRenderIssue>,
    pub config: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct XrayRenderIssue {
    pub route_id: String,
    pub scope: String,
    pub severity: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RouteCredentialStore {
    #[serde(default)]
    pub credentials: Vec<RouteCredential>,
    /// Reality material for inbounds, received from the panel over the same
    /// channel as mTLS private keys. Carries private keys: never log it.
    #[serde(default)]
    pub reality_materials: Vec<node_domain::NodeRealityMaterial>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteCredential {
    pub credential_ref: String,
    pub kind: String,
    #[serde(default)]
    pub certificate_file: Option<String>,
    #[serde(default)]
    pub private_key_file: Option<String>,
    #[serde(default)]
    pub ca_certificate_file: Option<String>,
    #[serde(default)]
    pub public_key: Option<String>,
    #[serde(default)]
    pub server_name: Option<String>,
    #[serde(default)]
    pub short_id: Option<String>,
    #[serde(default)]
    pub certificate_pins: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeEventEntry {
    pub kind: String,
    pub detail: String,
    pub created_at_unix: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeAlertSeverity {
    Warning,
    Critical,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeAlertKind {
    PollBackoff,
    RuntimeValidationFailed,
    XrayRuntimeFailed,
    XrayUpdateFailed,
    SidecarFailed,
    SidecarDegraded,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeAlertSource {
    PollLoop,
    RuntimeValidation,
    Xray,
    Sidecar,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeAlert {
    pub alert_id: String,
    pub kind: RuntimeAlertKind,
    pub severity: RuntimeAlertSeverity,
    pub source: RuntimeAlertSource,
    pub active: bool,
    pub detail: String,
    pub observed_at_unix: u64,
}

#[derive(Debug, Clone)]
struct StagedSubscriptionSessionObservations {
    adapter_instance_id: String,
    request: ReportSubscriptionSessionsRequest,
    received_at_unix: u64,
}

#[derive(Debug, Clone)]
struct PendingSubscriptionSessionEnforcement {
    adapter_instance_id: String,
    command: LocalSubscriptionSessionEnforcementCommand,
}

#[derive(Debug, Clone, Default)]
struct RuntimeActivityState {
    enabled: bool,
    xray_counters: HashMap<String, u64>,
    xray_last_activity: HashMap<String, u64>,
    hysteria2_online: BTreeMap<String, u64>,
}

#[derive(Debug, Deserialize)]
struct XrayStatsResponse {
    #[serde(default, alias = "stats")]
    stat: Vec<XrayStatEntry>,
}

#[derive(Debug, Deserialize)]
struct XrayStatEntry {
    name: String,
    value: serde_json::Value,
}

#[derive(Debug, Clone)]
struct ActiveSubscriptionSessionAdapterLease {
    adapter_instance_id: String,
    runtime_capabilities: Vec<SubscriptionSessionRuntimeCapability>,
    registered_at_unix: u64,
    lease_expires_at_unix: u64,
}

#[derive(Debug, Deserialize)]
struct GitHubReleaseResponse {
    tag_name: String,
    assets: Vec<GitHubReleaseAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubReleaseAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Clone)]
enum XrayApplyMode {
    Noop,
    ValidateJson,
    ExternalValidateOnly,
    ExternalProcess,
}

#[derive(Debug, Clone)]
struct XrayProcessManager {
    mode: XrayApplyMode,
    binary_path: Option<PathBuf>,
    validate_args: Vec<String>,
    run_args: Vec<String>,
}

impl XrayProcessManager {
    fn from_config(config: &NodeConfig) -> Self {
        let mode = match config.xray_apply_mode.trim().to_ascii_lowercase().as_str() {
            "noop" => XrayApplyMode::Noop,
            "external_validate_only" => XrayApplyMode::ExternalValidateOnly,
            "external_process" => XrayApplyMode::ExternalProcess,
            _ => XrayApplyMode::ValidateJson,
        };

        Self {
            mode,
            binary_path: config.xray_binary_path.clone().map(PathBuf::from),
            validate_args: config.xray_validate_args.clone(),
            run_args: config.xray_run_args.clone(),
        }
    }

    fn apply_mode_name(&self) -> &'static str {
        match self.mode {
            XrayApplyMode::Noop => "noop",
            XrayApplyMode::ValidateJson => "validate_json",
            XrayApplyMode::ExternalValidateOnly => "external_validate_only",
            XrayApplyMode::ExternalProcess => "external_process",
        }
    }

    fn requires_binary(&self) -> bool {
        matches!(
            self.mode,
            XrayApplyMode::ExternalValidateOnly | XrayApplyMode::ExternalProcess
        )
    }

    fn apply_config(&self, config_path: &Path) -> Result<String> {
        match self.mode {
            XrayApplyMode::Noop => Ok("noop apply mode".to_string()),
            XrayApplyMode::ValidateJson => {
                let content = fs::read_to_string(config_path)
                    .with_context(|| format!("failed to read {}", config_path.display()))?;
                serde_json::from_str::<serde_json::Value>(&content)
                    .context("generated config file is not valid JSON")?;
                Ok("config persisted and JSON-validated".to_string())
            }
            XrayApplyMode::ExternalValidateOnly => self.run_validate_command(config_path),
            XrayApplyMode::ExternalProcess => self.run_validate_command(config_path),
        }
    }

    fn run_action(
        &self,
        action: LocalRuntimeAction,
        config_path: &Path,
        runtime: &mut XrayRuntimeState,
        process: &mut Option<Child>,
    ) -> Result<String> {
        match action {
            LocalRuntimeAction::Validate => {
                let detail = self.apply_config(config_path)?;
                runtime.status = Some(match runtime.status.unwrap_or(XrayRuntimeStatus::Stopped) {
                    XrayRuntimeStatus::Running => XrayRuntimeStatus::Running,
                    XrayRuntimeStatus::Unknown
                    | XrayRuntimeStatus::Stopped
                    | XrayRuntimeStatus::Failed => XrayRuntimeStatus::Stopped,
                });
                runtime.last_action = Some("validate".to_string());
                runtime.last_detail = Some(detail.clone());
                runtime.last_validated_at_unix = Some(now_unix());
                Ok(detail)
            }
            LocalRuntimeAction::Start => {
                if matches!(self.mode, XrayApplyMode::ExternalValidateOnly) {
                    let detail = self.apply_config(config_path)?;
                    runtime.status = Some(XrayRuntimeStatus::Stopped);
                    runtime.last_action = Some("validate".to_string());
                    runtime.last_detail = Some(format!(
                        "{detail}; runtime start skipped in external_validate_only mode"
                    ));
                    runtime.last_pid = None;
                    runtime.last_exit_code = None;
                    runtime.last_validated_at_unix = Some(now_unix());
                    return Ok(format!(
                        "{detail}; runtime start skipped in external_validate_only mode"
                    ));
                }
                let detail = self.start_process(config_path, process)?;
                runtime.status = Some(XrayRuntimeStatus::Running);
                runtime.last_action = Some("start".to_string());
                runtime.last_detail = Some(format!("runtime started ({detail})"));
                runtime.last_pid = process.as_ref().and_then(|child| child.id());
                runtime.last_exit_code = None;
                runtime.last_started_at_unix = Some(now_unix());
                runtime.last_validated_at_unix = Some(now_unix());
                Ok(detail)
            }
            LocalRuntimeAction::Stop => {
                self.stop_process(process, runtime)?;
                runtime.status = Some(XrayRuntimeStatus::Stopped);
                runtime.last_action = Some("stop".to_string());
                runtime.last_detail = Some("runtime stopped".to_string());
                runtime.last_pid = None;
                runtime.last_stopped_at_unix = Some(now_unix());
                Ok("runtime stopped".to_string())
            }
            LocalRuntimeAction::Restart => {
                if matches!(self.mode, XrayApplyMode::ExternalValidateOnly) {
                    self.stop_process(process, runtime)?;
                    let detail = self.apply_config(config_path)?;
                    runtime.status = Some(XrayRuntimeStatus::Stopped);
                    runtime.last_action = Some("validate".to_string());
                    runtime.last_detail = Some(format!(
                        "{detail}; runtime restart skipped in external_validate_only mode"
                    ));
                    runtime.last_pid = None;
                    runtime.last_exit_code = None;
                    runtime.last_validated_at_unix = Some(now_unix());
                    return Ok(format!(
                        "{detail}; runtime restart skipped in external_validate_only mode"
                    ));
                }
                self.stop_process(process, runtime)?;
                let detail = self.start_process(config_path, process)?;
                runtime.status = Some(XrayRuntimeStatus::Running);
                runtime.last_action = Some("restart".to_string());
                runtime.last_detail = Some(format!("runtime restarted ({detail})"));
                runtime.last_pid = process.as_ref().and_then(|child| child.id());
                runtime.last_exit_code = None;
                runtime.last_started_at_unix = Some(now_unix());
                runtime.last_validated_at_unix = Some(now_unix());
                Ok(detail)
            }
        }
    }

    fn run_validate_command(&self, config_path: &Path) -> Result<String> {
        let binary = self
            .binary_path
            .as_ref()
            .context("xray external_process mode requires HYDRA_NODE_XRAY_BINARY_PATH")?;

        let validate_args = self.effective_validate_args();

        let output = std::process::Command::new(binary)
            .args(self.expand_args(&validate_args, config_path))
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .with_context(|| format!("failed to run {}", binary.display()))?;

        if output.status.success() {
            Ok("config validated by external xray process".to_string())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            bail!(
                "external xray validation failed{}",
                if stderr.is_empty() {
                    String::new()
                } else {
                    format!(": {stderr}")
                }
            );
        }
    }

    fn start_process(&self, config_path: &Path, process: &mut Option<Child>) -> Result<String> {
        match self.mode {
            XrayApplyMode::Noop
            | XrayApplyMode::ValidateJson
            | XrayApplyMode::ExternalValidateOnly => {
                let detail = self.apply_config(config_path)?;
                Ok(detail)
            }
            XrayApplyMode::ExternalProcess => {
                let binary = self
                    .binary_path
                    .as_ref()
                    .context("xray external_process mode requires HYDRA_NODE_XRAY_BINARY_PATH")?;

                if self.run_args.is_empty() {
                    bail!("xray external_process mode requires HYDRA_NODE_XRAY_RUN_ARGS_JSON");
                }

                let child = tokio::process::Command::new(binary)
                    .args(self.expand_args(&self.run_args, config_path))
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .with_context(|| format!("failed to spawn {}", binary.display()))?;
                *process = Some(child);
                Ok("external xray process spawned".to_string())
            }
        }
    }

    fn stop_process(
        &self,
        process: &mut Option<Child>,
        runtime: &mut XrayRuntimeState,
    ) -> Result<()> {
        if let Some(mut child) = process.take() {
            child.start_kill().context("failed to stop xray process")?;
            runtime.last_exit_code = None;
        }
        Ok(())
    }

    fn expand_args(&self, args: &[String], config_path: &Path) -> Vec<String> {
        let config_path_string = config_path.display().to_string();
        args.iter()
            .map(|arg| arg.replace("{config_path}", &config_path_string))
            .collect()
    }

    fn effective_validate_args(&self) -> Vec<String> {
        if self.validate_args.is_empty() {
            vec![
                "run".to_string(),
                "-test".to_string(),
                "-config".to_string(),
                "{config_path}".to_string(),
            ]
        } else {
            self.validate_args.clone()
        }
    }
}

pub struct NodeRuntime {
    config: NodeConfig,
    client: reqwest::Client,
    runtime_stats_client: reqwest::Client,
    state: PersistedNodeState,
    xray_manager: XrayProcessManager,
    xray_child: Option<Child>,
    apply_history: Vec<ApplyHistoryEntry>,
    runtime_events: Vec<RuntimeEventEntry>,
    buffered_logs: Vec<NodeLogUploadLine>,
    active_subscription_session_adapter: Option<ActiveSubscriptionSessionAdapterLease>,
    staged_subscription_sessions: Option<StagedSubscriptionSessionObservations>,
    runtime_activity: RuntimeActivityState,
    pending_subscription_session_enforcements: Vec<PendingSubscriptionSessionEnforcement>,
}

impl NodeRuntime {
    pub fn new(config: NodeConfig) -> Result<Self> {
        if config.node_token.trim().is_empty() {
            bail!("HYDRA_NODE_TOKEN must be configured");
        }
        if loopback_socket_address(&config.xray_stats_api_address).is_none() {
            bail!("HYDRA_NODE_XRAY_STATS_API_ADDRESS must be a loopback socket address");
        }
        if config.runtime_stats_timeout_seconds == 0 || config.runtime_stats_timeout_seconds > 30 {
            bail!("HYDRA_NODE_RUNTIME_STATS_TIMEOUT_SECONDS must be between 1 and 30");
        }
        if config.runtime_activity_window_seconds == 0
            || config.runtime_activity_window_seconds > 3_600
        {
            bail!("HYDRA_NODE_RUNTIME_ACTIVITY_WINDOW_SECONDS must be between 1 and 3600");
        }
        if config.hysteria2_traffic_stats_base_port == 0 {
            bail!("HYDRA_NODE_HYSTERIA2_TRAFFIC_STATS_BASE_PORT must not be zero");
        }

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Hydra-Node-Token",
            HeaderValue::from_str(&config.node_token).context("invalid node token header")?,
        );

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .context("failed to build reqwest client")?;
        let runtime_stats_client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(config.runtime_stats_timeout_seconds))
            .build()
            .context("failed to build local runtime stats client")?;

        let mut state = load_state(&config.local_state_path)?;
        if state.xray_runtime.status == Some(XrayRuntimeStatus::Running) {
            state.xray_runtime.status = Some(XrayRuntimeStatus::Unknown);
            state.xray_runtime.last_pid = None;
            state.xray_runtime.last_detail = Some(
                "agent restarted and previous external process ownership was lost".to_string(),
            );
        }

        let apply_history = load_apply_history(&config.apply_history_path)?;
        let runtime_events = load_runtime_events(&config.runtime_event_history_path)?;
        let xray_manager = XrayProcessManager::from_config(&config);
        if let Some(binary_path) = config.xray_binary_path.as_ref().map(PathBuf::from)
            && binary_path.is_file()
            && state.xray_detected_version.is_none()
            && let Ok(version) = detect_xray_version_from_binary(&binary_path)
        {
            state.xray_detected_version = Some(version);
        }

        Ok(Self {
            config,
            client,
            runtime_stats_client,
            state,
            xray_manager,
            xray_child: None,
            apply_history,
            runtime_events,
            buffered_logs: Vec::new(),
            active_subscription_session_adapter: None,
            staged_subscription_sessions: None,
            runtime_activity: RuntimeActivityState::default(),
            pending_subscription_session_enforcements: Vec::new(),
        })
    }

    pub fn poll_interval(&self) -> Duration {
        Duration::from_secs(self.config.poll_interval_seconds)
    }

    pub fn next_poll_delay(&self) -> Duration {
        if self.state.consecutive_tick_failures == 0 {
            return self.poll_interval();
        }
        let backoff_seconds = bounded_backoff_seconds(
            self.config.tick_failure_backoff_base_seconds,
            self.config.tick_failure_backoff_max_seconds,
            self.state.consecutive_tick_failures,
        );
        Duration::from_secs(backoff_seconds.max(self.config.poll_interval_seconds))
    }

    pub fn snapshot(&self) -> LocalNodeSnapshot {
        LocalNodeSnapshot {
            node_id: self.state.node_id.clone(),
            status: self.derive_node_status(),
            applied_revision: self.state.applied_revision.clone(),
            last_successful_tick_at_unix: self.state.last_successful_tick_at_unix,
            consecutive_tick_failures: self.state.consecutive_tick_failures,
            last_error: self.state.last_error.clone(),
            last_apply_detail: self.state.last_apply_detail.clone(),
            xray_detected_version: self.state.xray_detected_version.clone(),
            last_xray_update_at_unix: self.state.last_xray_update_at_unix,
            last_xray_update_detail: self.state.last_xray_update_detail.clone(),
            last_xray_update_status: self.state.last_xray_update_status,
            last_xray_update_phase: self.state.last_xray_update_phase.clone(),
            last_xray_update_target_version: self.state.last_xray_update_target_version.clone(),
            last_xray_update_source_release: self.state.last_xray_update_source_release.clone(),
            last_xray_update_backup_path: self.state.last_xray_update_backup_path.clone(),
            last_config_backup_path: self.state.last_config_backup_path.clone(),
            rollback_marker_path: self.state.rollback_marker_path.clone(),
            last_config_saved_at_unix: self.state.last_config_saved_at_unix,
            last_runtime_config_saved_at_unix: self.state.last_runtime_config_saved_at_unix,
            local_runtime_config_path: self.config.local_runtime_config_path.clone(),
            last_sidecar_runtime_config_saved_at_unix: self
                .state
                .last_sidecar_runtime_config_saved_at_unix,
            local_sidecar_runtime_config_path: self
                .config
                .local_sidecar_runtime_config_path
                .clone(),
            last_xray_config_saved_at_unix: self.state.last_xray_config_saved_at_unix,
            local_xray_config_path: self.config.local_xray_config_path.clone(),
            last_route_credentials_saved_at_unix: self.state.last_route_credentials_saved_at_unix,
            last_cluster_targets_saved_at_unix: self.state.last_cluster_targets_saved_at_unix,
            last_metrics_reported_at_unix: self.state.last_metrics_reported_at_unix,
            last_sync_reported_at_unix: self.state.last_sync_reported_at_unix,
            xray_runtime_status: self.current_xray_runtime_status(),
            xray_last_action: self.state.xray_runtime.last_action.clone(),
            xray_last_detail: self.state.xray_runtime.last_detail.clone(),
            xray_last_pid: self.state.xray_runtime.last_pid,
            xray_last_exit_code: self.state.xray_runtime.last_exit_code,
            xray_restart_attempts: self.state.xray_runtime.restart_attempts,
            xray_next_restart_not_before_unix: self.state.xray_runtime.next_restart_not_before_unix,
            xray_last_started_at_unix: self.state.xray_runtime.last_started_at_unix,
            xray_last_stopped_at_unix: self.state.xray_runtime.last_stopped_at_unix,
            xray_last_validated_at_unix: self.state.xray_runtime.last_validated_at_unix,
            buffered_log_count: self.buffered_logs.len(),
            cluster_target_count: self.state.cluster_targets.len(),
            cluster_targets: self.state.cluster_targets.clone(),
            cluster_runtime_intent_count: self.state.cluster_runtime_intents.len(),
            cluster_runtime_intents: self.state.cluster_runtime_intents.clone(),
            node_route_assignment_count: self.state.node_route_assignments.len(),
            node_route_assignments: self.state.node_route_assignments.clone(),
            subscription_session_adapter: self.subscription_session_adapter_view(),
            last_xray_render_summary: self.state.last_xray_render_summary.clone(),
            last_sidecar_runtime_summary: self.state.last_sidecar_runtime_summary.clone(),
            apply_history: self.apply_history.clone(),
            runtime_events: self.runtime_events.clone(),
            runtime_validation_report: self.runtime_validation_report(),
            runtime_artifacts: self.runtime_artifacts(),
            runtime_components: self.reported_runtime_components(),
            external_xray_validation: self.external_xray_validation_report(),
            runtime_alerts: self.runtime_alerts(),
            sidecars: self.sidecar_state_views(),
        }
    }

    pub fn execute_local_runtime_action(&mut self, action: LocalRuntimeAction) -> Result<String> {
        self.refresh_runtime_process_state()?;

        let detail = self.xray_manager.run_action(
            action,
            Path::new(&self.config.local_xray_config_path),
            &mut self.state.xray_runtime,
            &mut self.xray_child,
        );

        match detail {
            Ok(detail) => {
                self.push_log("info", format!("local runtime action succeeded: {detail}"));
                self.record_runtime_event("runtime_action_succeeded", detail.clone());
                persist_state(&self.config.local_state_path, &self.state)?;
                persist_runtime_events(
                    &self.config.runtime_event_history_path,
                    &self.runtime_events,
                    self.config.max_runtime_event_entries,
                )?;
                Ok(detail)
            }
            Err(error) => {
                self.state.xray_runtime.status = Some(XrayRuntimeStatus::Failed);
                self.state.xray_runtime.last_detail = Some(error.to_string());
                self.push_log("error", format!("local runtime action failed: {error}"));
                self.record_runtime_event("runtime_action_failed", error.to_string());
                persist_state(&self.config.local_state_path, &self.state)?;
                persist_runtime_events(
                    &self.config.runtime_event_history_path,
                    &self.runtime_events,
                    self.config.max_runtime_event_entries,
                )?;
                Err(error)
            }
        }
    }

    pub fn execute_local_sidecar_action(
        &mut self,
        sidecar: LocalSidecarKind,
        action: LocalSidecarAction,
    ) -> Result<LocalSidecarLifecycleResponse> {
        let mut response = match action {
            LocalSidecarAction::Status | LocalSidecarAction::Validate => {
                if let Some(args) = self.sidecar_action_args(sidecar, action) {
                    self.execute_configured_sidecar_command(sidecar, action, args)
                } else {
                    self.sidecar_preflight_response(sidecar, action)
                }
            }
            LocalSidecarAction::Install
            | LocalSidecarAction::Update
            | LocalSidecarAction::Start
            | LocalSidecarAction::Stop
            | LocalSidecarAction::Restart
            | LocalSidecarAction::Logs => {
                if let Some(args) = self.sidecar_action_args(sidecar, action) {
                    self.execute_configured_sidecar_command(sidecar, action, args)
                } else {
                    placeholder_sidecar_lifecycle_response(sidecar, action)
                }
            }
        };
        self.update_sidecar_state(&response);
        if action == LocalSidecarAction::Logs {
            let mut logs = response.logs.clone();
            logs.extend(
                self.sidecar_state(sidecar)
                    .map(|state| state.logs.clone())
                    .unwrap_or_default(),
            );
            response.logs = logs;
        }
        self.record_runtime_event(
            "sidecar_lifecycle_unsupported",
            format!(
                "{:?} {:?}: {}",
                response.sidecar, response.action, response.detail
            ),
        );
        persist_state(&self.config.local_state_path, &self.state)?;
        persist_runtime_events(
            &self.config.runtime_event_history_path,
            &self.runtime_events,
            self.config.max_runtime_event_entries,
        )?;
        Ok(response)
    }

    pub fn sidecar_executor_session(&self) -> SidecarExecutorSession {
        let requirements = self.sidecar_runtime_requirements();
        build_sidecar_executor_session(
            self.state.last_sidecar_runtime_summary.as_ref(),
            requirements,
        )
    }

    pub fn complete_sidecar_executor_session(
        &mut self,
        result: SidecarExecutorSessionResultRequest,
    ) -> Result<SidecarExecutorSessionResultResponse> {
        let session = self.sidecar_executor_session();
        let response = validate_sidecar_executor_session_result(&session, &result);
        self.record_runtime_event(
            if response.accepted {
                "sidecar_executor_session_result_accepted"
            } else {
                "sidecar_executor_session_result_rejected"
            },
            response.detail.clone(),
        );
        if response.accepted {
            self.state.last_accepted_sidecar_executor_session =
                Some(PersistedSidecarExecutorSessionAcceptance {
                    session_id: session.session_id.clone(),
                    source_revision: session.source_revision.clone(),
                    accepted_at_unix: response.updated_at_unix,
                    command_ids: session.acceptance.required_command_ids.clone(),
                });
        } else {
            self.state.last_accepted_sidecar_executor_session = None;
            for requirement in &session.requirements {
                let failed = LocalSidecarLifecycleResponse {
                    sidecar: requirement.sidecar,
                    action: LocalSidecarAction::Status,
                    status: LocalSidecarStatus::Failed,
                    supported: false,
                    plan: placeholder_sidecar_command_plan(
                        requirement.sidecar,
                        LocalSidecarAction::Status,
                    ),
                    acceptance: placeholder_sidecar_acceptance_contract(
                        LocalSidecarAction::Status,
                        LocalSidecarStatus::Disabled,
                    ),
                    binary_path: None,
                    detected_version: None,
                    validated_at_unix: None,
                    detail: response.detail.clone(),
                    logs: Vec::new(),
                    updated_at_unix: response.updated_at_unix,
                };
                self.update_sidecar_state(&failed);
            }
        }
        persist_state(&self.config.local_state_path, &self.state)?;
        persist_runtime_events(
            &self.config.runtime_event_history_path,
            &self.runtime_events,
            self.config.max_runtime_event_entries,
        )?;
        Ok(response)
    }

    pub fn complete_local_sidecar_action(
        &mut self,
        sidecar: LocalSidecarKind,
        action: LocalSidecarAction,
        result: LocalSidecarExecutorResultRequest,
    ) -> Result<LocalSidecarExecutorResultResponse> {
        let expected = sidecar_lifecycle_contract_for_result(self, sidecar, action);
        let mut failed_checks = Vec::new();
        if result.command_id != expected.plan.command_id {
            failed_checks.push(format!(
                "command_id mismatch: expected {}, got {}",
                expected.plan.command_id, result.command_id
            ));
        }
        if result.status != expected.acceptance.expected_status {
            failed_checks.push(format!(
                "status mismatch: expected {:?}, got {:?}",
                expected.acceptance.expected_status, result.status
            ));
        }
        if result.exit_code.is_some_and(|exit_code| exit_code != 0) {
            failed_checks.push("exit_code must be 0".to_string());
        }
        for check in &expected.acceptance.required_checks {
            if !result
                .completed_checks
                .iter()
                .any(|completed| completed == check)
            {
                failed_checks.push(format!("required check missing: {check}"));
            }
        }

        let accepted = failed_checks.is_empty();
        let status = if accepted {
            result.status
        } else {
            LocalSidecarStatus::Failed
        };
        let detail = if accepted {
            result.detail.unwrap_or_else(|| {
                format!(
                    "{} {:?} executor result accepted",
                    sidecar_name(sidecar),
                    action
                )
            })
        } else {
            format!(
                "{} {:?} executor result rejected: {}",
                sidecar_name(sidecar),
                action,
                failed_checks.join("; ")
            )
        };
        let response = LocalSidecarLifecycleResponse {
            sidecar,
            action,
            status,
            supported: accepted && expected.supported,
            plan: expected.plan,
            acceptance: expected.acceptance,
            binary_path: expected.binary_path,
            detected_version: expected.detected_version,
            validated_at_unix: accepted
                .then(|| result.completed_at_unix.unwrap_or_else(now_unix))
                .or(expected.validated_at_unix),
            detail: detail.clone(),
            logs: Vec::new(),
            updated_at_unix: now_unix(),
        };
        self.update_sidecar_state(&response);
        self.record_runtime_event(
            if accepted {
                "sidecar_executor_result_accepted"
            } else {
                "sidecar_executor_result_rejected"
            },
            detail.clone(),
        );

        Ok(LocalSidecarExecutorResultResponse {
            sidecar,
            action,
            command_id: result.command_id,
            accepted,
            status,
            failed_checks,
            detail,
            updated_at_unix: response.updated_at_unix,
        })
    }

    pub async fn register_subscription_session_adapter(
        &mut self,
        request: RegisterLocalSubscriptionSessionAdapterRequest,
    ) -> Result<LocalSubscriptionSessionAdapterLeaseView> {
        validate_subscription_session_adapter_registration(&request)?;
        let now = now_unix();
        if let Some(active) = self.active_subscription_session_adapter.as_ref()
            && active.lease_expires_at_unix >= now
            && active.adapter_instance_id != request.adapter_instance_id
        {
            bail!("another local subscription session adapter instance holds an active lease");
        }
        if self
            .active_subscription_session_adapter
            .as_ref()
            .is_some_and(|active| {
                active.adapter_instance_id != request.adapter_instance_id
                    || active.runtime_capabilities != request.runtime_capabilities
            })
        {
            self.fail_all_pending_subscription_session_enforcements(
                "local session adapter lease was replaced before enforcement completion",
            )
            .await?;
            self.staged_subscription_sessions = None;
        }
        let lease = ActiveSubscriptionSessionAdapterLease {
            adapter_instance_id: request.adapter_instance_id,
            runtime_capabilities: request.runtime_capabilities,
            registered_at_unix: now,
            lease_expires_at_unix: now
                .saturating_add(self.config.subscription_session_adapter_lease_seconds),
        };
        let view = LocalSubscriptionSessionAdapterLeaseView {
            adapter_instance_id: lease.adapter_instance_id.clone(),
            runtime_capabilities: lease.runtime_capabilities.clone(),
            registered_at_unix: lease.registered_at_unix,
            lease_expires_at_unix: lease.lease_expires_at_unix,
        };
        self.active_subscription_session_adapter = Some(lease);
        self.record_runtime_event(
            "subscription_session_adapter_registered",
            "registered or renewed trusted local session adapter lease".to_string(),
        );
        Ok(view)
    }

    pub fn stage_subscription_session_observations(
        &mut self,
        adapter_instance_id: &str,
        request: ReportSubscriptionSessionsRequest,
    ) -> Result<SubscriptionSessionAdapterView> {
        self.require_active_subscription_session_adapter(
            adapter_instance_id,
            &request.runtime_capabilities,
        )?;
        validate_subscription_session_observation_snapshot(
            &request,
            self.config.max_subscription_session_observations,
        )?;
        let received_at_unix = now_unix();
        let observation_count = request.observations.len();
        self.staged_subscription_sessions = Some(StagedSubscriptionSessionObservations {
            adapter_instance_id: adapter_instance_id.to_string(),
            request,
            received_at_unix,
        });
        self.record_runtime_event(
            "subscription_sessions_staged",
            format!(
                "accepted {observation_count} session observations from configured local adapter"
            ),
        );
        Ok(self.subscription_session_adapter_view())
    }

    pub fn pending_subscription_session_enforcements(
        &self,
        adapter_instance_id: &str,
    ) -> Result<Vec<LocalSubscriptionSessionEnforcementCommand>> {
        self.require_active_subscription_session_adapter_instance(adapter_instance_id)?;
        Ok(self
            .pending_subscription_session_enforcements
            .iter()
            .filter(|pending| pending.adapter_instance_id == adapter_instance_id)
            .filter(|pending| pending.command.expires_at_unix >= now_unix())
            .map(|pending| pending.command.clone())
            .collect())
    }

    pub async fn complete_subscription_session_enforcement(
        &mut self,
        adapter_instance_id: &str,
        action_id: &str,
        result: CompleteLocalSubscriptionSessionEnforcementRequest,
    ) -> Result<()> {
        self.require_active_subscription_session_adapter_instance(adapter_instance_id)?;
        let pending = self
            .pending_subscription_session_enforcements
            .iter()
            .find(|pending| {
                pending.adapter_instance_id == adapter_instance_id
                    && pending.command.action_id == action_id
            })
            .cloned()
            .context("unknown local subscription session enforcement action")?;
        if pending.command.expires_at_unix < now_unix() {
            bail!("local subscription session enforcement action expired");
        }
        validate_local_subscription_session_enforcement_result(&pending.command, &result)?;
        let applied = result.status == SubscriptionSessionEnforcementStatus::Applied;
        self.client
            .post(format!(
                "{}/api/node-agent/subscription-sessions/enforcement-result",
                self.config.panel_url
            ))
            .json(&ReportSubscriptionSessionEnforcementResultRequest {
                action_id: pending.command.action_id.clone(),
                session_id: pending.command.session_id.clone(),
                status: result.status,
                runtime_session_ref: result.runtime_session_ref,
                adapter: applied
                    .then_some(SubscriptionSessionRuntimeAdapter::NodeManagedExactSession),
                session_absent_after_action: result.session_absent_after_action,
                verified_at_unix: result.verified_at_unix,
                detail: result.detail,
            })
            .send()
            .await
            .context("node-agent/subscription-sessions/enforcement-result request failed")?
            .error_for_status()
            .context("node-agent/subscription-sessions/enforcement-result returned error status")?;
        self.pending_subscription_session_enforcements
            .retain(|item| {
                item.adapter_instance_id != adapter_instance_id
                    || item.command.action_id != action_id
            });
        self.record_runtime_event(
            "subscription_session_enforcement_reported",
            format!(
                "reported {} result for local session enforcement action",
                if applied { "applied" } else { "failed" }
            ),
        );
        Ok(())
    }

    pub async fn update_xray_core(&mut self) -> Result<String> {
        self.refresh_runtime_process_state()?;
        self.record_xray_update_phase("preflight", "xray update preflight started")?;

        let binary_path = self
            .config
            .xray_binary_path
            .clone()
            .map(PathBuf::from)
            .context("xray update requires HYDRA_NODE_XRAY_BINARY_PATH")?;

        let release = fetch_latest_xray_release().await?;
        let asset_name = expected_xray_asset_name()?;
        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == asset_name)
            .with_context(|| format!("latest Xray release does not contain asset {asset_name}"))?;
        self.state.last_xray_update_source_release = Some(release.tag_name.clone());
        self.state.last_xray_update_target_version = Some(release.tag_name.clone());
        self.record_xray_update_phase(
            "release_selected",
            format!("selected {} from {}", asset.name, release.tag_name),
        )?;

        let was_running = self.current_xray_runtime_status() == XrayRuntimeStatus::Running;
        if was_running {
            self.record_xray_update_phase("stop_runtime", "stopping xray before core update")?;
            if let Err(error) = self.xray_manager.run_action(
                LocalRuntimeAction::Stop,
                Path::new(&self.config.local_xray_config_path),
                &mut self.state.xray_runtime,
                &mut self.xray_child,
            ) {
                self.record_xray_update_failure(format!(
                    "failed to stop xray before core update: {error}"
                ))?;
                return Err(error).context("failed to stop xray before core update");
            }
        }

        self.record_xray_update_phase("backup_binary", "backing up current xray binary")?;
        let binary_backup_path = backup_xray_binary_before_update(&binary_path)?;
        self.state.last_xray_update_backup_path = binary_backup_path
            .as_ref()
            .map(|path| path.to_string_lossy().to_string());

        self.record_xray_update_phase("download_binary", format!("downloading {}", asset.name))?;
        download_and_install_xray_binary(&asset.browser_download_url, &binary_path).await?;

        self.record_xray_update_phase("detect_version", "detecting updated xray version")?;
        let detected_version = match detect_xray_version_from_binary(&binary_path) {
            Ok(version) => version,
            Err(error) => {
                let restore_detail = restore_xray_binary_after_failed_update(
                    &binary_path,
                    binary_backup_path.as_deref(),
                )?;
                self.record_xray_update_failure(format!(
                    "updated xray binary failed version detection and previous binary was restored: {error}; {restore_detail}"
                ))?;
                return Err(error).context(
                    "updated xray binary failed version detection and previous binary was restored",
                );
            }
        };
        self.state.xray_detected_version = Some(detected_version.clone());
        self.state.last_xray_update_target_version = Some(detected_version.clone());
        self.state.last_xray_update_at_unix = Some(now_unix());
        if Path::new(&self.config.local_xray_config_path).is_file() {
            self.record_xray_update_phase(
                "validate_config",
                "validating current xray config with updated binary",
            )?;
            if let Err(error) = self
                .xray_manager
                .apply_config(Path::new(&self.config.local_xray_config_path))
            {
                let restore_detail = restore_xray_binary_after_failed_update(
                    &binary_path,
                    binary_backup_path.as_deref(),
                )?;
                self.record_xray_update_failure(format!(
                    "updated xray binary failed to validate current xray config and previous binary was restored: {error}; {restore_detail}"
                ))?;
                return Err(error)
                    .context("updated xray binary failed to validate current xray config and previous binary was restored");
            }
        }

        let mut detail = format!(
            "updated xray core to {} from {}",
            detected_version, release.tag_name
        );

        if was_running {
            self.record_xray_update_phase("restart_runtime", "restarting xray after core update")?;
            let restart_detail = match self.xray_manager.run_action(
                LocalRuntimeAction::Start,
                Path::new(&self.config.local_xray_config_path),
                &mut self.state.xray_runtime,
                &mut self.xray_child,
            ) {
                Ok(detail) => detail,
                Err(error) => {
                    self.record_xray_update_failure(format!(
                        "failed to restart xray after core update: {error}"
                    ))?;
                    return Err(error).context("failed to restart xray after core update");
                }
            };
            detail = format!("{detail}; runtime: {restart_detail}");
        }

        self.state.last_xray_update_status = Some(XrayUpdateStatus::Succeeded);
        self.state.last_xray_update_phase = Some("succeeded".to_string());
        self.state.last_xray_update_detail = Some(detail.clone());
        self.push_log("info", detail.clone());
        self.record_runtime_event("xray_core_updated", detail.clone());
        persist_state(&self.config.local_state_path, &self.state)?;
        persist_runtime_events(
            &self.config.runtime_event_history_path,
            &self.runtime_events,
            self.config.max_runtime_event_entries,
        )?;
        Ok(detail)
    }

    fn record_xray_update_phase(&mut self, phase: &str, detail: impl Into<String>) -> Result<()> {
        let detail = detail.into();
        self.state.last_xray_update_at_unix = Some(now_unix());
        self.state.last_xray_update_status = Some(XrayUpdateStatus::Running);
        self.state.last_xray_update_phase = Some(phase.to_string());
        self.state.last_xray_update_detail = Some(detail.clone());
        self.state.xray_runtime.last_action = Some("xray_update".to_string());
        self.state.xray_runtime.last_detail = Some(detail.clone());
        self.record_runtime_event("xray_core_update_phase", format!("{phase}: {detail}"));
        persist_state(&self.config.local_state_path, &self.state)?;
        persist_runtime_events(
            &self.config.runtime_event_history_path,
            &self.runtime_events,
            self.config.max_runtime_event_entries,
        )
    }

    fn record_xray_update_failure(&mut self, detail: String) -> Result<()> {
        self.state.last_xray_update_at_unix = Some(now_unix());
        self.state.last_xray_update_status = Some(XrayUpdateStatus::Failed);
        if self.state.last_xray_update_phase.is_none() {
            self.state.last_xray_update_phase = Some("failed".to_string());
        }
        self.state.last_xray_update_detail = Some(detail.clone());
        self.state.xray_runtime.status = Some(XrayRuntimeStatus::Failed);
        self.state.xray_runtime.last_action = Some("xray_update".to_string());
        self.state.xray_runtime.last_detail = Some(detail.clone());
        self.push_log("error", detail.clone());
        self.record_runtime_event("xray_core_update_failed", detail);
        persist_state(&self.config.local_state_path, &self.state)?;
        persist_runtime_events(
            &self.config.runtime_event_history_path,
            &self.runtime_events,
            self.config.max_runtime_event_entries,
        )
    }

    pub fn rollback_last_config(&mut self) -> Result<String> {
        self.refresh_runtime_process_state()?;

        let backup_path = self
            .state
            .last_config_backup_path
            .clone()
            .map(PathBuf::from)
            .context("no config backup is available for rollback")?;

        if !backup_path.is_file() {
            bail!(
                "configured backup file does not exist: {}",
                backup_path.display()
            );
        }

        let config_path = Path::new(&self.config.local_xray_config_path);
        fs::copy(&backup_path, config_path).with_context(|| {
            format!(
                "failed to restore backup from {} to {}",
                backup_path.display(),
                config_path.display()
            )
        })?;

        let action = match self.current_xray_runtime_status() {
            XrayRuntimeStatus::Running => LocalRuntimeAction::Restart,
            XrayRuntimeStatus::Unknown | XrayRuntimeStatus::Stopped | XrayRuntimeStatus::Failed => {
                LocalRuntimeAction::Start
            }
        };

        let runtime_detail = match self.xray_manager.run_action(
            action,
            config_path,
            &mut self.state.xray_runtime,
            &mut self.xray_child,
        ) {
            Ok(detail) => detail,
            Err(error) => {
                self.state.xray_runtime.status = Some(XrayRuntimeStatus::Failed);
                self.state.xray_runtime.last_action = Some("rollback".to_string());
                self.state.xray_runtime.last_detail = Some(error.to_string());
                self.state.last_apply_detail = Some(format!(
                    "rollback failed after restoring backup {}; runtime: {}",
                    backup_path.display(),
                    error
                ));
                self.record_runtime_event(
                    "config_rollback_failed",
                    format!(
                        "restored backup {} but runtime apply failed: {}",
                        backup_path.display(),
                        error
                    ),
                );
                self.push_log(
                    "error",
                    format!(
                        "config rollback failed after restoring {}: {}",
                        backup_path.display(),
                        error
                    ),
                );
                persist_state(&self.config.local_state_path, &self.state)?;
                persist_runtime_events(
                    &self.config.runtime_event_history_path,
                    &self.runtime_events,
                    self.config.max_runtime_event_entries,
                )?;
                return Err(error);
            }
        };

        clear_rollback_marker(config_path)?;
        self.state.rollback_marker_path = None;
        self.state.last_apply_detail = Some(format!(
            "rolled back config from backup {}; runtime: {runtime_detail}",
            backup_path.display()
        ));
        self.record_runtime_event(
            "config_rollback_succeeded",
            format!(
                "restored backup {} and applied runtime action",
                backup_path.display()
            ),
        );
        self.push_log(
            "warning",
            format!("config rollback succeeded from {}", backup_path.display()),
        );
        persist_state(&self.config.local_state_path, &self.state)?;
        persist_runtime_events(
            &self.config.runtime_event_history_path,
            &self.runtime_events,
            self.config.max_runtime_event_entries,
        )?;

        Ok(format!(
            "rolled back config from backup {}; runtime: {runtime_detail}",
            backup_path.display()
        ))
    }

    pub async fn tick(&mut self) -> Result<()> {
        self.refresh_runtime_process_state()?;
        self.refresh_sidecar_preflight_state();
        self.expire_subscription_session_adapter_lease().await?;
        self.expire_subscription_session_enforcements().await?;

        let result = self.tick_inner().await;
        match &result {
            Ok(()) => {
                self.state.last_successful_tick_at_unix = Some(now_unix());
                self.state.consecutive_tick_failures = 0;
                self.state.last_error = None;
            }
            Err(error) => {
                self.state.consecutive_tick_failures =
                    self.state.consecutive_tick_failures.saturating_add(1);
                self.state.last_error = Some(error.to_string());
                self.push_log("error", format!("tick failed: {error}"));
            }
        }
        persist_state(&self.config.local_state_path, &self.state)?;
        persist_apply_history(
            &self.config.apply_history_path,
            &self.apply_history,
            self.config.max_apply_history_entries,
        )?;
        persist_runtime_events(
            &self.config.runtime_event_history_path,
            &self.runtime_events,
            self.config.max_runtime_event_entries,
        )?;
        result
    }

    async fn tick_inner(&mut self) -> Result<()> {
        let identity = self.me().await?;
        self.state.node_id = Some(identity.node_id.clone());

        let config = self.fetch_config().await?;
        let cluster_targets = self.fetch_cluster_targets(&config).await?;
        let cluster_targets_changed = self.store_cluster_targets(cluster_targets)?;
        let route_assignments_changed = self
            .store_node_route_assignments(config.generated_config.node_route_assignments.clone())?;
        let route_credentials_changed = self.sync_route_credentials(&config).await?;
        self.send_heartbeat().await?;
        let runtime_inputs_changed =
            cluster_targets_changed || route_assignments_changed || route_credentials_changed;

        let (mut sync_status, mut detail) = if self.state.applied_revision.as_deref()
            == Some(config.revision.as_str())
            && !runtime_inputs_changed
        {
            (
                NodeSyncStatus::Synced,
                "local revision already matches panel revision".to_string(),
            )
        } else {
            match self.apply_config(&config).await {
                Ok(detail) => (NodeSyncStatus::Synced, detail),
                Err(error) => {
                    self.record_apply_event(
                        Some(config.revision.clone()),
                        NodeSyncStatus::Drifted,
                        format!("apply failed: {error}"),
                    );
                    self.state.last_apply_detail = Some(format!("apply failed: {error}"));
                    self.push_log("error", format!("apply failed: {error}"));
                    (NodeSyncStatus::Drifted, format!("apply failed: {error}"))
                }
            }
        };
        if let Some(blocking_detail) = self
            .required_protocol_blocking_detail()
            .or_else(|| self.xray_render_blocking_detail())
        {
            sync_status = NodeSyncStatus::Drifted;
            detail = format!("{detail}; {blocking_detail}");
            self.state.last_apply_detail = Some(detail.clone());
        }

        self.report_sync(
            sync_status,
            self.state.applied_revision.clone(),
            Some(detail),
        )
        .await?;
        self.report_metrics().await?;
        self.collect_runtime_activity().await;
        self.report_subscription_sessions().await?;
        self.flush_logs().await?;
        persist_state(&self.config.local_state_path, &self.state)?;

        info!(
            node_id = %identity.node_id,
            revision = %config.revision,
            "node tick completed"
        );

        Ok(())
    }

    async fn me(&self) -> Result<NodeAgentIdentity> {
        self.client
            .get(format!("{}/api/node-agent/me", self.config.panel_url))
            .send()
            .await
            .context("node-agent/me request failed")?
            .error_for_status()
            .context("node-agent/me returned error status")?
            .json::<NodeAgentIdentity>()
            .await
            .context("failed to decode node-agent/me response")
    }

    async fn fetch_config(&self) -> Result<NodeAgentConfigResponse> {
        self.client
            .get(format!("{}/api/node-agent/config", self.config.panel_url))
            .send()
            .await
            .context("node-agent/config request failed")?
            .error_for_status()
            .context("node-agent/config returned error status")?
            .json::<NodeAgentConfigResponse>()
            .await
            .context("failed to decode node-agent/config response")
    }

    async fn fetch_route_credentials(&self) -> Result<NodeRouteCredentialBundle> {
        self.client
            .get(format!(
                "{}/api/node-agent/route-credentials",
                self.config.panel_url
            ))
            .send()
            .await
            .context("node-agent/route-credentials request failed")?
            .error_for_status()
            .context("node-agent/route-credentials returned error status")?
            .json::<NodeRouteCredentialBundle>()
            .await
            .context("failed to decode node-agent/route-credentials response")
    }

    async fn sync_route_credentials(&mut self, config: &NodeAgentConfigResponse) -> Result<bool> {
        if config.generated_config.node_route_assignments.is_empty() {
            return Ok(false);
        }
        let bundle = self.fetch_route_credentials().await?;
        let count = install_route_credentials(
            &self.config.route_credentials_dir,
            &self.config.route_credentials_path,
            &bundle,
        )?;
        if count > 0 {
            self.state.last_route_credentials_saved_at_unix = Some(now_unix());
            self.record_runtime_event(
                "route_credentials_synced",
                format!("installed or updated {count} node-local route credential file set(s)"),
            );
        }
        Ok(count > 0)
    }

    async fn fetch_cluster_targets(
        &self,
        config: &NodeAgentConfigResponse,
    ) -> Result<Vec<GeneratedClusterNodeTarget>> {
        let response = match self
            .client
            .get(format!(
                "{}/api/node-agent/cluster-targets",
                self.config.panel_url
            ))
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) => {
                warn!(
                    error = %error,
                    "failed to fetch node cluster targets; falling back to generated config"
                );
                return Ok(config.generated_config.cluster_node_targets.clone());
            }
        };

        let response = match response.error_for_status() {
            Ok(response) => response,
            Err(error) => {
                warn!(
                    error = %error,
                    "node cluster targets endpoint failed; falling back to generated config"
                );
                return Ok(config.generated_config.cluster_node_targets.clone());
            }
        };

        match response.json::<Vec<GeneratedClusterNodeTarget>>().await {
            Ok(targets) => Ok(targets),
            Err(error) => {
                warn!(
                    error = %error,
                    "failed to decode node cluster targets; falling back to generated config"
                );
                Ok(config.generated_config.cluster_node_targets.clone())
            }
        }
    }

    async fn send_heartbeat(&self) -> Result<()> {
        let body = NodeHeartbeatRequest {
            xray_version: self
                .state
                .xray_detected_version
                .clone()
                .or_else(|| self.config.xray_version.clone()),
            node_version: Some(self.config.node_version.clone()),
            status: self.derive_node_status(),
        };

        self.client
            .post(format!(
                "{}/api/node-agent/heartbeat",
                self.config.panel_url
            ))
            .json(&body)
            .send()
            .await
            .context("node-agent/heartbeat request failed")?
            .error_for_status()
            .context("node-agent/heartbeat returned error status")?;

        Ok(())
    }

    fn store_cluster_targets(&mut self, targets: Vec<GeneratedClusterNodeTarget>) -> Result<bool> {
        let target_count = targets.len();
        let intents = build_cluster_runtime_intents(&targets);
        let intent_count = intents.len();
        let changed =
            self.state.cluster_targets != targets || self.state.cluster_runtime_intents != intents;
        self.state.cluster_targets = targets;
        self.state.cluster_runtime_intents = intents;
        self.state.last_cluster_targets_saved_at_unix = Some(now_unix());
        if changed {
            self.record_runtime_event(
                "cluster_targets_updated",
                format!(
                    "stored {target_count} cluster target(s) and {intent_count} runtime intent(s) for this node"
                ),
            );
        }
        persist_state(&self.config.local_state_path, &self.state)?;
        Ok(changed)
    }

    fn store_node_route_assignments(
        &mut self,
        assignments: Vec<NodeRouteAssignment>,
    ) -> Result<bool> {
        let assignments = match self.state.node_id.as_deref() {
            Some(node_id) => assignments
                .into_iter()
                .filter(|assignment| assignment.node_id == node_id)
                .collect::<Vec<_>>(),
            None => assignments,
        };
        let assignment_count = assignments.len();
        let changed = self.state.node_route_assignments != assignments;
        self.state.node_route_assignments = assignments;
        if changed {
            self.record_runtime_event(
                "node_route_assignments_updated",
                format!("stored {assignment_count} least-knowledge route assignment(s)"),
            );
        }
        persist_state(&self.config.local_state_path, &self.state)?;
        Ok(changed)
    }

    async fn apply_config(&mut self, response: &NodeAgentConfigResponse) -> Result<String> {
        let backup_path = backup_config_if_exists(Path::new(&self.config.local_xray_config_path))?;
        if let Some(path) = backup_path.as_ref() {
            self.state.last_config_backup_path = Some(path.display().to_string());
        }
        persist_generated_config(&self.config.local_config_path, &response.generated_config)?;
        let runtime_config = build_node_runtime_config_document(
            response,
            &self.state.node_id,
            &self.state.cluster_runtime_intents,
            &self.state.node_route_assignments,
        );
        self.state.last_runtime_protocol_requirements = runtime_config.required_protocols.clone();
        persist_node_runtime_config(&self.config.local_runtime_config_path, &runtime_config)?;
        let sidecar_runtime_config = build_sidecar_runtime_config_document_with_stats(
            &runtime_config,
            self.config.hysteria2_traffic_stats_base_port,
            self.config.node_token.as_bytes(),
        );
        persist_sidecar_runtime_config(
            &self.config.local_sidecar_runtime_config_path,
            &sidecar_runtime_config,
        )?;
        let sidecar_config_file_count = persist_sidecar_generated_config_files(
            &self.config.local_sidecar_runtime_config_path,
            &sidecar_runtime_config,
        )?;
        let wireguard_session_mapping = build_wireguard_session_mapping(&sidecar_runtime_config)?;
        persist_wireguard_session_mapping(
            &self.config.local_sidecar_runtime_config_path,
            &wireguard_session_mapping,
        )?;
        let sidecar_runtime_summary = summarize_sidecar_runtime_config(&sidecar_runtime_config);
        self.state.last_sidecar_runtime_summary = Some(sidecar_runtime_summary.clone());
        let route_credentials =
            load_route_credentials(Path::new(&self.config.route_credentials_path))?;
        let xray_render_plan = render_xray_config_with_stats(
            &runtime_config,
            &route_credentials,
            self.state.xray_detected_version.clone(),
            Some(&self.config.xray_stats_api_address),
        );
        persist_xray_render_plan(&self.config.local_xray_config_path, &xray_render_plan)?;
        let xray_render_summary = summarize_xray_render_plan(&xray_render_plan);
        self.state.last_xray_render_summary = Some(xray_render_summary.clone());
        let apply_detail = match self
            .xray_manager
            .apply_config(Path::new(&self.config.local_xray_config_path))
        {
            Ok(detail) => detail,
            Err(error) => {
                let marker_path = write_rollback_marker(
                    Path::new(&self.config.local_xray_config_path),
                    response.revision.as_str(),
                    &error.to_string(),
                    backup_path.as_ref(),
                )?;
                self.state.rollback_marker_path = Some(marker_path.display().to_string());
                self.state.xray_runtime.status = Some(XrayRuntimeStatus::Failed);
                self.state.xray_runtime.last_action = Some("validate".to_string());
                self.state.xray_runtime.last_detail = Some(error.to_string());
                self.state.last_config_saved_at_unix = Some(now_unix());
                self.state.last_runtime_config_saved_at_unix = Some(runtime_config.created_at_unix);
                self.state.last_sidecar_runtime_config_saved_at_unix =
                    Some(sidecar_runtime_config.created_at_unix);
                self.state.last_xray_config_saved_at_unix = Some(xray_render_plan.created_at_unix);
                self.state.last_apply_detail = Some(format!(
                    "revision {} rendered but validation failed: {}; render: {}",
                    response.revision,
                    error,
                    format_xray_render_summary(&xray_render_summary),
                ));
                self.record_runtime_event(
                    "config_apply_validation_failed",
                    format!(
                        "revision {} validation failed: {}; rollback marker: {}",
                        response.revision,
                        error,
                        marker_path.display()
                    ),
                );
                return Err(error);
            }
        };

        self.state.last_config_saved_at_unix = Some(now_unix());
        self.state.last_runtime_config_saved_at_unix = Some(runtime_config.created_at_unix);
        self.state.last_sidecar_runtime_config_saved_at_unix =
            Some(sidecar_runtime_config.created_at_unix);
        self.state.last_xray_config_saved_at_unix = Some(xray_render_plan.created_at_unix);

        let runtime_action = match self.current_xray_runtime_status() {
            XrayRuntimeStatus::Running => LocalRuntimeAction::Restart,
            XrayRuntimeStatus::Unknown | XrayRuntimeStatus::Stopped | XrayRuntimeStatus::Failed => {
                LocalRuntimeAction::Start
            }
        };
        let runtime_detail = match self.xray_manager.run_action(
            runtime_action,
            Path::new(&self.config.local_xray_config_path),
            &mut self.state.xray_runtime,
            &mut self.xray_child,
        ) {
            Ok(detail) => {
                clear_rollback_marker(Path::new(&self.config.local_xray_config_path))?;
                self.state.rollback_marker_path = None;
                detail
            }
            Err(error) => {
                let marker_path = write_rollback_marker(
                    Path::new(&self.config.local_xray_config_path),
                    response.revision.as_str(),
                    &error.to_string(),
                    backup_path.as_ref(),
                )?;
                self.state.rollback_marker_path = Some(marker_path.display().to_string());
                self.state.xray_runtime.status = Some(XrayRuntimeStatus::Failed);
                self.state.xray_runtime.last_action = Some("apply_runtime".to_string());
                self.state.xray_runtime.last_detail = Some(error.to_string());
                self.state.last_apply_detail = Some(format!(
                    "revision {} rendered and validated but runtime apply failed: {}; render: {}",
                    response.revision,
                    error,
                    format_xray_render_summary(&xray_render_summary),
                ));
                self.record_runtime_event(
                    "config_apply_runtime_failed",
                    format!(
                        "revision {} runtime apply failed: {}; rollback marker: {}",
                        response.revision,
                        error,
                        marker_path.display()
                    ),
                );
                return Err(error);
            }
        };
        self.state.applied_revision = Some(response.revision.clone());
        self.state.last_apply_detail = Some(format!(
            "revision {} applied: {apply_detail}; runtime: {runtime_detail}; render: {}",
            response.revision,
            format_xray_render_summary(&xray_render_summary),
        ));
        if sidecar_config_file_count > 0 {
            self.record_runtime_event(
                "sidecar_generated_configs_written",
                format!("wrote {sidecar_config_file_count} sidecar generated config file(s)"),
            );
        }
        self.record_apply_event(
            Some(response.revision.clone()),
            NodeSyncStatus::Synced,
            format!(
                "{apply_detail}; {runtime_detail}; render: {}",
                format_xray_render_summary(&xray_render_summary)
            ),
        );
        self.push_log(
            "info",
            format!(
                "applied revision {} ({apply_detail}; runtime: {runtime_detail}; render: {})",
                response.revision,
                format_xray_render_summary(&xray_render_summary),
            ),
        );

        Ok(format!(
            "revision {} applied: {apply_detail}; runtime: {runtime_detail}; render: {}",
            response.revision,
            format_xray_render_summary(&xray_render_summary),
        ))
    }

    async fn report_sync(
        &mut self,
        sync_status: NodeSyncStatus,
        applied_revision: Option<String>,
        detail: Option<String>,
    ) -> Result<()> {
        let body = NodeSyncRequest {
            sync_status,
            applied_revision,
            detail,
            runtime_components: self.reported_runtime_components(),
            external_xray_validation: self.external_xray_validation_report(),
            runtime_alerts: self
                .runtime_alerts()
                .into_iter()
                .map(runtime_alert_to_domain)
                .collect(),
        };

        self.client
            .post(format!("{}/api/node-agent/sync", self.config.panel_url))
            .json(&body)
            .send()
            .await
            .context("node-agent/sync request failed")?
            .error_for_status()
            .context("node-agent/sync returned error status")?;

        self.state.last_sync_reported_at_unix = Some(now_unix());
        Ok(())
    }

    async fn report_metrics(&mut self) -> Result<()> {
        let metrics = collect_metrics();

        self.client
            .post(format!("{}/api/node-agent/metrics", self.config.panel_url))
            .json(&metrics)
            .send()
            .await
            .context("node-agent/metrics request failed")?
            .error_for_status()
            .context("node-agent/metrics returned error status")?;

        self.state.last_metrics_reported_at_unix = Some(now_unix());
        Ok(())
    }

    async fn collect_runtime_activity(&mut self) {
        let runtime_config = match self.load_last_runtime_config() {
            Ok(config) => config,
            Err(error) => {
                self.update_runtime_activity_error(Some(format!(
                    "runtime activity config unavailable: {error}"
                )));
                return;
            }
        };
        let allowed_principals = runtime_config
            .users
            .iter()
            .map(|user| user.username.clone())
            .collect::<BTreeSet<_>>();
        let mut errors = Vec::new();

        let xray_stats_required = runtime_config
            .required_protocols
            .iter()
            .any(|requirement| requirement.required_component == RuntimeComponentKind::Xray);
        if xray_stats_required {
            match self.collect_xray_principal_counters().await {
                Ok(counters) => {
                    let now = now_unix();
                    self.runtime_activity
                        .xray_counters
                        .retain(|principal, _| allowed_principals.contains(principal));
                    self.runtime_activity
                        .xray_last_activity
                        .retain(|principal, _| allowed_principals.contains(principal));
                    let mut counters = counters
                        .into_iter()
                        .filter(|(principal, _)| allowed_principals.contains(principal))
                        .collect::<Vec<_>>();
                    counters.sort_by(|left, right| left.0.cmp(&right.0));
                    counters.truncate(self.config.max_subscription_session_observations);
                    for (principal, total) in counters {
                        let previous = self
                            .runtime_activity
                            .xray_counters
                            .insert(principal.clone(), total);
                        if total > previous.unwrap_or_default() {
                            self.runtime_activity
                                .xray_last_activity
                                .insert(principal, now);
                        }
                    }
                }
                Err(error) => errors.push(format!("Xray Stats API: {error}")),
            }
        } else {
            self.runtime_activity.xray_counters.clear();
            self.runtime_activity.xray_last_activity.clear();
        }

        let hysteria_configs = build_sidecar_runtime_config_document_with_stats(
            &runtime_config,
            self.config.hysteria2_traffic_stats_base_port,
            self.config.node_token.as_bytes(),
        )
        .hysteria2_configs;
        self.runtime_activity.enabled = xray_stats_required || !hysteria_configs.is_empty();
        let mut hysteria2_online: BTreeMap<String, u64> = BTreeMap::new();
        for config in hysteria_configs {
            match self.collect_hysteria2_online(&config).await {
                Ok(online) => {
                    for (principal, connection_count) in online {
                        if connection_count > 0 && allowed_principals.contains(&principal) {
                            if hysteria2_online.len()
                                >= self.config.max_subscription_session_observations
                                && !hysteria2_online.contains_key(&principal)
                            {
                                continue;
                            }
                            hysteria2_online
                                .entry(principal)
                                .and_modify(|count| {
                                    *count = (*count).max(connection_count);
                                })
                                .or_insert(connection_count);
                        }
                    }
                }
                Err(error) => errors.push(format!(
                    "Hysteria2 Traffic Stats API for {}: {error}",
                    config.tag
                )),
            }
        }
        self.runtime_activity.hysteria2_online = hysteria2_online;
        self.state.last_runtime_activity_collected_at_unix = Some(now_unix());
        self.update_runtime_activity_error(
            (!errors.is_empty()).then(|| errors.join("; ").chars().take(512).collect()),
        );
    }

    async fn collect_xray_principal_counters(&self) -> Result<HashMap<String, u64>> {
        let binary = self
            .config
            .xray_binary_path
            .as_deref()
            .context("Xray binary path is not configured")?;
        let output = tokio::time::timeout(
            Duration::from_secs(self.config.runtime_stats_timeout_seconds),
            Command::new(binary)
                .args([
                    "api",
                    "statsquery",
                    &format!("--server={}", self.config.xray_stats_api_address),
                    "--pattern=user>>>",
                    "--reset=false",
                ])
                .stdin(Stdio::null())
                .output(),
        )
        .await
        .context("Xray stats query timed out")?
        .context("failed to execute Xray stats query")?;
        if !output.status.success() {
            bail!("Xray stats query returned a non-zero status");
        }
        if output.stdout.len() > MAX_RUNTIME_STATS_RESPONSE_BYTES {
            bail!("Xray stats response exceeds the configured safety limit");
        }
        parse_xray_principal_counters(&output.stdout)
    }

    async fn collect_hysteria2_online(
        &self,
        config: &Hysteria2RuntimeConfig,
    ) -> Result<BTreeMap<String, u64>> {
        let address = loopback_socket_address(&config.traffic_stats_listen)
            .context("traffic stats listener is not a loopback socket address")?;
        let response = self
            .runtime_stats_client
            .get(format!("http://{address}/online"))
            .header("Authorization", &config.traffic_stats_secret)
            .send()
            .await
            .context("traffic stats request failed")?
            .error_for_status()
            .context("traffic stats endpoint returned an error status")?;
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RUNTIME_STATS_RESPONSE_BYTES as u64)
        {
            bail!("traffic stats response exceeds the configured safety limit");
        }
        let body = response
            .bytes()
            .await
            .context("failed to read traffic stats response")?;
        if body.len() > MAX_RUNTIME_STATS_RESPONSE_BYTES {
            bail!("traffic stats response exceeds the configured safety limit");
        }
        serde_json::from_slice(&body).context("traffic stats response is not a client-count map")
    }

    fn runtime_activity_snapshot(&self) -> Option<ReportSubscriptionSessionsRequest> {
        if !self.runtime_activity.enabled {
            return None;
        }
        self.state.last_runtime_activity_collected_at_unix?;
        let now = now_unix();
        let mut observations = self
            .runtime_activity
            .xray_last_activity
            .iter()
            .filter(|(_, observed_at)| {
                now.saturating_sub(**observed_at) <= self.config.runtime_activity_window_seconds
            })
            .map(|(principal, observed_at)| SubscriptionSessionObservation {
                session_id: runtime_activity_session_id("xray-traffic", principal),
                runtime_username: principal.clone(),
                runtime_session_ref: None,
                device_fingerprint: None,
                source_ip: None,
                connected_at_unix: Some(*observed_at),
            })
            .collect::<Vec<_>>();
        observations.extend(
            self.runtime_activity
                .hysteria2_online
                .keys()
                .map(|principal| SubscriptionSessionObservation {
                    session_id: runtime_activity_session_id("hysteria2-online", principal),
                    runtime_username: principal.clone(),
                    runtime_session_ref: None,
                    device_fingerprint: None,
                    source_ip: None,
                    connected_at_unix: Some(now),
                }),
        );
        observations.sort_by(|left, right| left.session_id.cmp(&right.session_id));
        observations.truncate(self.config.max_subscription_session_observations);
        Some(ReportSubscriptionSessionsRequest {
            observation_source: SubscriptionSessionObservationSource::NodeManagedRuntimeTable,
            runtime_capabilities: Vec::new(),
            observations,
        })
    }

    fn update_runtime_activity_error(&mut self, error: Option<String>) {
        if self.state.last_runtime_activity_error == error {
            return;
        }
        let recovered = self.state.last_runtime_activity_error.is_some() && error.is_none();
        self.state.last_runtime_activity_error = error.clone();
        if let Some(error) = error {
            self.record_runtime_event("runtime_activity_collection_failed", error);
        } else if recovered {
            self.record_runtime_event(
                "runtime_activity_collection_recovered",
                "Xray/Hysteria2 runtime activity collection recovered".to_string(),
            );
        }
    }

    async fn report_subscription_sessions(&mut self) -> Result<()> {
        let mut reported_count = 0usize;
        let mut blocked_count = 0usize;
        if let Some(request) = self.runtime_activity_snapshot() {
            let response = self
                .report_subscription_session_snapshot(request, None)
                .await?;
            reported_count = reported_count.saturating_add(response.reported_count);
            blocked_count = blocked_count.saturating_add(response.blocked_count);
        }

        let staged = self
            .staged_subscription_sessions
            .as_ref()
            .and_then(|staged| {
                (now_unix().saturating_sub(staged.received_at_unix)
                    <= self
                        .config
                        .subscription_session_observation_stale_after_seconds)
                    .then(|| staged.clone())
            });
        if self.staged_subscription_sessions.is_some() && staged.is_none() {
            self.staged_subscription_sessions = None;
            self.record_runtime_event(
                "subscription_sessions_expired",
                "discarded stale local adapter session observations".to_string(),
            );
        }
        if let Some(staged) = staged {
            let response = self
                .report_subscription_session_snapshot(
                    staged.request,
                    Some(staged.adapter_instance_id),
                )
                .await?;
            reported_count = reported_count.saturating_add(response.reported_count);
            blocked_count = blocked_count.saturating_add(response.blocked_count);
        }

        self.state.last_subscription_sessions_report_at_unix = Some(now_unix());
        self.state.last_subscription_sessions_reported_count = reported_count;
        self.state.last_subscription_sessions_blocked_count = blocked_count;
        Ok(())
    }

    async fn report_subscription_session_snapshot(
        &mut self,
        request: ReportSubscriptionSessionsRequest,
        adapter_instance_id: Option<String>,
    ) -> Result<ReportSubscriptionSessionsResponse> {
        let response = self
            .client
            .post(format!(
                "{}/api/node-agent/subscription-sessions/report",
                self.config.panel_url
            ))
            .json(&request)
            .send()
            .await
            .context("node-agent/subscription-sessions/report request failed")?
            .error_for_status()
            .context("node-agent/subscription-sessions/report returned error status")?
            .json::<ReportSubscriptionSessionsResponse>()
            .await
            .context("failed to decode node-agent/subscription-sessions/report response")?;

        for verdict in &response.verdicts {
            if let Some(enforcement) = verdict.enforcement.as_ref() {
                if let Some(runtime_session_ref) = request
                    .observations
                    .iter()
                    .find(|observation| observation.session_id == enforcement.session_id)
                    .and_then(|observation| observation.runtime_session_ref.clone())
                    && exact_subscription_session_capabilities(&request.runtime_capabilities)
                    && let Some(adapter_instance_id) = adapter_instance_id.clone()
                {
                    self.queue_subscription_session_enforcement(
                        adapter_instance_id,
                        enforcement.action_id.clone(),
                        enforcement.session_id.clone(),
                        enforcement.action,
                        runtime_session_ref,
                        enforcement.reason.clone(),
                        enforcement.requires_absence_verification,
                        enforcement.issued_at_unix,
                    )
                    .await?;
                } else {
                    self.report_unsupported_subscription_session_enforcement(
                        enforcement.action_id.clone(),
                        enforcement.session_id.clone(),
                    )
                    .await?;
                }
            }
        }
        Ok(response)
    }

    async fn queue_subscription_session_enforcement(
        &mut self,
        adapter_instance_id: String,
        action_id: String,
        session_id: String,
        action: node_domain::SubscriptionSessionEnforcementAction,
        runtime_session_ref: String,
        reason: String,
        requires_absence_verification: bool,
        issued_at_unix: u64,
    ) -> Result<()> {
        if self
            .pending_subscription_session_enforcements
            .iter()
            .any(|pending| {
                pending.adapter_instance_id == adapter_instance_id
                    && pending.command.action_id == action_id
            })
        {
            return Ok(());
        }
        if self.pending_subscription_session_enforcements.len()
            >= self.config.max_pending_subscription_session_enforcements
        {
            self.report_unsupported_subscription_session_enforcement(action_id, session_id)
                .await?;
            return Ok(());
        }
        self.pending_subscription_session_enforcements.push(
            PendingSubscriptionSessionEnforcement {
                adapter_instance_id,
                command: LocalSubscriptionSessionEnforcementCommand {
                    action_id,
                    session_id,
                    action,
                    runtime_session_ref,
                    reason,
                    requires_absence_verification,
                    issued_at_unix,
                    expires_at_unix: now_unix()
                        .saturating_add(self.config.subscription_session_action_timeout_seconds),
                },
            },
        );
        self.record_runtime_event(
            "subscription_session_enforcement_queued",
            "queued exact session enforcement for trusted local adapter".to_string(),
        );
        Ok(())
    }

    async fn expire_subscription_session_adapter_lease(&mut self) -> Result<()> {
        let expired = self
            .active_subscription_session_adapter
            .as_ref()
            .is_some_and(|lease| lease.lease_expires_at_unix < now_unix());
        if !expired {
            return Ok(());
        }
        self.fail_all_pending_subscription_session_enforcements(
            "local session adapter lease expired before enforcement completion",
        )
        .await?;
        self.active_subscription_session_adapter = None;
        self.staged_subscription_sessions = None;
        self.record_runtime_event(
            "subscription_session_adapter_expired",
            "expired trusted local session adapter lease and cleared staged observations"
                .to_string(),
        );
        Ok(())
    }

    async fn fail_all_pending_subscription_session_enforcements(
        &mut self,
        detail: &str,
    ) -> Result<()> {
        let pending = self.pending_subscription_session_enforcements.clone();
        for action in pending {
            self.report_failed_subscription_session_enforcement(
                action.command.action_id.clone(),
                action.command.session_id.clone(),
                detail.to_string(),
            )
            .await?;
            self.pending_subscription_session_enforcements
                .retain(|stored| stored.command.action_id != action.command.action_id);
        }
        Ok(())
    }

    async fn expire_subscription_session_enforcements(&mut self) -> Result<()> {
        let expired = self
            .pending_subscription_session_enforcements
            .iter()
            .filter(|pending| pending.command.expires_at_unix < now_unix())
            .cloned()
            .collect::<Vec<_>>();
        for action in expired {
            self.report_failed_subscription_session_enforcement(
                action.command.action_id.clone(),
                action.command.session_id.clone(),
                "local session enforcement action deadline expired before adapter completion"
                    .to_string(),
            )
            .await?;
            self.pending_subscription_session_enforcements
                .retain(|stored| stored.command.action_id != action.command.action_id);
        }
        Ok(())
    }

    async fn report_unsupported_subscription_session_enforcement(
        &mut self,
        action_id: String,
        session_id: String,
    ) -> Result<()> {
        let detail =
            "node local adapter is observation-only; exact session termination is unsupported"
                .to_string();
        self.report_failed_subscription_session_enforcement(action_id, session_id, detail)
            .await
    }

    async fn report_failed_subscription_session_enforcement(
        &mut self,
        action_id: String,
        session_id: String,
        detail: String,
    ) -> Result<()> {
        self.client
            .post(format!(
                "{}/api/node-agent/subscription-sessions/enforcement-result",
                self.config.panel_url
            ))
            .json(&ReportSubscriptionSessionEnforcementResultRequest {
                action_id,
                session_id,
                status: SubscriptionSessionEnforcementStatus::Failed,
                runtime_session_ref: None,
                adapter: None,
                session_absent_after_action: None,
                verified_at_unix: Some(now_unix()),
                detail: Some(detail.clone()),
            })
            .send()
            .await
            .context("node-agent/subscription-sessions/enforcement-result request failed")?
            .error_for_status()
            .context("node-agent/subscription-sessions/enforcement-result returned error status")?;
        self.push_log("warning", detail);
        Ok(())
    }

    async fn flush_logs(&mut self) -> Result<()> {
        if self.buffered_logs.is_empty() {
            return Ok(());
        }

        let upload_count = self
            .buffered_logs
            .len()
            .min(self.config.max_log_lines_per_upload);
        let lines = self
            .buffered_logs
            .drain(0..upload_count)
            .collect::<Vec<_>>();
        let body = NodeLogUploadRequest { lines };

        self.client
            .post(format!("{}/api/node-agent/logs", self.config.panel_url))
            .json(&body)
            .send()
            .await
            .context("node-agent/logs request failed")?
            .error_for_status()
            .context("node-agent/logs returned error status")?;

        Ok(())
    }

    pub fn push_log(&mut self, level: &str, message: String) {
        self.buffered_logs.push(NodeLogUploadLine {
            level: level.to_string(),
            message,
            created_at_unix: Some(now_unix()),
        });
        if self.buffered_logs.len() > self.config.max_buffered_log_lines {
            let drain = self.buffered_logs.len() - self.config.max_buffered_log_lines;
            self.buffered_logs.drain(0..drain);
            warn!("node log buffer was truncated");
        }
    }

    fn derive_node_status(&self) -> NodeStatus {
        if self.state.consecutive_tick_failures > 0
            || self.state.last_error.is_some()
            || self.current_xray_runtime_status() == XrayRuntimeStatus::Failed
            || self.required_protocol_blocking_detail().is_some()
            || self.xray_render_blocking_detail().is_some()
        {
            NodeStatus::Degraded
        } else {
            NodeStatus::Healthy
        }
    }

    fn current_xray_runtime_status(&self) -> XrayRuntimeStatus {
        self.state
            .xray_runtime
            .status
            .unwrap_or(XrayRuntimeStatus::Unknown)
    }

    fn reported_runtime_components(&self) -> Vec<NodeReportedRuntimeComponentView> {
        let mut required_components = BTreeSet::from([RuntimeComponentKind::Xray]);
        required_components.extend(
            self.state
                .last_runtime_protocol_requirements
                .iter()
                .map(|requirement| requirement.required_component),
        );

        required_components
            .into_iter()
            .map(|component| self.reported_runtime_component(component))
            .collect()
    }

    fn reported_runtime_component(
        &self,
        component: RuntimeComponentKind,
    ) -> NodeReportedRuntimeComponentView {
        let report = match component {
            RuntimeComponentKind::Xray => self.xray_component_report(),
            RuntimeComponentKind::Hysteria2 => {
                self.sidecar_component_report(LocalSidecarKind::Hysteria2)
            }
            RuntimeComponentKind::WireGuard => {
                self.sidecar_component_report(LocalSidecarKind::WireGuard)
            }
        };
        NodeReportedRuntimeComponentView {
            owner: panel_runtime_owner_for_component(component),
            component: panel_runtime_component_name(component).to_string(),
            installed: matches!(
                report.readiness,
                RuntimeComponentReadiness::Ready
                    | RuntimeComponentReadiness::Unknown
                    | RuntimeComponentReadiness::Failed
            ),
            healthy: report.readiness == RuntimeComponentReadiness::Ready,
            version: report.detected_version,
            last_validated_at_unix: report.last_validated_at_unix,
            last_error: report.last_error,
            checked_at_unix: Some(now_unix()),
        }
    }

    fn external_xray_validation_report(&self) -> Option<XrayExternalValidationReport> {
        let binary_path = self
            .xray_manager
            .binary_path
            .as_ref()
            .map(|path| path.display().to_string());
        if !self.xray_manager.requires_binary() && binary_path.is_none() {
            return None;
        }

        let status = if self.current_xray_runtime_status() == XrayRuntimeStatus::Failed {
            XrayExternalValidationStatus::Failed
        } else if self.state.xray_runtime.last_validated_at_unix.is_some() {
            XrayExternalValidationStatus::Passed
        } else {
            XrayExternalValidationStatus::Skipped
        };

        let detail = match status {
            XrayExternalValidationStatus::Passed => self
                .state
                .xray_runtime
                .last_detail
                .clone()
                .unwrap_or_else(|| "external Xray validation passed".to_string()),
            XrayExternalValidationStatus::Failed => self
                .state
                .xray_runtime
                .last_detail
                .clone()
                .unwrap_or_else(|| "external Xray validation failed".to_string()),
            XrayExternalValidationStatus::Skipped => {
                "external Xray validation has not run on this node yet".to_string()
            }
        };

        Some(XrayExternalValidationReport {
            status,
            checked_at_unix: self
                .state
                .xray_runtime
                .last_validated_at_unix
                .unwrap_or_else(now_unix),
            binary_path,
            internal_validation_valid: self
                .state
                .last_xray_render_summary
                .as_ref()
                .is_some_and(|summary| !summary.fail_closed),
            exit_code: match status {
                XrayExternalValidationStatus::Passed => Some(0),
                XrayExternalValidationStatus::Failed => self.state.xray_runtime.last_exit_code,
                XrayExternalValidationStatus::Skipped => None,
            },
            stdout: String::new(),
            stderr: if status == XrayExternalValidationStatus::Failed {
                detail.clone()
            } else {
                String::new()
            },
            detail,
            config_retained: Path::new(&self.config.local_xray_config_path).is_file(),
        })
    }

    fn runtime_validation_report(&self) -> RuntimeValidationReport {
        let components = vec![
            self.xray_component_report(),
            self.sidecar_component_report(LocalSidecarKind::Hysteria2),
            self.sidecar_component_report(LocalSidecarKind::WireGuard),
        ];
        let protocols = runtime_protocol_reports(&components);
        let sidecar_runtime = self.sidecar_runtime_validation_report();
        let required_protocols = runtime_protocol_requirement_statuses(
            &self.state.last_runtime_protocol_requirements,
            &protocols,
            Some(&sidecar_runtime),
        );
        let render_blocking_detail = self.xray_render_blocking_detail();
        let disabled_reasons = components
            .iter()
            .filter(|component| component.readiness == RuntimeComponentReadiness::Disabled)
            .map(|component| format!("{:?}: {}", component.component, component.detail))
            .chain(
                protocols
                    .iter()
                    .filter_map(|protocol| protocol.disabled_reason.clone()),
            )
            .chain(
                required_protocols
                    .iter()
                    .filter(|status| status.readiness != RuntimeProtocolReadiness::Ready)
                    .map(|status| status.detail.clone()),
            )
            .chain(
                sidecar_runtime
                    .requirements
                    .iter()
                    .filter(|requirement| {
                        requirement.status == SidecarRuntimeRequirementStatus::Blocked
                    })
                    .map(|requirement| {
                        format!(
                            "{:?} sidecar intent from {}:{} is blocked: {}",
                            requirement.protocol,
                            requirement.source,
                            requirement.source_ref,
                            requirement.reason
                        )
                    }),
            )
            .chain(render_blocking_detail.clone())
            .collect::<Vec<_>>();
        let ready = components.iter().all(|component| {
            !component.required || component.readiness == RuntimeComponentReadiness::Ready
        }) && required_protocols
            .iter()
            .all(|status| status.readiness == RuntimeProtocolReadiness::Ready)
            && sidecar_runtime.ready
            && render_blocking_detail.is_none();
        RuntimeValidationReport {
            generated_at_unix: now_unix(),
            ready,
            component_count: components.len(),
            components,
            protocol_count: protocols.len(),
            protocols,
            required_protocol_count: required_protocols.len(),
            required_protocols,
            sidecar_runtime,
            disabled_reasons,
        }
    }

    pub fn runtime_alerts(&self) -> Vec<RuntimeAlert> {
        let mut alerts = Vec::new();
        let observed_at_unix = now_unix();

        if self.state.consecutive_tick_failures > 0 {
            let detail = self
                .state
                .last_error
                .as_deref()
                .map(truncate_runtime_alert_detail)
                .unwrap_or_else(|| "node poll loop is failing".to_string());
            alerts.push(RuntimeAlert {
                alert_id: "poll_loop_backoff".to_string(),
                kind: RuntimeAlertKind::PollBackoff,
                severity: RuntimeAlertSeverity::Warning,
                source: RuntimeAlertSource::PollLoop,
                active: true,
                detail: format!(
                    "{} consecutive tick failure(s): {detail}",
                    self.state.consecutive_tick_failures
                ),
                observed_at_unix,
            });
        }

        if self.current_xray_runtime_status() == XrayRuntimeStatus::Failed {
            alerts.push(RuntimeAlert {
                alert_id: "xray_runtime_failed".to_string(),
                kind: RuntimeAlertKind::XrayRuntimeFailed,
                severity: RuntimeAlertSeverity::Critical,
                source: RuntimeAlertSource::Xray,
                active: true,
                detail: self
                    .state
                    .xray_runtime
                    .last_detail
                    .as_deref()
                    .map(truncate_runtime_alert_detail)
                    .unwrap_or_else(|| "xray runtime is failed".to_string()),
                observed_at_unix,
            });
        }

        if self.state.last_xray_update_status == Some(XrayUpdateStatus::Failed) {
            alerts.push(RuntimeAlert {
                alert_id: "xray_update_failed".to_string(),
                kind: RuntimeAlertKind::XrayUpdateFailed,
                severity: RuntimeAlertSeverity::Critical,
                source: RuntimeAlertSource::Xray,
                active: true,
                detail: self
                    .state
                    .last_xray_update_detail
                    .as_deref()
                    .map(truncate_runtime_alert_detail)
                    .unwrap_or_else(|| "xray update failed".to_string()),
                observed_at_unix,
            });
        }

        let validation_report = self.runtime_validation_report();
        if !validation_report.ready {
            let detail = validation_report
                .disabled_reasons
                .first()
                .map(|reason| {
                    format!(
                        "{} runtime validation blocker(s); first: {}",
                        validation_report.disabled_reasons.len(),
                        truncate_runtime_alert_detail(reason)
                    )
                })
                .unwrap_or_else(|| "runtime validation is not ready".to_string());
            alerts.push(RuntimeAlert {
                alert_id: "runtime_validation_failed".to_string(),
                kind: RuntimeAlertKind::RuntimeValidationFailed,
                severity: RuntimeAlertSeverity::Warning,
                source: RuntimeAlertSource::RuntimeValidation,
                active: true,
                detail,
                observed_at_unix,
            });
        }

        for sidecar in self.sidecar_state_views() {
            match sidecar.status {
                LocalSidecarStatus::Failed => alerts.push(RuntimeAlert {
                    alert_id: format!("sidecar_{:?}_failed", sidecar.sidecar).to_ascii_lowercase(),
                    kind: RuntimeAlertKind::SidecarFailed,
                    severity: RuntimeAlertSeverity::Critical,
                    source: RuntimeAlertSource::Sidecar,
                    active: true,
                    detail: sidecar
                        .last_detail
                        .as_deref()
                        .map(truncate_runtime_alert_detail)
                        .unwrap_or_else(|| format!("{:?} sidecar failed", sidecar.sidecar)),
                    observed_at_unix,
                }),
                LocalSidecarStatus::Degraded => alerts.push(RuntimeAlert {
                    alert_id: format!("sidecar_{:?}_degraded", sidecar.sidecar)
                        .to_ascii_lowercase(),
                    kind: RuntimeAlertKind::SidecarDegraded,
                    severity: RuntimeAlertSeverity::Warning,
                    source: RuntimeAlertSource::Sidecar,
                    active: true,
                    detail: sidecar
                        .last_detail
                        .as_deref()
                        .map(truncate_runtime_alert_detail)
                        .unwrap_or_else(|| format!("{:?} sidecar is degraded", sidecar.sidecar)),
                    observed_at_unix,
                }),
                LocalSidecarStatus::Disabled
                | LocalSidecarStatus::Missing
                | LocalSidecarStatus::Ready
                | LocalSidecarStatus::Running => {}
            }
        }

        alerts.truncate(MAX_RUNTIME_ALERTS);
        alerts
    }

    fn runtime_artifacts(&self) -> Vec<RuntimeArtifactView> {
        let sidecar_generated_dir =
            sidecar_generated_config_dir(&self.config.local_sidecar_runtime_config_path);
        let hysteria2_dir = sidecar_generated_dir.join("hysteria2");
        let wireguard_dir = sidecar_generated_dir.join("wireguard");
        let wireguard_session_mapping =
            wireguard_session_mapping_path(&self.config.local_sidecar_runtime_config_path);
        vec![
            runtime_file_artifact(
                RuntimeArtifactKind::GeneratedConfig,
                &self.config.local_config_path,
                self.state.last_config_saved_at_unix,
                false,
                false,
                "raw panel-generated config snapshot; not passed directly to runtime",
            ),
            runtime_file_artifact(
                RuntimeArtifactKind::NodeRuntimeConfig,
                &self.config.local_runtime_config_path,
                self.state.last_runtime_config_saved_at_unix,
                false,
                false,
                "node-local runtime intent document",
            ),
            runtime_file_artifact(
                RuntimeArtifactKind::SidecarRuntimeConfig,
                &self.config.local_sidecar_runtime_config_path,
                self.state.last_sidecar_runtime_config_saved_at_unix,
                false,
                false,
                "sidecar-owned protocol intent document; currently fail-closed and not executed",
            ),
            RuntimeArtifactView {
                kind: RuntimeArtifactKind::Hysteria2ConfigDirectory,
                path: hysteria2_dir.to_string_lossy().to_string(),
                exists: hysteria2_dir.is_dir(),
                last_saved_at_unix: self.state.last_sidecar_runtime_config_saved_at_unix,
                executable_runtime_input: true,
                secret_sensitive: true,
                detail:
                    "generated Hysteria2 candidate config directory; may contain auth material"
                        .to_string(),
            },
            RuntimeArtifactView {
                kind: RuntimeArtifactKind::WireGuardConfigDirectory,
                path: wireguard_dir.to_string_lossy().to_string(),
                exists: wireguard_dir.is_dir(),
                last_saved_at_unix: self.state.last_sidecar_runtime_config_saved_at_unix,
                executable_runtime_input: true,
                secret_sensitive: true,
                detail:
                    "generated WireGuard candidate config directory; may contain private keys"
                        .to_string(),
            },
            RuntimeArtifactView {
                kind: RuntimeArtifactKind::WireGuardSessionMapping,
                path: wireguard_session_mapping.to_string_lossy().to_string(),
                exists: wireguard_session_mapping.is_file(),
                last_saved_at_unix: self.state.last_sidecar_runtime_config_saved_at_unix,
                executable_runtime_input: true,
                secret_sensitive: true,
                detail: "owner-only WireGuard peer-to-runtime-principal mapping; private interface keys are excluded".to_string(),
            },
            runtime_file_artifact(
                RuntimeArtifactKind::XrayConfig,
                &self.config.local_xray_config_path,
                self.state.last_xray_config_saved_at_unix,
                true,
                false,
                "final generated Xray config; validated and passed to Xray runtime actions",
            ),
            runtime_file_artifact(
                RuntimeArtifactKind::RouteCredentialManifest,
                &self.config.route_credentials_path,
                self.state.last_route_credentials_saved_at_unix,
                false,
                true,
                "node-local route credential manifest; contains sensitive material file references",
            ),
            RuntimeArtifactView {
                kind: RuntimeArtifactKind::RouteCredentialDirectory,
                path: self.config.route_credentials_dir.clone(),
                exists: Path::new(&self.config.route_credentials_dir).is_dir(),
                last_saved_at_unix: self.state.last_route_credentials_saved_at_unix,
                executable_runtime_input: false,
                secret_sensitive: true,
                detail:
                    "node-local route credential directory; certificate/private key files are stored here"
                        .to_string(),
            },
        ]
    }

    fn sidecar_runtime_validation_report(&self) -> SidecarRuntimeValidationReport {
        let requirements = self.sidecar_runtime_requirements();
        let blocked_count = requirements
            .iter()
            .filter(|requirement| requirement.status == SidecarRuntimeRequirementStatus::Blocked)
            .count();
        let ready = blocked_count == 0;
        let executor_session = build_sidecar_executor_session(
            self.state.last_sidecar_runtime_summary.as_ref(),
            requirements.clone(),
        )
        .summary();
        let detail = if requirements.is_empty() {
            "no sidecar-owned runtime requirements".to_string()
        } else {
            format!(
                "{blocked_count}/{} sidecar runtime requirement(s) are blocked",
                requirements.len()
            )
        };

        SidecarRuntimeValidationReport {
            config_path: self.config.local_sidecar_runtime_config_path.clone(),
            summary: self.state.last_sidecar_runtime_summary.clone(),
            requirement_count: requirements.len(),
            blocked_count,
            executor_session,
            requirements,
            ready,
            detail,
        }
    }

    fn sidecar_runtime_requirements(&self) -> Vec<SidecarRuntimeRequirement> {
        self.state
            .last_runtime_protocol_requirements
            .iter()
            .filter_map(|requirement| {
                let sidecar = sidecar_kind_for_component(requirement.required_component)?;
                Some(self.sidecar_runtime_requirement(requirement, sidecar))
            })
            .collect()
    }

    fn sidecar_runtime_requirement(
        &self,
        requirement: &RuntimeProtocolRequirement,
        sidecar: LocalSidecarKind,
    ) -> SidecarRuntimeRequirement {
        let runtime_config = self.load_last_runtime_config().ok();
        let payload_exists = runtime_config.as_ref().is_some_and(|runtime_config| {
            sidecar_runtime_config_payload_exists(runtime_config, requirement)
        });
        let config_path = sidecar_generated_config_path(
            &self.config.local_sidecar_runtime_config_path,
            sidecar,
            &requirement.source_ref,
        );
        let config_exists = config_path.is_file();
        let session = build_sidecar_executor_session(
            self.state.last_sidecar_runtime_summary.as_ref(),
            vec![self.sidecar_runtime_requirement_candidate(requirement, sidecar)],
        );
        let accepted = self
            .state
            .last_accepted_sidecar_executor_session
            .as_ref()
            .is_some_and(|accepted| {
                accepted.session_id == session.session_id
                    && accepted.source_revision == session.source_revision
                    && accepted.command_ids == session.acceptance.required_command_ids
            });
        let component_ready = self.sidecar_state(sidecar).is_some_and(|state| {
            matches!(
                state.status,
                LocalSidecarStatus::Ready | LocalSidecarStatus::Running
            )
        });
        let ready = payload_exists && config_exists && accepted && component_ready;
        let (status, reason) = if ready {
            (
                SidecarRuntimeRequirementStatus::Ready,
                "sidecar runtime config file exists and accepted executor session matches current requirement".to_string(),
            )
        } else {
            let reason = match runtime_config.as_ref() {
            Some(_) if payload_exists && !config_exists => {
                "sidecar runtime config rendered but generated config file is missing".to_string()
            }
            Some(_) if payload_exists && !component_ready => {
                "sidecar runtime config rendered but sidecar component is not ready/running".to_string()
            }
            Some(_) if payload_exists && !accepted => {
                "sidecar runtime config rendered but matching executor session has not been accepted yet".to_string()
            }
            Some(_) => {
                "sidecar runtime config material is missing or invalid; requirement is fail-closed"
                    .to_string()
            }
            None => "sidecar runtime config has not been applied yet".to_string(),
        };
            (SidecarRuntimeRequirementStatus::Blocked, reason)
        };
        SidecarRuntimeRequirement {
            sidecar,
            protocol: requirement.protocol,
            source: requirement.source.clone(),
            source_ref: requirement.source_ref.clone(),
            status,
            reason,
            planned_envelopes: self.sidecar_runtime_executor_envelopes(
                sidecar,
                requirement.protocol,
                &requirement.source_ref,
            ),
        }
    }

    fn sidecar_runtime_requirement_candidate(
        &self,
        requirement: &RuntimeProtocolRequirement,
        sidecar: LocalSidecarKind,
    ) -> SidecarRuntimeRequirement {
        SidecarRuntimeRequirement {
            sidecar,
            protocol: requirement.protocol,
            source: requirement.source.clone(),
            source_ref: requirement.source_ref.clone(),
            status: SidecarRuntimeRequirementStatus::Blocked,
            reason: "sidecar runtime candidate".to_string(),
            planned_envelopes: self.sidecar_runtime_executor_envelopes(
                sidecar,
                requirement.protocol,
                &requirement.source_ref,
            ),
        }
    }

    fn load_last_runtime_config(&self) -> Result<NodeRuntimeConfigDocument> {
        let data = fs::read(&self.config.local_runtime_config_path).with_context(|| {
            format!(
                "failed to read node runtime config {}",
                self.config.local_runtime_config_path
            )
        })?;
        serde_json::from_slice::<NodeRuntimeConfigDocument>(&data).with_context(|| {
            format!(
                "failed to parse node runtime config {}",
                self.config.local_runtime_config_path
            )
        })
    }

    fn sidecar_runtime_executor_envelopes(
        &self,
        sidecar: LocalSidecarKind,
        protocol: RuntimeProtocolKind,
        source_ref: &str,
    ) -> Vec<SidecarRuntimeExecutorEnvelope> {
        [
            LocalSidecarAction::Validate,
            LocalSidecarAction::Start,
            LocalSidecarAction::Status,
        ]
        .into_iter()
        .map(|action| {
            let config_path = sidecar_generated_config_path(
                &self.config.local_sidecar_runtime_config_path,
                sidecar,
                source_ref,
            );
            let config_exists = config_path.is_file();
            let response = if action == LocalSidecarAction::Validate {
                if let Some(args) =
                    self.sidecar_action_args_for_requirement(sidecar, action, &config_path)
                {
                    let plan = configured_sidecar_command_plan(sidecar, action, &args);
                    LocalSidecarLifecycleResponse {
                        sidecar,
                        action,
                        status: sidecar_success_status(action),
                        supported: true,
                        plan,
                        acceptance: configured_sidecar_acceptance_contract(action),
                        binary_path: self.sidecar_binary_path(sidecar),
                        detected_version: None,
                        validated_at_unix: None,
                        detail: format!(
                            "{} {:?} has a standard OS recipe executor argv",
                            sidecar_name(sidecar),
                            action
                        ),
                        logs: Vec::new(),
                        updated_at_unix: now_unix(),
                    }
                } else {
                    self.sidecar_preflight_response(sidecar, action)
                }
            } else if let Some(args) =
                self.sidecar_action_args_for_requirement(sidecar, action, &config_path)
            {
                let plan = configured_sidecar_command_plan(sidecar, action, &args);
                LocalSidecarLifecycleResponse {
                    sidecar,
                    action,
                    status: sidecar_success_status(action),
                    supported: true,
                    plan,
                    acceptance: configured_sidecar_acceptance_contract(action),
                    binary_path: self.sidecar_binary_path(sidecar),
                    detected_version: None,
                    validated_at_unix: None,
                    detail: format!(
                        "{} {:?} has configured executor argv",
                        sidecar_name(sidecar),
                        action
                    ),
                    logs: Vec::new(),
                    updated_at_unix: now_unix(),
                }
            } else {
                placeholder_sidecar_lifecycle_response(sidecar, action)
            };
            SidecarRuntimeExecutorEnvelope {
                sidecar,
                action,
                command_id: sidecar_envelope_command_id(sidecar, source_ref, action),
                config_path: Some(config_path.to_string_lossy().to_string()),
                config_exists,
                plan: LocalSidecarCommandPlan {
                    command_id: sidecar_envelope_command_id(sidecar, source_ref, action),
                    ..response.plan
                },
                acceptance: response.acceptance,
                reason: format!(
                    "{:?} requirement {} uses {} {:?} executor envelope",
                    protocol,
                    source_ref,
                    sidecar_name(sidecar),
                    action
                ),
            }
        })
        .collect()
    }

    fn xray_component_report(&self) -> RuntimeComponentReport {
        let binary_path = self
            .xray_manager
            .binary_path
            .as_ref()
            .map(|path| path.display().to_string());
        let xray_config_exists = Path::new(&self.config.local_xray_config_path).is_file();
        let runtime_failed = self.current_xray_runtime_status() == XrayRuntimeStatus::Failed;
        let required = true;

        let (readiness, detail) = match self.xray_manager.binary_path.as_ref() {
            Some(path) if !path.is_file() => (
                RuntimeComponentReadiness::Missing,
                format!("configured xray binary does not exist: {}", path.display()),
            ),
            Some(_) if runtime_failed => (
                RuntimeComponentReadiness::Failed,
                self.state
                    .xray_runtime
                    .last_detail
                    .clone()
                    .unwrap_or_else(|| "xray runtime is failed".to_string()),
            ),
            Some(_)
                if self.state.xray_runtime.last_validated_at_unix.is_some()
                    && xray_config_exists =>
            {
                (
                    RuntimeComponentReadiness::Ready,
                    format!(
                        "xray binary configured, final config exists, last validation succeeded in {} mode",
                        self.xray_manager.apply_mode_name()
                    ),
                )
            }
            Some(_) if !xray_config_exists => (
                RuntimeComponentReadiness::Unknown,
                format!(
                    "xray binary configured but final config is missing: {}",
                    self.config.local_xray_config_path
                ),
            ),
            Some(_) => (
                RuntimeComponentReadiness::Unknown,
                "xray binary configured but no successful validation has been recorded".to_string(),
            ),
            None if self.xray_manager.requires_binary() => (
                RuntimeComponentReadiness::Missing,
                "external_process mode requires HYDRA_NODE_XRAY_BINARY_PATH".to_string(),
            ),
            None => (
                RuntimeComponentReadiness::Disabled,
                format!(
                    "no xray binary configured; current apply mode is {} and only non-runtime validation is available",
                    self.xray_manager.apply_mode_name()
                ),
            ),
        };

        RuntimeComponentReport {
            component: RuntimeComponentKind::Xray,
            required,
            readiness,
            binary_path,
            detected_version: self.state.xray_detected_version.clone(),
            last_validated_at_unix: self.state.xray_runtime.last_validated_at_unix,
            last_error: runtime_failed.then(|| {
                self.state
                    .xray_runtime
                    .last_detail
                    .clone()
                    .unwrap_or_else(|| "xray runtime failed".to_string())
            }),
            detail,
        }
    }

    fn sidecar_component_report(&self, sidecar: LocalSidecarKind) -> RuntimeComponentReport {
        let state = self
            .sidecar_state(sidecar)
            .cloned()
            .unwrap_or_else(|| default_sidecar_state(sidecar));
        RuntimeComponentReport {
            component: sidecar_runtime_component(sidecar),
            required: false,
            readiness: sidecar_runtime_readiness(state.status),
            binary_path: state.binary_path.clone(),
            detected_version: state.detected_version.clone(),
            last_validated_at_unix: state.last_validated_at_unix,
            last_error: (state.status == LocalSidecarStatus::Failed).then(|| {
                state
                    .last_detail
                    .clone()
                    .unwrap_or_else(|| "sidecar failed".to_string())
            }),
            detail: state
                .last_detail
                .clone()
                .unwrap_or_else(|| default_sidecar_detail(sidecar)),
        }
    }

    fn sidecar_state(&self, sidecar: LocalSidecarKind) -> Option<&PersistedSidecarState> {
        self.state
            .sidecar_states
            .iter()
            .find(|state| state.sidecar == sidecar)
    }

    fn update_sidecar_state(&mut self, response: &LocalSidecarLifecycleResponse) -> bool {
        let previous = self.sidecar_state(response.sidecar).cloned();
        let mut state = self
            .sidecar_state(response.sidecar)
            .cloned()
            .unwrap_or_else(|| default_sidecar_state(response.sidecar));
        state.status = response.status;
        state.supported = response.supported;
        state.binary_path = response.binary_path.clone();
        state.detected_version = response.detected_version.clone();
        state.last_action = Some(response.action);
        state.last_detail = Some(response.detail.clone());
        state.last_validated_at_unix = response.validated_at_unix;
        state.updated_at_unix = Some(response.updated_at_unix);
        state
            .logs
            .push(format!("{:?}: {}", response.action, response.detail));
        if state.logs.len() > MAX_SIDECAR_STATE_LOGS {
            let drain = state.logs.len() - MAX_SIDECAR_STATE_LOGS;
            state.logs.drain(0..drain);
        }
        self.state
            .sidecar_states
            .retain(|stored| stored.sidecar != response.sidecar);
        self.state.sidecar_states.push(state);
        previous.is_none_or(|previous| {
            previous.status != response.status
                || previous.supported != response.supported
                || previous.binary_path != response.binary_path
                || previous.detected_version != response.detected_version
                || previous.last_detail.as_deref() != Some(response.detail.as_str())
        })
    }

    fn sidecar_state_views(&self) -> Vec<LocalSidecarStateView> {
        [LocalSidecarKind::Hysteria2, LocalSidecarKind::WireGuard]
            .into_iter()
            .map(|sidecar| {
                let state = self
                    .sidecar_state(sidecar)
                    .cloned()
                    .unwrap_or_else(|| default_sidecar_state(sidecar));
                LocalSidecarStateView {
                    sidecar: state.sidecar,
                    status: state.status,
                    supported: state.supported,
                    binary_path: state.binary_path,
                    detected_version: state.detected_version,
                    last_action: state.last_action,
                    last_detail: state.last_detail,
                    last_validated_at_unix: state.last_validated_at_unix,
                    updated_at_unix: state.updated_at_unix,
                    logs: state.logs,
                }
            })
            .collect()
    }

    fn refresh_sidecar_preflight_state(&mut self) {
        for sidecar in [LocalSidecarKind::Hysteria2, LocalSidecarKind::WireGuard] {
            let response = self.sidecar_preflight_response(sidecar, LocalSidecarAction::Status);
            if self.update_sidecar_state(&response) {
                self.record_runtime_event(
                    "sidecar_preflight_state_changed",
                    format!(
                        "{} preflight is {:?}: {}",
                        sidecar_name(sidecar),
                        response.status,
                        response.detail
                    ),
                );
            }
        }
    }

    fn sidecar_preflight_response(
        &self,
        sidecar: LocalSidecarKind,
        action: LocalSidecarAction,
    ) -> LocalSidecarLifecycleResponse {
        let binary_path = self.sidecar_binary_path(sidecar);
        let Some(binary_path) = binary_path else {
            return sidecar_preflight_lifecycle_response(
                sidecar,
                action,
                LocalSidecarStatus::Disabled,
                false,
                None,
                None,
                format!("{} binary path is not configured", sidecar_name(sidecar)),
            );
        };
        let path = PathBuf::from(&binary_path);
        if !path.is_file() {
            return sidecar_preflight_lifecycle_response(
                sidecar,
                action,
                LocalSidecarStatus::Missing,
                false,
                Some(binary_path),
                None,
                format!(
                    "configured {} binary does not exist: {}",
                    sidecar_name(sidecar),
                    path.display()
                ),
            );
        }
        match detect_sidecar_version(sidecar, &path) {
            Ok(version) => {
                let helper = self.sidecar_helper_preflight(sidecar);
                let status = if helper.ready {
                    LocalSidecarStatus::Ready
                } else {
                    LocalSidecarStatus::Degraded
                };
                sidecar_preflight_lifecycle_response(
                    sidecar,
                    action,
                    status,
                    helper.ready,
                    Some(binary_path),
                    Some(version.clone()),
                    format!(
                        "{} binary preflight succeeded; detected version: {}; {}",
                        sidecar_name(sidecar),
                        version,
                        helper.detail
                    ),
                )
            }
            Err(error) => sidecar_preflight_lifecycle_response(
                sidecar,
                action,
                LocalSidecarStatus::Failed,
                false,
                Some(binary_path),
                None,
                format!(
                    "{} binary preflight failed: {}",
                    sidecar_name(sidecar),
                    error
                ),
            ),
        }
    }

    fn execute_configured_sidecar_command(
        &self,
        sidecar: LocalSidecarKind,
        action: LocalSidecarAction,
        args: Vec<String>,
    ) -> LocalSidecarLifecycleResponse {
        if args.is_empty() || args[0].trim().is_empty() {
            return placeholder_sidecar_lifecycle_response(sidecar, action);
        }
        let plan = configured_sidecar_command_plan(sidecar, action, &args);
        let acceptance = configured_sidecar_acceptance_contract(action);
        let command = &args[0];
        let command_args = &args[1..];
        let output = std::process::Command::new(command)
            .args(command_args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output();
        match output {
            Ok(output) => {
                let stdout = bounded_output(&output.stdout);
                let stderr = bounded_output(&output.stderr);
                let success = output.status.success();
                let status = if success {
                    sidecar_success_status(action)
                } else {
                    LocalSidecarStatus::Failed
                };
                let detail = format!(
                    "{} {:?} command {} with exit code {:?}",
                    sidecar_name(sidecar),
                    action,
                    if success { "succeeded" } else { "failed" },
                    output.status.code()
                );
                let mut logs = Vec::new();
                if !stdout.is_empty() {
                    logs.push(format!("stdout: {stdout}"));
                }
                if !stderr.is_empty() {
                    logs.push(format!("stderr: {stderr}"));
                }
                LocalSidecarLifecycleResponse {
                    sidecar,
                    action,
                    status,
                    supported: success,
                    plan,
                    acceptance,
                    binary_path: self.sidecar_binary_path(sidecar),
                    detected_version: None,
                    validated_at_unix: success
                        .then_some(now_unix())
                        .filter(|_| matches!(action, LocalSidecarAction::Validate)),
                    detail,
                    logs,
                    updated_at_unix: now_unix(),
                }
            }
            Err(error) => LocalSidecarLifecycleResponse {
                sidecar,
                action,
                status: LocalSidecarStatus::Failed,
                supported: false,
                plan,
                acceptance,
                binary_path: self.sidecar_binary_path(sidecar),
                detected_version: None,
                validated_at_unix: None,
                detail: format!(
                    "{} {:?} command failed to execute: {}",
                    sidecar_name(sidecar),
                    action,
                    error
                ),
                logs: Vec::new(),
                updated_at_unix: now_unix(),
            },
        }
    }

    fn sidecar_action_args(
        &self,
        sidecar: LocalSidecarKind,
        action: LocalSidecarAction,
    ) -> Option<Vec<String>> {
        self.explicit_sidecar_action_args(sidecar, action)
            .or_else(|| self.standard_sidecar_action_args(sidecar, action, None))
    }

    fn sidecar_action_args_for_requirement(
        &self,
        sidecar: LocalSidecarKind,
        action: LocalSidecarAction,
        config_path: &Path,
    ) -> Option<Vec<String>> {
        self.explicit_sidecar_action_args(sidecar, action)
            .or_else(|| self.standard_sidecar_action_args(sidecar, action, Some(config_path)))
    }

    fn explicit_sidecar_action_args(
        &self,
        sidecar: LocalSidecarKind,
        action: LocalSidecarAction,
    ) -> Option<Vec<String>> {
        let args = match (sidecar, action) {
            (LocalSidecarKind::Hysteria2, LocalSidecarAction::Install) => {
                &self.config.hysteria2_install_args
            }
            (LocalSidecarKind::Hysteria2, LocalSidecarAction::Update) => {
                &self.config.hysteria2_update_args
            }
            (LocalSidecarKind::Hysteria2, LocalSidecarAction::Start) => {
                &self.config.hysteria2_start_args
            }
            (LocalSidecarKind::Hysteria2, LocalSidecarAction::Stop) => {
                &self.config.hysteria2_stop_args
            }
            (LocalSidecarKind::Hysteria2, LocalSidecarAction::Restart) => {
                &self.config.hysteria2_restart_args
            }
            (LocalSidecarKind::Hysteria2, LocalSidecarAction::Status) => {
                &self.config.hysteria2_status_args
            }
            (LocalSidecarKind::Hysteria2, LocalSidecarAction::Logs) => {
                &self.config.hysteria2_logs_args
            }
            (LocalSidecarKind::Hysteria2, LocalSidecarAction::Validate) => {
                return None;
            }
            (LocalSidecarKind::WireGuard, LocalSidecarAction::Install) => {
                &self.config.wireguard_install_args
            }
            (LocalSidecarKind::WireGuard, LocalSidecarAction::Update) => {
                &self.config.wireguard_update_args
            }
            (LocalSidecarKind::WireGuard, LocalSidecarAction::Start) => {
                &self.config.wireguard_start_args
            }
            (LocalSidecarKind::WireGuard, LocalSidecarAction::Stop) => {
                &self.config.wireguard_stop_args
            }
            (LocalSidecarKind::WireGuard, LocalSidecarAction::Restart) => {
                &self.config.wireguard_restart_args
            }
            (LocalSidecarKind::WireGuard, LocalSidecarAction::Status) => {
                &self.config.wireguard_status_args
            }
            (LocalSidecarKind::WireGuard, LocalSidecarAction::Logs) => {
                &self.config.wireguard_logs_args
            }
            (LocalSidecarKind::WireGuard, LocalSidecarAction::Validate) => {
                return None;
            }
        };
        (!args.is_empty()).then(|| args.clone())
    }

    fn standard_sidecar_action_args(
        &self,
        sidecar: LocalSidecarKind,
        action: LocalSidecarAction,
        config_path: Option<&Path>,
    ) -> Option<Vec<String>> {
        if self.config.sidecar_recipe_mode != "standard" {
            return None;
        }
        match sidecar {
            LocalSidecarKind::Hysteria2 => {
                standard_hysteria2_action_args(action, &self.config.hysteria2_service_name)
            }
            LocalSidecarKind::WireGuard => standard_wireguard_action_args(
                action,
                self.config.wireguard_binary_path.as_deref(),
                self.config.wg_quick_binary_path.as_deref(),
                &self.config.wireguard_interface_name,
                config_path,
            ),
        }
    }

    fn sidecar_binary_path(&self, sidecar: LocalSidecarKind) -> Option<String> {
        match sidecar {
            LocalSidecarKind::Hysteria2 => self.config.hysteria2_binary_path.clone(),
            LocalSidecarKind::WireGuard => self.config.wireguard_binary_path.clone(),
        }
    }

    fn sidecar_helper_preflight(&self, sidecar: LocalSidecarKind) -> SidecarHelperPreflight {
        match sidecar {
            LocalSidecarKind::Hysteria2 => SidecarHelperPreflight {
                ready: true,
                detail: "no additional helper is required".to_string(),
            },
            LocalSidecarKind::WireGuard => match self.config.wg_quick_binary_path.as_deref() {
                Some(path) if Path::new(path).is_file() => SidecarHelperPreflight {
                    ready: true,
                    detail: format!("wg-quick helper configured: {path}"),
                },
                Some(path) => SidecarHelperPreflight {
                    ready: false,
                    detail: format!("wg-quick helper configured but missing: {path}"),
                },
                None => SidecarHelperPreflight {
                    ready: false,
                    detail: "wg-quick helper is not configured".to_string(),
                },
            },
        }
    }

    fn required_protocol_blocking_detail(&self) -> Option<String> {
        let components = vec![
            self.xray_component_report(),
            self.sidecar_component_report(LocalSidecarKind::Hysteria2),
            self.sidecar_component_report(LocalSidecarKind::WireGuard),
        ];
        let protocols = runtime_protocol_reports(&components);
        let blocked = runtime_protocol_requirement_statuses(
            &self.state.last_runtime_protocol_requirements,
            &protocols,
            Some(&self.sidecar_runtime_validation_report()),
        )
        .into_iter()
        .filter(|status| status.readiness != RuntimeProtocolReadiness::Ready)
        .take(5)
        .map(|status| status.detail)
        .collect::<Vec<_>>();
        (!blocked.is_empty()).then(|| {
            format!(
                "runtime protocol requirements not ready: {}",
                blocked.join("; ")
            )
        })
    }

    fn xray_render_blocking_detail(&self) -> Option<String> {
        let summary = self.state.last_xray_render_summary.as_ref()?;
        if !summary.fail_closed {
            return None;
        }
        let sidecar_runtime = self.sidecar_runtime_validation_report();
        let issues = summary
            .issues
            .iter()
            .filter(|issue| !xray_render_issue_resolved_by_sidecar_runtime(issue, &sidecar_runtime))
            .take(5)
            .map(|issue| format!("{}:{}:{}", issue.scope, issue.route_id, issue.reason))
            .collect::<Vec<_>>();
        if issues.is_empty() && summary.issue_count > 0 {
            return None;
        }
        Some(format!(
            "xray render is fail-closed: {}",
            if issues.is_empty() {
                "render summary marked fail-closed".to_string()
            } else {
                issues.join("; ")
            }
        ))
    }

    fn subscription_session_adapter_view(&self) -> SubscriptionSessionAdapterView {
        let configured = self.config.subscription_session_adapter_token.is_some();
        let active_lease = self
            .active_subscription_session_adapter
            .as_ref()
            .filter(|lease| lease.lease_expires_at_unix >= now_unix());
        let active_staged = self.staged_subscription_sessions.as_ref().filter(|staged| {
            now_unix().saturating_sub(staged.received_at_unix)
                <= self
                    .config
                    .subscription_session_observation_stale_after_seconds
                && active_lease
                    .is_some_and(|lease| lease.adapter_instance_id == staged.adapter_instance_id)
        });
        let declared_capabilities = self
            .staged_subscription_sessions
            .as_ref()
            .filter(|_| active_staged.is_some())
            .map(|staged| staged.request.runtime_capabilities.clone())
            .unwrap_or_default();
        let exact_ready = configured
            && active_lease.is_some()
            && exact_subscription_session_capabilities(&declared_capabilities);
        SubscriptionSessionAdapterView {
            status: if exact_ready {
                SubscriptionSessionAdapterStatus::ExactEnforcementReady
            } else if active_lease.is_some() {
                SubscriptionSessionAdapterStatus::ObservationOnly
            } else {
                SubscriptionSessionAdapterStatus::Unsupported
            },
            observation_source: active_lease
                .is_some()
                .then_some(SubscriptionSessionObservationSource::NodeManagedRuntimeTable),
            runtime_capabilities: declared_capabilities,
            exact_session_termination_ready: exact_ready,
            disabled_reason: (!exact_ready).then(|| if active_lease.is_some() {
                "no active exact-capable local adapter snapshot is available; observation-only mode cannot terminate sessions"
                    .to_string()
            } else if configured {
                "session adapter token is configured, but no trusted local adapter currently holds an active lease"
                    .to_string()
            } else {
                "exact runtime session observation/termination adapter is not implemented; Xray process control alone is not exact per-session enforcement"
                    .to_string()
            }),
            buffered_observation_count: self
                .staged_subscription_sessions
                .as_ref()
                .filter(|_| active_staged.is_some())
                .map(|staged| staged.request.observations.len())
                .unwrap_or_default(),
            last_observation_at_unix: active_staged.map(|staged| staged.received_at_unix),
            last_report_at_unix: self.state.last_subscription_sessions_report_at_unix,
            last_reported_count: self.state.last_subscription_sessions_reported_count,
            last_blocked_count: self.state.last_subscription_sessions_blocked_count,
            pending_enforcement_count: self.pending_subscription_session_enforcements.len(),
            adapter_registered: active_lease.is_some(),
            active_lease_expires_at_unix: active_lease.map(|lease| lease.lease_expires_at_unix),
        }
    }

    fn require_active_subscription_session_adapter(
        &self,
        adapter_instance_id: &str,
        capabilities: &[SubscriptionSessionRuntimeCapability],
    ) -> Result<()> {
        let active =
            self.require_active_subscription_session_adapter_instance(adapter_instance_id)?;
        if active.runtime_capabilities != capabilities {
            bail!("local session adapter snapshot capability set does not match its active lease");
        }
        Ok(())
    }

    fn require_active_subscription_session_adapter_instance(
        &self,
        adapter_instance_id: &str,
    ) -> Result<&ActiveSubscriptionSessionAdapterLease> {
        let active = self
            .active_subscription_session_adapter
            .as_ref()
            .context("local subscription session adapter is not registered")?;
        if active.lease_expires_at_unix < now_unix() {
            bail!("local subscription session adapter lease expired");
        }
        if active.adapter_instance_id != adapter_instance_id {
            bail!("local subscription session adapter instance does not own the active lease");
        }
        Ok(active)
    }

    fn record_apply_event(
        &mut self,
        revision: Option<String>,
        status: NodeSyncStatus,
        detail: String,
    ) {
        self.apply_history.push(ApplyHistoryEntry {
            revision,
            status,
            detail,
            created_at_unix: now_unix(),
        });
        if self.apply_history.len() > self.config.max_apply_history_entries {
            let drain = self.apply_history.len() - self.config.max_apply_history_entries;
            self.apply_history.drain(0..drain);
        }
    }

    fn record_runtime_event(&mut self, kind: &str, detail: String) {
        self.runtime_events.push(RuntimeEventEntry {
            kind: kind.to_string(),
            detail,
            created_at_unix: now_unix(),
        });
        if self.runtime_events.len() > self.config.max_runtime_event_entries {
            let drain = self.runtime_events.len() - self.config.max_runtime_event_entries;
            self.runtime_events.drain(0..drain);
        }
    }

    fn refresh_runtime_process_state(&mut self) -> Result<()> {
        if let Some(child) = self.xray_child.as_mut() {
            if let Some(status) = child
                .try_wait()
                .context("failed to poll xray process state")?
            {
                let detail = match status.code() {
                    Some(code) => format!("xray process exited with code {code}"),
                    None => "xray process exited without code".to_string(),
                };
                self.state.xray_runtime.status = Some(XrayRuntimeStatus::Failed);
                self.state.xray_runtime.last_pid = None;
                self.state.xray_runtime.last_exit_code = status.code();
                self.state.xray_runtime.last_detail = Some(detail.clone());
                self.state.xray_runtime.restart_attempts =
                    self.state.xray_runtime.restart_attempts.saturating_add(1);
                let backoff = calculate_restart_backoff_seconds(
                    self.state.xray_runtime.restart_attempts,
                    self.config.xray_restart_backoff_base_seconds,
                    self.config.xray_restart_backoff_max_seconds,
                );
                self.state.xray_runtime.next_restart_not_before_unix = Some(now_unix() + backoff);
                self.push_log("error", detail);
                self.record_runtime_event(
                    "xray_process_exited",
                    format!(
                        "process exited; next restart not before {}",
                        self.state
                            .xray_runtime
                            .next_restart_not_before_unix
                            .unwrap_or_default()
                    ),
                );
                self.xray_child = None;
            } else {
                self.state.xray_runtime.status = Some(XrayRuntimeStatus::Running);
                self.state.xray_runtime.last_pid = child.id();
                self.state.xray_runtime.restart_attempts = 0;
                self.state.xray_runtime.next_restart_not_before_unix = None;
            }
        } else if self.state.xray_runtime.status == Some(XrayRuntimeStatus::Failed)
            && let Some(not_before) = self.state.xray_runtime.next_restart_not_before_unix
            && now_unix() >= not_before
            && Path::new(&self.config.local_xray_config_path).is_file()
        {
            let restart_result = self.xray_manager.run_action(
                LocalRuntimeAction::Start,
                Path::new(&self.config.local_xray_config_path),
                &mut self.state.xray_runtime,
                &mut self.xray_child,
            );
            match restart_result {
                Ok(detail) => {
                    self.state.xray_runtime.restart_attempts = 0;
                    self.state.xray_runtime.next_restart_not_before_unix = None;
                    self.push_log(
                        "info",
                        format!("automatic xray restart succeeded: {detail}"),
                    );
                    self.record_runtime_event("xray_auto_restart_succeeded", detail);
                }
                Err(error) => {
                    self.state.xray_runtime.restart_attempts =
                        self.state.xray_runtime.restart_attempts.saturating_add(1);
                    let backoff = calculate_restart_backoff_seconds(
                        self.state.xray_runtime.restart_attempts,
                        self.config.xray_restart_backoff_base_seconds,
                        self.config.xray_restart_backoff_max_seconds,
                    );
                    self.state.xray_runtime.next_restart_not_before_unix =
                        Some(now_unix() + backoff);
                    self.push_log("error", format!("automatic xray restart failed: {error}"));
                    self.record_runtime_event("xray_auto_restart_failed", error.to_string());
                }
            }
        }
        Ok(())
    }
}

fn validate_subscription_session_observation_snapshot(
    request: &ReportSubscriptionSessionsRequest,
    max_observations: usize,
) -> Result<()> {
    if request.observation_source != SubscriptionSessionObservationSource::NodeManagedRuntimeTable {
        bail!("unsupported subscription session observation source");
    }
    if !request.runtime_capabilities.is_empty()
        && !exact_subscription_session_capabilities(&request.runtime_capabilities)
    {
        bail!(
            "local subscription session adapter must declare either no capabilities or the complete exact-session capability set"
        );
    }
    if request.observations.len() > max_observations {
        bail!("subscription session observation snapshot exceeds configured limit");
    }
    for observation in &request.observations {
        if observation.session_id.trim().is_empty() || observation.session_id.len() > 128 {
            bail!("subscription session id must be between 1 and 128 characters");
        }
        if observation.runtime_username.trim().is_empty()
            || observation.runtime_username.len() > 128
        {
            bail!("subscription session runtime username must be between 1 and 128 characters");
        }
        if request.runtime_capabilities.is_empty() && observation.runtime_session_ref.is_some() {
            bail!(
                "observation-only adapter must not submit exact runtime session references before exact enforcement is implemented"
            );
        }
        if exact_subscription_session_capabilities(&request.runtime_capabilities)
            && observation
                .runtime_session_ref
                .as_ref()
                .is_none_or(|value| value.trim().is_empty() || value.len() > 256)
        {
            bail!("exact session adapter observations require a bounded opaque runtime handle");
        }
        if observation
            .device_fingerprint
            .as_ref()
            .is_some_and(|value| value.trim().is_empty() || value.len() > 256)
        {
            bail!("subscription session device fingerprint is invalid");
        }
        if observation
            .source_ip
            .as_ref()
            .is_some_and(|value| value.trim().is_empty() || value.len() > 128)
        {
            bail!("subscription session source IP is invalid");
        }
    }
    Ok(())
}

fn validate_subscription_session_adapter_registration(
    request: &RegisterLocalSubscriptionSessionAdapterRequest,
) -> Result<()> {
    if request.adapter_instance_id.trim().is_empty() || request.adapter_instance_id.len() > 128 {
        bail!("local session adapter instance id must be between 1 and 128 characters");
    }
    if !request.runtime_capabilities.is_empty()
        && !exact_subscription_session_capabilities(&request.runtime_capabilities)
    {
        bail!(
            "local session adapter lease must declare either no capabilities or the complete exact-session capability set"
        );
    }
    Ok(())
}

fn exact_subscription_session_capabilities(
    capabilities: &[SubscriptionSessionRuntimeCapability],
) -> bool {
    capabilities.len() == 3
        && capabilities.contains(&SubscriptionSessionRuntimeCapability::OpaqueSessionReference)
        && capabilities.contains(&SubscriptionSessionRuntimeCapability::ExactSessionTermination)
        && capabilities
            .contains(&SubscriptionSessionRuntimeCapability::PostActionAbsenceVerification)
}

fn validate_local_subscription_session_enforcement_result(
    command: &LocalSubscriptionSessionEnforcementCommand,
    result: &CompleteLocalSubscriptionSessionEnforcementRequest,
) -> Result<()> {
    if result.status == SubscriptionSessionEnforcementStatus::Pending {
        bail!("local adapter cannot report pending as an enforcement result");
    }
    if result.status == SubscriptionSessionEnforcementStatus::Applied {
        let runtime_session_ref = result
            .runtime_session_ref
            .as_deref()
            .context("applied enforcement requires the opaque runtime session reference")?;
        if !constant_time_slice_eq(
            runtime_session_ref.as_bytes(),
            command.runtime_session_ref.as_bytes(),
        ) {
            bail!("applied enforcement runtime session reference does not match queued action");
        }
        if result.session_absent_after_action != Some(true) || result.verified_at_unix.is_none() {
            bail!("applied enforcement requires post-action absence verification");
        }
    }
    if result
        .detail
        .as_ref()
        .is_some_and(|detail| detail.len() > 512)
    {
        bail!("local enforcement result detail exceeds maximum length");
    }
    Ok(())
}

fn constant_time_slice_eq(left: &[u8], right: &[u8]) -> bool {
    let max_len = left.len().max(right.len());
    let mut diff = left.len() ^ right.len();
    for index in 0..max_len {
        let left = left.get(index).copied().unwrap_or(0);
        let right = right.get(index).copied().unwrap_or(0);
        diff |= usize::from(left ^ right);
    }
    diff == 0
}

fn collect_metrics() -> NodeMetricsRequest {
    let mut system = System::new_all();
    system.refresh_memory();
    let disks = Disks::new_with_refreshed_list();

    let memory_total_bytes = system.total_memory();
    let memory_used_bytes = system.used_memory();

    let (disk_total_bytes, disk_used_bytes) =
        disks
            .iter()
            .fold((0_u64, 0_u64), |(total_acc, used_acc), disk| {
                let total = disk.total_space();
                let used = total.saturating_sub(disk.available_space());
                (
                    total_acc.saturating_add(total),
                    used_acc.saturating_add(used),
                )
            });

    NodeMetricsRequest {
        memory_used_bytes,
        memory_total_bytes,
        disk_used_bytes,
        disk_total_bytes,
    }
}

async fn fetch_latest_xray_release() -> Result<GitHubReleaseResponse> {
    reqwest::Client::builder()
        .user_agent("hydra-node-rust/0.1")
        .build()
        .context("failed to build github release client")?
        .get("https://api.github.com/repos/XTLS/Xray-core/releases/latest")
        .send()
        .await
        .context("xray latest release request failed")?
        .error_for_status()
        .context("xray latest release returned error status")?
        .json::<GitHubReleaseResponse>()
        .await
        .context("failed to decode xray latest release response")
}

fn expected_xray_asset_name() -> Result<String> {
    match (OS, ARCH) {
        ("linux", "x86_64") => Ok("Xray-linux-64.zip".to_string()),
        ("linux", "aarch64") => Ok("Xray-linux-arm64-v8a.zip".to_string()),
        ("linux", "arm") => Ok("Xray-linux-arm32-v7a.zip".to_string()),
        _ => bail!("unsupported platform for xray auto-update: {OS}/{ARCH}"),
    }
}

async fn download_and_install_xray_binary(download_url: &str, binary_path: &Path) -> Result<()> {
    validate_xray_download_url(download_url)?;

    let response = reqwest::Client::builder()
        .user_agent("hydra-node-rust/0.1")
        .build()
        .context("failed to build xray download client")?
        .get(download_url)
        .send()
        .await
        .with_context(|| format!("xray download request failed: {download_url}"))?
        .error_for_status()
        .context("xray asset download returned error status")?;

    if response
        .content_length()
        .is_some_and(|length| length > MAX_XRAY_DOWNLOAD_BYTES)
    {
        bail!(
            "xray asset download is larger than allowed limit of {} bytes",
            MAX_XRAY_DOWNLOAD_BYTES
        );
    }

    let bytes = response
        .bytes()
        .await
        .context("failed to read xray asset bytes")?;
    if bytes.len() as u64 > MAX_XRAY_DOWNLOAD_BYTES {
        bail!(
            "xray asset download is larger than allowed limit of {} bytes",
            MAX_XRAY_DOWNLOAD_BYTES
        );
    }

    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .context("failed to open downloaded xray zip asset")?;
    let file = archive
        .by_name("xray")
        .context("downloaded xray archive does not contain xray binary")?;
    if file.size() > MAX_XRAY_DOWNLOAD_BYTES {
        bail!(
            "xray binary inside archive is larger than allowed limit of {} bytes",
            MAX_XRAY_DOWNLOAD_BYTES
        );
    }
    let mut binary = Vec::new();
    let mut limited_file = file.take(MAX_XRAY_DOWNLOAD_BYTES + 1);
    limited_file
        .read_to_end(&mut binary)
        .context("failed to read xray binary from archive")?;
    if binary.len() as u64 > MAX_XRAY_DOWNLOAD_BYTES {
        bail!(
            "xray binary inside archive is larger than allowed limit of {} bytes",
            MAX_XRAY_DOWNLOAD_BYTES
        );
    }

    ensure_parent_dir(binary_path)?;
    let temp_path = temp_path(binary_path);
    let mut temp_file = fs::File::create(&temp_path)
        .with_context(|| format!("failed to create {}", temp_path.display()))?;
    temp_file
        .write_all(&binary)
        .with_context(|| format!("failed to write {}", temp_path.display()))?;
    #[cfg(unix)]
    {
        let mut permissions = temp_file
            .metadata()
            .with_context(|| format!("failed to stat {}", temp_path.display()))?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&temp_path, permissions)
            .with_context(|| format!("failed to chmod {}", temp_path.display()))?;
    }
    fs::rename(&temp_path, binary_path).with_context(|| {
        format!(
            "failed to move downloaded xray binary from {} to {}",
            temp_path.display(),
            binary_path.display()
        )
    })?;
    Ok(())
}

fn validate_xray_download_url(download_url: &str) -> Result<()> {
    let url = reqwest::Url::parse(download_url).context("invalid xray download URL")?;
    if url.scheme() != "https" {
        bail!("xray download URL must use https");
    }
    let host = url.host_str().context("xray download URL has no host")?;
    let allowed = matches!(
        host,
        "github.com"
            | "objects.githubusercontent.com"
            | "github-releases.githubusercontent.com"
            | "release-assets.githubusercontent.com"
    );
    if !allowed {
        bail!("xray download URL host is not trusted: {host}");
    }
    Ok(())
}

fn backup_xray_binary_before_update(binary_path: &Path) -> Result<Option<PathBuf>> {
    if !binary_path.is_file() {
        return Ok(None);
    }

    let backup_path = xray_binary_update_backup_path(binary_path);
    ensure_parent_dir(&backup_path)?;
    fs::copy(binary_path, &backup_path).with_context(|| {
        format!(
            "failed to backup xray binary from {} to {}",
            binary_path.display(),
            backup_path.display()
        )
    })?;
    #[cfg(unix)]
    {
        let permissions = fs::metadata(binary_path)
            .with_context(|| format!("failed to stat {}", binary_path.display()))?
            .permissions();
        fs::set_permissions(&backup_path, permissions)
            .with_context(|| format!("failed to chmod {}", backup_path.display()))?;
    }
    Ok(Some(backup_path))
}

fn restore_xray_binary_after_failed_update(
    binary_path: &Path,
    backup_path: Option<&Path>,
) -> Result<String> {
    match backup_path {
        Some(backup_path) if backup_path.is_file() => {
            fs::copy(backup_path, binary_path).with_context(|| {
                format!(
                    "failed to restore previous xray binary from {} to {}",
                    backup_path.display(),
                    binary_path.display()
                )
            })?;
            #[cfg(unix)]
            {
                let permissions = fs::metadata(backup_path)
                    .with_context(|| format!("failed to stat {}", backup_path.display()))?
                    .permissions();
                fs::set_permissions(binary_path, permissions)
                    .with_context(|| format!("failed to chmod {}", binary_path.display()))?;
            }
            Ok(format!(
                "restored previous xray binary from {}",
                backup_path.display()
            ))
        }
        Some(backup_path) => bail!(
            "xray update failed and backup binary is missing: {}",
            backup_path.display()
        ),
        None => {
            if binary_path.exists() {
                fs::remove_file(binary_path).with_context(|| {
                    format!(
                        "failed to remove invalid xray binary at {}",
                        binary_path.display()
                    )
                })?;
            }
            Ok("removed invalid xray binary because no previous binary existed".to_string())
        }
    }
}

fn xray_binary_update_backup_path(binary_path: &Path) -> PathBuf {
    let mut backup = binary_path.to_path_buf();
    let file_name = binary_path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| format!("{name}.pre-update.bak"))
        .unwrap_or_else(|| "xray.pre-update.bak".to_string());
    backup.set_file_name(file_name);
    backup
}

fn runtime_protocol_reports(components: &[RuntimeComponentReport]) -> Vec<RuntimeProtocolReport> {
    vec![
        xray_protocol_report(RuntimeProtocolKind::VlessTlsWebSocket, components),
        sidecar_protocol_report(
            RuntimeProtocolKind::Hysteria2,
            RuntimeComponentKind::Hysteria2,
            components,
        ),
        sidecar_protocol_report(
            RuntimeProtocolKind::WireGuard,
            RuntimeComponentKind::WireGuard,
            components,
        ),
    ]
}

fn runtime_protocol_requirement_statuses(
    requirements: &[RuntimeProtocolRequirement],
    protocols: &[RuntimeProtocolReport],
    sidecar_runtime: Option<&SidecarRuntimeValidationReport>,
) -> Vec<RuntimeProtocolRequirementStatus> {
    requirements
        .iter()
        .take(MAX_RUNTIME_PROTOCOL_REQUIREMENTS)
        .map(|requirement| {
            if let Some(sidecar_runtime) = sidecar_runtime
                && let Some(sidecar_requirement) =
                    sidecar_runtime
                        .requirements
                        .iter()
                        .find(|sidecar_requirement| {
                            sidecar_requirement.protocol == requirement.protocol
                                && sidecar_requirement.source == requirement.source
                                && sidecar_requirement.source_ref == requirement.source_ref
                        })
            {
                let readiness = match sidecar_requirement.status {
                    SidecarRuntimeRequirementStatus::Ready => RuntimeProtocolReadiness::Ready,
                    SidecarRuntimeRequirementStatus::Blocked => RuntimeProtocolReadiness::Blocked,
                };
                return RuntimeProtocolRequirementStatus {
                    requirement: requirement.clone(),
                    readiness,
                    detail: if readiness == RuntimeProtocolReadiness::Ready {
                        format!(
                            "{:?} requirement from {}:{} is ready: {}",
                            requirement.protocol,
                            requirement.source,
                            requirement.source_ref,
                            sidecar_requirement.reason
                        )
                    } else {
                        format!(
                            "{:?} requirement from {}:{} is blocked: {}",
                            requirement.protocol,
                            requirement.source,
                            requirement.source_ref,
                            sidecar_requirement.reason
                        )
                    },
                };
            }
            let report = protocols
                .iter()
                .find(|report| report.protocol == requirement.protocol);
            let readiness = report
                .map(|report| report.readiness)
                .unwrap_or(RuntimeProtocolReadiness::Blocked);
            let detail = match report {
                Some(report) if report.readiness == RuntimeProtocolReadiness::Ready => format!(
                    "{:?} requirement from {}:{} is ready",
                    requirement.protocol, requirement.source, requirement.source_ref
                ),
                Some(report) => format!(
                    "{:?} requirement from {}:{} is {:?}: {}",
                    requirement.protocol,
                    requirement.source,
                    requirement.source_ref,
                    report.readiness,
                    report
                        .disabled_reason
                        .as_deref()
                        .unwrap_or("runtime protocol is not ready")
                ),
                None => format!(
                    "{:?} requirement from {}:{} is blocked: protocol readiness report missing",
                    requirement.protocol, requirement.source, requirement.source_ref
                ),
            };
            RuntimeProtocolRequirementStatus {
                requirement: requirement.clone(),
                readiness,
                detail,
            }
        })
        .collect()
}

fn xray_render_issue_resolved_by_sidecar_runtime(
    issue: &XrayRenderIssue,
    sidecar_runtime: &SidecarRuntimeValidationReport,
) -> bool {
    issue.reason == "non_xray_protocol_requires_sidecar"
        && sidecar_runtime.requirements.iter().any(|requirement| {
            requirement.status == SidecarRuntimeRequirementStatus::Ready
                && requirement.source == issue.scope
                && requirement.source_ref == issue.route_id
        })
}

fn runtime_file_artifact(
    kind: RuntimeArtifactKind,
    path: &str,
    last_saved_at_unix: Option<u64>,
    executable_runtime_input: bool,
    secret_sensitive: bool,
    detail: &str,
) -> RuntimeArtifactView {
    RuntimeArtifactView {
        kind,
        path: path.to_string(),
        exists: Path::new(path).is_file(),
        last_saved_at_unix,
        executable_runtime_input,
        secret_sensitive,
        detail: detail.to_string(),
    }
}

fn truncate_runtime_alert_detail(value: &str) -> String {
    let normalized = value.trim().replace(['\r', '\n'], " ");
    let mut redacted_words = Vec::new();
    let mut redact_next = false;
    for word in normalized.split_whitespace() {
        if redact_next {
            redacted_words.push("[redacted]".to_string());
            redact_next = false;
            continue;
        }
        if word.eq_ignore_ascii_case("bearer") {
            redacted_words.push("Bearer".to_string());
            redact_next = true;
            continue;
        }
        let lower = word.to_ascii_lowercase();
        if lower.contains("token=") || lower.contains("auth_token=") {
            redacted_words.push("[redacted-token]".to_string());
        } else {
            redacted_words.push(word.to_string());
        }
    }
    let mut detail = redacted_words.join(" ");
    if detail.len() > MAX_RUNTIME_ALERT_DETAIL_LEN {
        detail.truncate(MAX_RUNTIME_ALERT_DETAIL_LEN);
        detail.push_str("...");
    }
    detail
}

fn runtime_alert_to_domain(alert: RuntimeAlert) -> node_domain::RuntimeAlert {
    node_domain::RuntimeAlert {
        alert_id: alert.alert_id,
        kind: match alert.kind {
            RuntimeAlertKind::PollBackoff => node_domain::RuntimeAlertKind::PollBackoff,
            RuntimeAlertKind::RuntimeValidationFailed => {
                node_domain::RuntimeAlertKind::RuntimeValidationFailed
            }
            RuntimeAlertKind::XrayRuntimeFailed => node_domain::RuntimeAlertKind::XrayRuntimeFailed,
            RuntimeAlertKind::XrayUpdateFailed => node_domain::RuntimeAlertKind::XrayUpdateFailed,
            RuntimeAlertKind::SidecarFailed => node_domain::RuntimeAlertKind::SidecarFailed,
            RuntimeAlertKind::SidecarDegraded => node_domain::RuntimeAlertKind::SidecarDegraded,
        },
        severity: match alert.severity {
            RuntimeAlertSeverity::Warning => node_domain::RuntimeAlertSeverity::Warning,
            RuntimeAlertSeverity::Critical => node_domain::RuntimeAlertSeverity::Critical,
        },
        source: match alert.source {
            RuntimeAlertSource::PollLoop => node_domain::RuntimeAlertSource::PollLoop,
            RuntimeAlertSource::RuntimeValidation => {
                node_domain::RuntimeAlertSource::RuntimeValidation
            }
            RuntimeAlertSource::Xray => node_domain::RuntimeAlertSource::Xray,
            RuntimeAlertSource::Sidecar => node_domain::RuntimeAlertSource::Sidecar,
        },
        active: alert.active,
        detail: alert.detail,
        observed_at_unix: alert.observed_at_unix,
    }
}

fn xray_protocol_report(
    protocol: RuntimeProtocolKind,
    components: &[RuntimeComponentReport],
) -> RuntimeProtocolReport {
    component_protocol_report(protocol, RuntimeComponentKind::Xray, components)
}

fn sidecar_protocol_report(
    protocol: RuntimeProtocolKind,
    component: RuntimeComponentKind,
    components: &[RuntimeComponentReport],
) -> RuntimeProtocolReport {
    match components
        .iter()
        .find(|report| report.component == component)
    {
        Some(report) if report.readiness == RuntimeComponentReadiness::Ready => {
            RuntimeProtocolReport {
                protocol,
                readiness: RuntimeProtocolReadiness::Blocked,
                required_components: vec![component],
                disabled_reason: Some(format!(
                    "{:?} blocked until generated sidecar config and matching executor session are ready",
                    protocol
                )),
            }
        }
        _ => component_protocol_report(protocol, component, components),
    }
}

fn component_protocol_report(
    protocol: RuntimeProtocolKind,
    component: RuntimeComponentKind,
    components: &[RuntimeComponentReport],
) -> RuntimeProtocolReport {
    match components
        .iter()
        .find(|report| report.component == component)
    {
        Some(report) if report.readiness == RuntimeComponentReadiness::Ready => {
            RuntimeProtocolReport {
                protocol,
                readiness: RuntimeProtocolReadiness::Ready,
                required_components: vec![component],
                disabled_reason: None,
            }
        }
        Some(report) if report.readiness == RuntimeComponentReadiness::Disabled => {
            RuntimeProtocolReport {
                protocol,
                readiness: RuntimeProtocolReadiness::Disabled,
                required_components: vec![component],
                disabled_reason: Some(format!(
                    "{:?} disabled because {:?} is disabled: {}",
                    protocol, component, report.detail
                )),
            }
        }
        Some(report) => RuntimeProtocolReport {
            protocol,
            readiness: RuntimeProtocolReadiness::Blocked,
            required_components: vec![component],
            disabled_reason: Some(format!(
                "{:?} blocked because {:?} is {:?}: {}",
                protocol, component, report.readiness, report.detail
            )),
        },
        None => RuntimeProtocolReport {
            protocol,
            readiness: RuntimeProtocolReadiness::Blocked,
            required_components: vec![component],
            disabled_reason: Some(format!(
                "{:?} blocked because {:?} component report is missing",
                protocol, component
            )),
        },
    }
}

fn default_sidecar_state(sidecar: LocalSidecarKind) -> PersistedSidecarState {
    PersistedSidecarState {
        sidecar,
        status: LocalSidecarStatus::Disabled,
        supported: false,
        binary_path: None,
        detected_version: None,
        last_action: None,
        last_detail: Some(default_sidecar_detail(sidecar)),
        last_validated_at_unix: None,
        updated_at_unix: None,
        logs: Vec::new(),
    }
}

fn default_sidecar_detail(sidecar: LocalSidecarKind) -> String {
    format!(
        "{} sidecar lifecycle is not configured on this node",
        sidecar_name(sidecar)
    )
}

fn sidecar_runtime_component(sidecar: LocalSidecarKind) -> RuntimeComponentKind {
    match sidecar {
        LocalSidecarKind::Hysteria2 => RuntimeComponentKind::Hysteria2,
        LocalSidecarKind::WireGuard => RuntimeComponentKind::WireGuard,
    }
}

fn safe_service_name(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > 96 {
        return None;
    }
    trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '@' | '_' | '-'))
        .then(|| trimmed.to_string())
}

fn safe_interface_name(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > 32 {
        return None;
    }
    trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
        .then(|| trimmed.to_string())
}

fn standard_hysteria2_action_args(
    action: LocalSidecarAction,
    service_name: &str,
) -> Option<Vec<String>> {
    let service_name = safe_service_name(service_name)?;
    match OS {
        "linux" => match action {
            LocalSidecarAction::Start => Some(vec![
                "systemctl".to_string(),
                "start".to_string(),
                service_name,
            ]),
            LocalSidecarAction::Stop => Some(vec![
                "systemctl".to_string(),
                "stop".to_string(),
                service_name,
            ]),
            LocalSidecarAction::Restart => Some(vec![
                "systemctl".to_string(),
                "restart".to_string(),
                service_name,
            ]),
            LocalSidecarAction::Status => Some(vec![
                "systemctl".to_string(),
                "is-active".to_string(),
                service_name,
            ]),
            LocalSidecarAction::Logs => Some(vec![
                "journalctl".to_string(),
                "--no-pager".to_string(),
                "-n".to_string(),
                "80".to_string(),
                "-u".to_string(),
                service_name,
            ]),
            LocalSidecarAction::Install
            | LocalSidecarAction::Update
            | LocalSidecarAction::Validate => None,
        },
        "windows" => {
            let service_name = service_name.trim_end_matches(".service").to_string();
            match action {
                LocalSidecarAction::Start => Some(vec![
                    "sc.exe".to_string(),
                    "start".to_string(),
                    service_name,
                ]),
                LocalSidecarAction::Stop => {
                    Some(vec!["sc.exe".to_string(), "stop".to_string(), service_name])
                }
                LocalSidecarAction::Status => Some(vec![
                    "sc.exe".to_string(),
                    "query".to_string(),
                    service_name,
                ]),
                LocalSidecarAction::Restart
                | LocalSidecarAction::Install
                | LocalSidecarAction::Update
                | LocalSidecarAction::Validate
                | LocalSidecarAction::Logs => None,
            }
        }
        _ => None,
    }
}

fn standard_wireguard_action_args(
    action: LocalSidecarAction,
    wg_binary_path: Option<&str>,
    wg_quick_binary_path: Option<&str>,
    interface_name: &str,
    config_path: Option<&Path>,
) -> Option<Vec<String>> {
    let interface_name = safe_interface_name(interface_name)?;
    let wg = wg_binary_path.filter(|value| !value.trim().is_empty())?;
    let wg_quick = wg_quick_binary_path.filter(|value| !value.trim().is_empty())?;
    match action {
        LocalSidecarAction::Validate => config_path.map(|path| {
            vec![
                wg_quick.to_string(),
                "strip".to_string(),
                path.to_string_lossy().to_string(),
            ]
        }),
        LocalSidecarAction::Start => config_path.map(|path| {
            vec![
                wg_quick.to_string(),
                "up".to_string(),
                path.to_string_lossy().to_string(),
            ]
        }),
        LocalSidecarAction::Stop => config_path.map(|path| {
            vec![
                wg_quick.to_string(),
                "down".to_string(),
                path.to_string_lossy().to_string(),
            ]
        }),
        LocalSidecarAction::Status => {
            Some(vec![wg.to_string(), "show".to_string(), interface_name])
        }
        LocalSidecarAction::Logs if OS == "linux" => Some(vec![
            "journalctl".to_string(),
            "--no-pager".to_string(),
            "-n".to_string(),
            "80".to_string(),
            "-u".to_string(),
            format!("wg-quick@{interface_name}.service"),
        ]),
        LocalSidecarAction::Restart if OS == "linux" => Some(vec![
            "systemctl".to_string(),
            "restart".to_string(),
            format!("wg-quick@{interface_name}.service"),
        ]),
        LocalSidecarAction::Install
        | LocalSidecarAction::Update
        | LocalSidecarAction::Restart
        | LocalSidecarAction::Logs => None,
    }
}

fn panel_runtime_owner_for_component(component: RuntimeComponentKind) -> ProtocolRuntimeOwner {
    match component {
        RuntimeComponentKind::Xray => ProtocolRuntimeOwner::Xray,
        RuntimeComponentKind::Hysteria2 => ProtocolRuntimeOwner::Sidecar,
        RuntimeComponentKind::WireGuard => ProtocolRuntimeOwner::NodeNative,
    }
}

fn panel_runtime_component_name(component: RuntimeComponentKind) -> &'static str {
    match component {
        RuntimeComponentKind::Xray => "xray",
        RuntimeComponentKind::Hysteria2 => "hysteria2_sidecar",
        RuntimeComponentKind::WireGuard => "wireguard_node_native",
    }
}

fn sidecar_runtime_readiness(status: LocalSidecarStatus) -> RuntimeComponentReadiness {
    match status {
        LocalSidecarStatus::Disabled => RuntimeComponentReadiness::Disabled,
        LocalSidecarStatus::Missing => RuntimeComponentReadiness::Missing,
        LocalSidecarStatus::Degraded => RuntimeComponentReadiness::Failed,
        LocalSidecarStatus::Ready | LocalSidecarStatus::Running => RuntimeComponentReadiness::Ready,
        LocalSidecarStatus::Failed => RuntimeComponentReadiness::Failed,
    }
}

fn placeholder_sidecar_lifecycle_response(
    sidecar: LocalSidecarKind,
    action: LocalSidecarAction,
) -> LocalSidecarLifecycleResponse {
    let status = LocalSidecarStatus::Disabled;
    LocalSidecarLifecycleResponse {
        sidecar,
        action,
        status,
        supported: false,
        plan: placeholder_sidecar_command_plan(sidecar, action),
        acceptance: placeholder_sidecar_acceptance_contract(action, status),
        binary_path: None,
        detected_version: None,
        validated_at_unix: None,
        detail: format!(
            "{} sidecar lifecycle is not configured on this node; action {:?} was not executed",
            sidecar_name(sidecar),
            action
        ),
        logs: Vec::new(),
        updated_at_unix: now_unix(),
    }
}

fn sidecar_preflight_lifecycle_response(
    sidecar: LocalSidecarKind,
    action: LocalSidecarAction,
    status: LocalSidecarStatus,
    supported: bool,
    binary_path: Option<String>,
    detected_version: Option<String>,
    detail: String,
) -> LocalSidecarLifecycleResponse {
    let validated_at_unix = matches!(status, LocalSidecarStatus::Ready).then(now_unix);
    LocalSidecarLifecycleResponse {
        sidecar,
        action,
        status,
        supported,
        plan: sidecar_preflight_command_plan(sidecar, action, binary_path.as_deref()),
        acceptance: sidecar_preflight_acceptance_contract(action, status),
        binary_path,
        detected_version,
        validated_at_unix,
        detail,
        logs: Vec::new(),
        updated_at_unix: now_unix(),
    }
}

fn sidecar_lifecycle_contract_for_result(
    runtime: &NodeRuntime,
    sidecar: LocalSidecarKind,
    action: LocalSidecarAction,
) -> LocalSidecarLifecycleResponse {
    match action {
        LocalSidecarAction::Status | LocalSidecarAction::Validate => {
            let state = runtime
                .sidecar_state(sidecar)
                .cloned()
                .unwrap_or_else(|| default_sidecar_state(sidecar));
            sidecar_preflight_lifecycle_response(
                sidecar,
                action,
                state.status,
                state.supported,
                state.binary_path,
                state.detected_version,
                state
                    .last_detail
                    .unwrap_or_else(|| default_sidecar_detail(sidecar)),
            )
        }
        LocalSidecarAction::Install
        | LocalSidecarAction::Update
        | LocalSidecarAction::Start
        | LocalSidecarAction::Stop
        | LocalSidecarAction::Restart
        | LocalSidecarAction::Logs => {
            if let Some(args) = runtime.sidecar_action_args(sidecar, action) {
                LocalSidecarLifecycleResponse {
                    sidecar,
                    action,
                    status: sidecar_success_status(action),
                    supported: true,
                    plan: configured_sidecar_command_plan(sidecar, action, &args),
                    acceptance: configured_sidecar_acceptance_contract(action),
                    binary_path: runtime.sidecar_binary_path(sidecar),
                    detected_version: None,
                    validated_at_unix: None,
                    detail: format!(
                        "{} {:?} has explicit configured executor argv",
                        sidecar_name(sidecar),
                        action
                    ),
                    logs: Vec::new(),
                    updated_at_unix: now_unix(),
                }
            } else {
                placeholder_sidecar_lifecycle_response(sidecar, action)
            }
        }
    }
}

fn placeholder_sidecar_command_plan(
    sidecar: LocalSidecarKind,
    action: LocalSidecarAction,
) -> LocalSidecarCommandPlan {
    LocalSidecarCommandPlan {
        executor_required: false,
        command_id: format!("{}:{:?}:placeholder", sidecar_name(sidecar), action)
            .to_ascii_lowercase(),
        command_kind: format!("{:?}", action).to_ascii_lowercase(),
        dry_run: true,
        steps: vec![
            "no sidecar executor is implemented".to_string(),
            "do not execute shell commands for this placeholder response".to_string(),
            "keep sidecar disabled and report unsupported state".to_string(),
        ],
    }
}

fn sidecar_preflight_command_plan(
    sidecar: LocalSidecarKind,
    action: LocalSidecarAction,
    binary_path: Option<&str>,
) -> LocalSidecarCommandPlan {
    let binary_path = binary_path.unwrap_or("<not-configured>");
    LocalSidecarCommandPlan {
        executor_required: false,
        command_id: format!("{}:{:?}:preflight", sidecar_name(sidecar), action)
            .to_ascii_lowercase(),
        command_kind: format!("{:?}", action).to_ascii_lowercase(),
        dry_run: false,
        steps: vec![
            format!("check configured binary path: {binary_path}"),
            format!("run safe version probe for {}", sidecar_name(sidecar)),
            "do not start, stop, install, update, or modify services".to_string(),
        ],
    }
}

fn configured_sidecar_command_plan(
    sidecar: LocalSidecarKind,
    action: LocalSidecarAction,
    args: &[String],
) -> LocalSidecarCommandPlan {
    LocalSidecarCommandPlan {
        executor_required: false,
        command_id: format!("{}:{:?}:configured", sidecar_name(sidecar), action)
            .to_ascii_lowercase(),
        command_kind: format!("{:?}", action).to_ascii_lowercase(),
        dry_run: false,
        steps: vec![
            "execute argv from explicit operator configuration or Hydra standard OS recipe"
                .to_string(),
            format!(
                "program: {}",
                args.first().map(String::as_str).unwrap_or("")
            ),
            format!("argument_count: {}", args.len().saturating_sub(1)),
            "capture bounded stdout/stderr without shell interpolation".to_string(),
        ],
    }
}

fn placeholder_sidecar_acceptance_contract(
    action: LocalSidecarAction,
    expected_status: LocalSidecarStatus,
) -> LocalSidecarAcceptanceContract {
    LocalSidecarAcceptanceContract {
        expected_status,
        required_checks: vec![
            format!("{:?} action did not execute external commands", action),
            "supported=false".to_string(),
            "status remains disabled".to_string(),
        ],
        fail_closed: true,
        timeout_seconds: 0,
    }
}

fn configured_sidecar_acceptance_contract(
    action: LocalSidecarAction,
) -> LocalSidecarAcceptanceContract {
    LocalSidecarAcceptanceContract {
        expected_status: sidecar_success_status(action),
        required_checks: vec![
            "argv is configured explicitly or derived from a standard Hydra OS recipe".to_string(),
            "command exits with status 0".to_string(),
            "stdout/stderr are bounded before storing".to_string(),
        ],
        fail_closed: true,
        timeout_seconds: 30,
    }
}

fn sidecar_success_status(action: LocalSidecarAction) -> LocalSidecarStatus {
    match action {
        LocalSidecarAction::Install
        | LocalSidecarAction::Update
        | LocalSidecarAction::Validate
        | LocalSidecarAction::Status
        | LocalSidecarAction::Logs => LocalSidecarStatus::Ready,
        LocalSidecarAction::Start | LocalSidecarAction::Restart => LocalSidecarStatus::Running,
        LocalSidecarAction::Stop => LocalSidecarStatus::Ready,
    }
}

fn bounded_output(bytes: &[u8]) -> String {
    const MAX_SIDECAR_OUTPUT_BYTES: usize = 4096;
    let bounded = if bytes.len() > MAX_SIDECAR_OUTPUT_BYTES {
        &bytes[..MAX_SIDECAR_OUTPUT_BYTES]
    } else {
        bytes
    };
    let mut value = String::from_utf8_lossy(bounded).trim().to_string();
    if bytes.len() > MAX_SIDECAR_OUTPUT_BYTES {
        value.push_str("...<truncated>");
    }
    value
}

fn sidecar_preflight_acceptance_contract(
    _action: LocalSidecarAction,
    expected_status: LocalSidecarStatus,
) -> LocalSidecarAcceptanceContract {
    LocalSidecarAcceptanceContract {
        expected_status,
        required_checks: vec![
            "binary path is configured".to_string(),
            "binary file exists".to_string(),
            "safe version probe exits successfully".to_string(),
        ],
        fail_closed: true,
        timeout_seconds: 10,
    }
}

fn detect_sidecar_version(sidecar: LocalSidecarKind, binary_path: &Path) -> Result<String> {
    let args: &[&str] = match sidecar {
        LocalSidecarKind::Hysteria2 => &["version"],
        LocalSidecarKind::WireGuard => &["--version"],
    };
    let output = std::process::Command::new(binary_path)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("failed to execute {}", binary_path.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        bail!(
            "{} version command failed{}",
            sidecar_name(sidecar),
            if stderr.is_empty() {
                String::new()
            } else {
                format!(": {stderr}")
            }
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let first_line = stdout
        .lines()
        .chain(stderr.lines())
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("version output was empty");
    Ok(first_line.to_string())
}

fn sidecar_name(sidecar: LocalSidecarKind) -> &'static str {
    match sidecar {
        LocalSidecarKind::Hysteria2 => "hysteria2",
        LocalSidecarKind::WireGuard => "wireguard",
    }
}

fn detect_xray_version_from_binary(binary_path: &Path) -> Result<String> {
    let output = std::process::Command::new(binary_path)
        .arg("version")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("failed to execute {}", binary_path.display()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        bail!(
            "xray version command failed{}",
            if stderr.is_empty() {
                String::new()
            } else {
                format!(": {stderr}")
            }
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_line = stdout.lines().next().unwrap_or_default().trim();
    if let Some(version) = first_line.strip_prefix("Xray ") {
        Ok(version.trim().to_string())
    } else {
        Ok(first_line.to_string())
    }
}

fn calculate_restart_backoff_seconds(attempts: u32, base_seconds: u64, max_seconds: u64) -> u64 {
    let exponent = attempts.saturating_sub(1).min(10);
    let multiplier = 1_u64.checked_shl(exponent).unwrap_or(u64::MAX);
    base_seconds
        .saturating_mul(multiplier)
        .min(max_seconds.max(base_seconds))
}

fn backup_config_if_exists(config_path: &Path) -> Result<Option<PathBuf>> {
    if !config_path.is_file() {
        return Ok(None);
    }

    let backup_path = backup_path(config_path);
    ensure_parent_dir(&backup_path)?;
    fs::copy(config_path, &backup_path).with_context(|| {
        format!(
            "failed to create config backup from {} to {}",
            config_path.display(),
            backup_path.display()
        )
    })?;
    Ok(Some(backup_path))
}

fn write_rollback_marker(
    config_path: &Path,
    revision: &str,
    detail: &str,
    backup_path: Option<&PathBuf>,
) -> Result<PathBuf> {
    let marker_path = rollback_marker_path(config_path);
    ensure_parent_dir(&marker_path)?;
    let temp_path = temp_path(&marker_path);
    let data = serde_json::to_vec_pretty(&serde_json::json!({
        "revision": revision,
        "detail": detail,
        "created_at_unix": now_unix(),
        "backup_path": backup_path.map(|path| path.display().to_string()),
    }))
    .context("failed to serialize rollback marker")?;
    fs::write(&temp_path, data)
        .with_context(|| format!("failed to write {}", temp_path.display()))?;
    fs::rename(&temp_path, &marker_path).with_context(|| {
        format!(
            "failed to move {} to {}",
            temp_path.display(),
            marker_path.display()
        )
    })?;
    Ok(marker_path)
}

fn clear_rollback_marker(config_path: &Path) -> Result<()> {
    let marker_path = rollback_marker_path(config_path);
    if marker_path.is_file() {
        fs::remove_file(&marker_path)
            .with_context(|| format!("failed to remove {}", marker_path.display()))?;
    }
    Ok(())
}

/// Reads persisted JSON, distinguishing three cases.
///
/// - file absent -> `Ok(None)`, a normal first run;
/// - file unreadable -> `Err`, an I/O failure;
/// - file does not parse -> `Err`, **never** a default.
///
/// All three used to collapse into an empty struct: a corrupt state file quietly
/// became "the node has applied nothing", so the agent re-applied configuration
/// from scratch believing itself clean.
///
/// Returning `Result` leaves the decision to the caller. Today every call comes
/// from startup and chooses to refuse starting; runtime readers already pass the
/// error upward.
fn read_persisted_json<T: serde::de::DeserializeOwned>(path: &str) -> Result<Option<T>> {
    let data = match fs::read(path) {
        Ok(data) => data,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(anyhow::anyhow!("cannot read {path}: {error}"));
        }
    };
    serde_json::from_slice::<T>(&data)
        .map(Some)
        .map_err(|error| {
            anyhow::anyhow!(
                "{path} is corrupt or incompatible: {error}. It will not be replaced \
             with an empty file — repair or remove it deliberately"
            )
        })
}

fn load_state(path: &str) -> Result<PersistedNodeState> {
    Ok(read_persisted_json::<PersistedNodeState>(path)?.unwrap_or_default())
}

fn persist_state(path: &str, state: &PersistedNodeState) -> Result<()> {
    persist_json_pretty_atomic(
        path,
        state,
        "failed to serialize node state",
        Durability::Fsync,
    )
}

fn persist_generated_config(path: &str, config: &GeneratedCoreConfig) -> Result<()> {
    persist_json_pretty_atomic(
        path,
        config,
        "failed to serialize generated config",
        Durability::Fsync,
    )
}

fn persist_node_runtime_config(path: &str, config: &NodeRuntimeConfigDocument) -> Result<()> {
    persist_json_pretty_atomic(
        path,
        config,
        "failed to serialize node runtime config",
        Durability::Fsync,
    )
}

fn persist_sidecar_runtime_config(path: &str, config: &SidecarRuntimeConfigDocument) -> Result<()> {
    persist_json_pretty_atomic(
        path,
        config,
        "failed to serialize sidecar runtime config",
        Durability::Fsync,
    )
}

fn persist_sidecar_generated_config_files(
    sidecar_runtime_config_path: &str,
    config: &SidecarRuntimeConfigDocument,
) -> Result<usize> {
    let base_dir = sidecar_generated_config_dir(sidecar_runtime_config_path);
    let hysteria_dir = base_dir.join("hysteria2");
    let wireguard_dir = base_dir.join("wireguard");
    fs::create_dir_all(&hysteria_dir)
        .with_context(|| format!("failed to create {}", hysteria_dir.display()))?;
    fs::create_dir_all(&wireguard_dir)
        .with_context(|| format!("failed to create {}", wireguard_dir.display()))?;
    secure_directory_permissions(&hysteria_dir)?;
    secure_directory_permissions(&wireguard_dir)?;
    let mut written = 0usize;
    for hysteria in &config.hysteria2_configs {
        let path = hysteria_dir.join(format!("{}.yaml", safe_credential_file_stem(&hysteria.tag)));
        write_secret_file_if_changed(
            &path,
            render_hysteria2_candidate_config(hysteria).as_bytes(),
        )?;
        written += 1;
    }
    for wireguard in &config.wireguard_configs {
        let path = wireguard_dir.join(format!(
            "{}.conf",
            safe_credential_file_stem(&wireguard.tag)
        ));
        write_secret_file_if_changed(
            &path,
            render_wireguard_candidate_config(wireguard).as_bytes(),
        )?;
        written += 1;
    }
    Ok(written)
}

fn persist_wireguard_session_mapping(
    sidecar_runtime_config_path: &str,
    mapping: &WireGuardSessionMappingDocument,
) -> Result<bool> {
    let path = wireguard_session_mapping_path(sidecar_runtime_config_path);
    let data = serde_json::to_vec_pretty(mapping)
        .context("failed to serialize WireGuard session mapping")?;
    write_secret_file_if_changed(&path, &data)
}

fn persist_xray_render_plan(path: &str, plan: &XrayRenderPlan) -> Result<()> {
    persist_json_pretty_atomic(
        path,
        &plan.config,
        "failed to serialize xray config",
        Durability::Fsync,
    )
}

fn install_route_credentials(
    credentials_dir: &str,
    manifest_path: &str,
    bundle: &NodeRouteCredentialBundle,
) -> Result<usize> {
    let base_dir = PathBuf::from(credentials_dir);
    fs::create_dir_all(&base_dir)
        .with_context(|| format!("failed to create {}", base_dir.display()))?;
    secure_directory_permissions(&base_dir)?;

    let mut credentials = Vec::new();
    let mut changed_count = 0usize;
    for material in &bundle.credentials {
        if material.kind != "mutual_tls" {
            continue;
        }
        let stem = safe_credential_file_stem(&material.credential_ref);
        let cert_path = base_dir.join(format!("{stem}.crt"));
        let key_path = base_dir.join(format!("{stem}.key"));
        let ca_path = base_dir.join(format!("{stem}.ca.crt"));

        let cert_changed =
            write_secret_file_if_changed(&cert_path, material.certificate_pem.as_bytes())?;
        let key_changed =
            write_secret_file_if_changed(&key_path, material.private_key_pem.as_bytes())?;
        let ca_changed =
            write_secret_file_if_changed(&ca_path, material.ca_certificate_pem.as_bytes())?;
        let changed = cert_changed || key_changed || ca_changed;

        credentials.push(RouteCredential {
            credential_ref: material.credential_ref.clone(),
            kind: material.kind.clone(),
            certificate_file: Some(cert_path.to_string_lossy().to_string()),
            private_key_file: Some(key_path.to_string_lossy().to_string()),
            ca_certificate_file: Some(ca_path.to_string_lossy().to_string()),
            public_key: None,
            server_name: material.server_name.clone(),
            short_id: None,
            certificate_pins: material.certificate_pins.clone(),
        });
        if changed {
            changed_count += 1;
        }
    }

    let store = RouteCredentialStore {
        credentials,
        reality_materials: bundle.reality_materials.clone(),
    };
    let manifest_changed = persist_route_credential_manifest_if_changed(manifest_path, &store)?;
    Ok(changed_count + usize::from(manifest_changed))
}

fn persist_route_credential_manifest_if_changed(
    path: &str,
    store: &RouteCredentialStore,
) -> Result<bool> {
    let path = PathBuf::from(path);
    ensure_parent_dir(&path)?;
    let data = serde_json::to_vec_pretty(store)
        .context("failed to serialize route credential manifest")?;
    write_secret_file_if_changed(&path, &data)
}

fn write_secret_file_if_changed(path: &Path, bytes: &[u8]) -> Result<bool> {
    if path.exists() {
        let existing =
            fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
        if existing == bytes {
            secure_file_permissions(path)?;
            return Ok(false);
        }
    }
    ensure_parent_dir(path)?;
    let temp_path = temp_path(path);
    // The mode is set at creation, not chmod'ed afterwards: `fs::write` creates by
    // umask (usually 0644), and in the window before chmod the secret material —
    // WireGuard keys, hysteria2 configs, the credential manifest — is readable by
    // anyone with access to the directory.
    replace_file_durably(&temp_path, path, bytes)?;
    secure_file_permissions(path)?;
    Ok(true)
}

fn safe_credential_file_stem(credential_ref: &str) -> String {
    credential_ref
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(unix)]
/// Creates a file with mode 0600 up front and writes to it.
fn write_secret_temp_file(path: &Path, bytes: &[u8]) -> Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write as _;
        use std::os::unix::fs::OpenOptionsExt;

        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .with_context(|| format!("failed to create {}", path.display()))?;
        file.write_all(bytes)
            .with_context(|| format!("failed to write {}", path.display()))?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        fs::write(path, bytes).with_context(|| format!("failed to write {}", path.display()))
    }
}

/// Durable file replacement: temp, fsync(temp), rename, fsync(directory).
///
/// All four steps matter: without the temp fsync the rename can reach the journal
/// before the data, and without the directory fsync the rename itself is not
/// durable. On ext4 `data=ordered` usually hides both, but that is a mount option,
/// not a POSIX guarantee.
///
/// Applied to files that are unrecoverable or carry secret material: runtime
/// config, the credential manifest, generated sidecar configs, the WireGuard
/// session map. Bounded telemetry — apply history and runtime events — goes
/// without fsync: it already drops old records, and an fsync per event at 1 vCPU
/// costs more than the last few records are worth.
fn replace_file_durably(temp_path: &Path, path: &Path, bytes: &[u8]) -> Result<()> {
    write_secret_temp_file(temp_path, bytes)?;
    fs::File::open(temp_path)
        .and_then(|file| file.sync_all())
        .with_context(|| format!("failed to fsync {}", temp_path.display()))?;
    fs::rename(temp_path, path).with_context(|| {
        format!(
            "failed to move {} to {}",
            temp_path.display(),
            path.display()
        )
    })?;
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::File::open(parent)
            .and_then(|dir| dir.sync_all())
            .with_context(|| format!("failed to fsync directory {}", parent.display()))?;
    }
    Ok(())
}

fn secure_file_permissions(path: &Path) -> Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("failed to chmod 0600 {}", path.display()))
}

#[cfg(not(unix))]
fn secure_file_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn secure_directory_permissions(path: &Path) -> Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .with_context(|| format!("failed to chmod 0700 {}", path.display()))
}

#[cfg(not(unix))]
fn secure_directory_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

fn load_route_credentials(path: &Path) -> Result<RouteCredentialStore> {
    if !path.exists() {
        return Ok(RouteCredentialStore::default());
    }
    warn_if_secret_manifest_permissions_are_loose(path);
    let data = fs::read_to_string(path).with_context(|| {
        format!(
            "failed to read route credential manifest {}",
            path.display()
        )
    })?;
    let store = serde_json::from_str::<RouteCredentialStore>(&data).with_context(|| {
        format!(
            "failed to parse route credential manifest {}",
            path.display()
        )
    })?;
    Ok(store)
}

#[cfg(unix)]
fn warn_if_secret_manifest_permissions_are_loose(path: &Path) {
    let Ok(metadata) = fs::metadata(path) else {
        return;
    };
    let mode = metadata.permissions().mode();
    if mode & 0o077 != 0 {
        warn!(
            path = %path.display(),
            "route credential manifest is readable by group/other; recommended mode is 0600"
        );
    }
}

#[cfg(not(unix))]
fn warn_if_secret_manifest_permissions_are_loose(_path: &Path) {}

fn build_node_runtime_config_document(
    response: &NodeAgentConfigResponse,
    node_id: &Option<String>,
    cluster_intents: &[ClusterRuntimeIntent],
    route_assignments: &[NodeRouteAssignment],
) -> NodeRuntimeConfigDocument {
    let required_protocols = collect_runtime_protocol_requirements(response, route_assignments);
    NodeRuntimeConfigDocument {
        schema_version: 1,
        node_id: node_id.clone(),
        source_revision: response.revision.clone(),
        source_generated_at_unix: response.generated_config.generated_at_unix,
        created_at_unix: now_unix(),
        source_user_count: response.generated_config.users.len(),
        source_node_count: response.generated_config.nodes.len(),
        users: response
            .generated_config
            .users
            .iter()
            .map(|user| NodeRuntimeUserConfig {
                username: user.username.clone(),
                proxy_profiles: user.proxy_profiles.clone(),
                inbounds: user.inbounds.clone(),
            })
            .collect(),
        inbounds: response.generated_config.inbounds.clone(),
        hosts: response.generated_config.hosts.clone(),
        cluster_intents: cluster_intents.to_vec(),
        route_assignments: route_assignments.to_vec(),
        required_protocols,
    }
}

#[cfg(test)]
fn build_sidecar_runtime_config_document(
    runtime_config: &NodeRuntimeConfigDocument,
) -> SidecarRuntimeConfigDocument {
    build_sidecar_runtime_config_document_with_stats(
        runtime_config,
        19_090,
        b"hydra-test-runtime-stats-key",
    )
}

fn build_sidecar_runtime_config_document_with_stats(
    runtime_config: &NodeRuntimeConfigDocument,
    hysteria2_traffic_stats_base_port: u16,
    stats_secret_key: &[u8],
) -> SidecarRuntimeConfigDocument {
    let requirements = runtime_config
        .required_protocols
        .iter()
        .filter_map(|requirement| {
            let sidecar = sidecar_kind_for_component(requirement.required_component)?;
            Some(sidecar_runtime_requirement(requirement, sidecar))
        })
        .collect();

    SidecarRuntimeConfigDocument {
        schema_version: 1,
        source_revision: runtime_config.source_revision.clone(),
        created_at_unix: now_unix(),
        requirements,
        hysteria2_configs: render_hysteria2_runtime_configs(
            runtime_config,
            hysteria2_traffic_stats_base_port,
            stats_secret_key,
        ),
        wireguard_configs: render_wireguard_runtime_configs(runtime_config),
    }
}

fn sidecar_runtime_requirement(
    requirement: &RuntimeProtocolRequirement,
    sidecar: LocalSidecarKind,
) -> SidecarRuntimeRequirement {
    sidecar_runtime_requirement_with_reason(
        requirement,
        sidecar,
        "sidecar protocol is blocked until generated config exists, component preflight is ready, and a matching executor session is accepted".to_string(),
    )
}

fn sidecar_runtime_requirement_with_reason(
    requirement: &RuntimeProtocolRequirement,
    sidecar: LocalSidecarKind,
    reason: String,
) -> SidecarRuntimeRequirement {
    SidecarRuntimeRequirement {
        sidecar,
        protocol: requirement.protocol,
        source: requirement.source.clone(),
        source_ref: requirement.source_ref.clone(),
        status: SidecarRuntimeRequirementStatus::Blocked,
        reason,
        planned_envelopes: sidecar_runtime_executor_envelopes(
            sidecar,
            requirement.protocol,
            &requirement.source_ref,
        ),
    }
}

fn render_hysteria2_runtime_configs(
    runtime_config: &NodeRuntimeConfigDocument,
    traffic_stats_base_port: u16,
    stats_secret_key: &[u8],
) -> Vec<Hysteria2RuntimeConfig> {
    runtime_config
        .inbounds
        .iter()
        .filter(|inbound| {
            classify_runtime_protocol(&inbound.protocol).is_some_and(|(protocol, component)| {
                protocol == RuntimeProtocolKind::Hysteria2
                    && component == RuntimeComponentKind::Hysteria2
            })
        })
        .enumerate()
        .filter_map(|(index, inbound)| {
            let stats_port = traffic_stats_base_port.checked_add(u16::try_from(index).ok()?)?;
            hysteria2_runtime_config_for_inbound_with_stats(
                runtime_config,
                inbound,
                stats_port,
                stats_secret_key,
            )
        })
        .collect()
}

fn hysteria2_runtime_config_for_inbound(
    runtime_config: &NodeRuntimeConfigDocument,
    inbound: &GeneratedInbound,
) -> Option<Hysteria2RuntimeConfig> {
    hysteria2_runtime_config_for_inbound_with_stats(
        runtime_config,
        inbound,
        19_090,
        b"hydra-test-runtime-stats-key",
    )
}

fn hysteria2_runtime_config_for_inbound_with_stats(
    runtime_config: &NodeRuntimeConfigDocument,
    inbound: &GeneratedInbound,
    traffic_stats_port: u16,
    stats_secret_key: &[u8],
) -> Option<Hysteria2RuntimeConfig> {
    let profiles =
        generated_profiles_for_inbound(runtime_config, inbound, RuntimeProtocolKind::Hysteria2);
    let mut auth_users = runtime_config
        .users
        .iter()
        .filter(|user| user_allows_inbound(user, &inbound.tag))
        .flat_map(|user| {
            user.proxy_profiles.iter().filter_map(|profile| {
                if !profile_matches_inbound(profile, &inbound.tag, RuntimeProtocolKind::Hysteria2) {
                    return None;
                }
                let settings = parse_profile_settings(profile)?;
                let password = string_setting(&settings, &["password", "auth", "auth_password"])?;
                Some(Hysteria2RuntimeUser {
                    runtime_username: user.username.clone(),
                    password,
                })
            })
        })
        .collect::<Vec<_>>();
    auth_users.sort_by(|left, right| left.runtime_username.cmp(&right.runtime_username));
    let unique_users = auth_users
        .iter()
        .map(|user| user.runtime_username.as_str())
        .collect::<BTreeSet<_>>();
    if unique_users.len() != auth_users.len() {
        return None;
    }
    let tls = profiles.iter().find_map(|profile| {
        let settings = parse_profile_settings(profile)?;
        let certificate_file =
            string_setting(&settings, &["tls_certificate_file", "certificate_file"])?;
        let key_file = string_setting(&settings, &["tls_key_file", "key_file"])?;
        (path_is_nonempty_file(&certificate_file) && path_is_nonempty_file(&key_file))
            .then_some((certificate_file, key_file))
    })?;
    (!auth_users.is_empty()).then_some(Hysteria2RuntimeConfig {
        tag: inbound.tag.clone(),
        listen: "0.0.0.0".to_string(),
        port: inbound.port,
        auth_users,
        traffic_stats_listen: format!("127.0.0.1:{traffic_stats_port}"),
        traffic_stats_secret: hysteria2_stats_secret(stats_secret_key, &inbound.tag),
        certificate_file: tls.0,
        key_file: tls.1,
    })
}

fn render_wireguard_runtime_configs(
    runtime_config: &NodeRuntimeConfigDocument,
) -> Vec<WireGuardRuntimeConfig> {
    runtime_config
        .inbounds
        .iter()
        .filter(|inbound| {
            classify_runtime_protocol(&inbound.protocol).is_some_and(|(protocol, component)| {
                protocol == RuntimeProtocolKind::WireGuard
                    && component == RuntimeComponentKind::WireGuard
            })
        })
        .filter_map(|inbound| wireguard_runtime_config_for_inbound(runtime_config, inbound))
        .collect()
}

fn wireguard_runtime_config_for_inbound(
    runtime_config: &NodeRuntimeConfigDocument,
    inbound: &GeneratedInbound,
) -> Option<WireGuardRuntimeConfig> {
    let interface_settings = runtime_config
        .users
        .iter()
        .filter(|user| user_allows_inbound(user, &inbound.tag))
        .flat_map(|user| user.proxy_profiles.iter())
        .filter(|profile| {
            profile_matches_inbound(profile, &inbound.tag, RuntimeProtocolKind::WireGuard)
        })
        .find_map(parse_profile_settings)?;
    let mut peers = runtime_config
        .users
        .iter()
        .filter(|user| user_allows_inbound(user, &inbound.tag))
        .flat_map(|user| {
            user.proxy_profiles.iter().filter_map(|profile| {
                if !profile_matches_inbound(profile, &inbound.tag, RuntimeProtocolKind::WireGuard) {
                    return None;
                }
                let settings = parse_profile_settings(profile)?;
                let public_key = string_setting(&settings, &["peer_public_key", "public_key"])?;
                if !is_wireguard_key(&public_key) {
                    return None;
                }
                let allowed_ips = string_array_setting(&settings, "allowed_ips")
                    .filter(|values| !values.is_empty())?;
                let device_fingerprint = string_setting(&settings, &["device_fingerprint"])
                    .unwrap_or_else(|| wireguard_device_fingerprint(&public_key));
                if device_fingerprint.len() > 256 {
                    return None;
                }
                Some(WireGuardRuntimePeer {
                    runtime_username: user.username.clone(),
                    public_key,
                    endpoint: string_setting(&settings, &["peer_endpoint", "endpoint"]),
                    allowed_ips,
                    device_fingerprint,
                })
            })
        })
        .collect::<Vec<_>>();
    peers.sort_by(|left, right| left.public_key.cmp(&right.public_key));
    let unique_public_keys = peers
        .iter()
        .map(|peer| peer.public_key.as_str())
        .collect::<BTreeSet<_>>();
    let unique_allowed_ips = peers
        .iter()
        .flat_map(|peer| peer.allowed_ips.iter().map(String::as_str))
        .collect::<BTreeSet<_>>();
    if unique_public_keys.len() != peers.len()
        || unique_allowed_ips.len()
            != peers
                .iter()
                .map(|peer| peer.allowed_ips.len())
                .sum::<usize>()
    {
        return None;
    }
    (!peers.is_empty()).then_some(WireGuardRuntimeConfig {
        tag: inbound.tag.clone(),
        interface_private_key: string_setting(
            &interface_settings,
            &["private_key", "interface_private_key"],
        )?,
        interface_address: string_setting(&interface_settings, &["address", "interface_address"])?,
        listen_port: interface_settings
            .get("listen_port")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| u16::try_from(value).ok())
            .or(Some(inbound.port)),
        peers,
    })
}

fn sidecar_runtime_config_payload_exists(
    runtime_config: &NodeRuntimeConfigDocument,
    requirement: &RuntimeProtocolRequirement,
) -> bool {
    match requirement.protocol {
        RuntimeProtocolKind::Hysteria2 => runtime_config
            .inbounds
            .iter()
            .find(|inbound| inbound.tag == requirement.source_ref)
            .and_then(|inbound| hysteria2_runtime_config_for_inbound(runtime_config, inbound))
            .is_some(),
        RuntimeProtocolKind::WireGuard => runtime_config
            .inbounds
            .iter()
            .find(|inbound| inbound.tag == requirement.source_ref)
            .and_then(|inbound| wireguard_runtime_config_for_inbound(runtime_config, inbound))
            .is_some(),
        _ => false,
    }
}

fn render_hysteria2_candidate_config(config: &Hysteria2RuntimeConfig) -> String {
    let mut output = String::new();
    output.push_str(&format!("listen: :{}\n", config.port));
    output.push_str("tls:\n");
    output.push_str(&format!("  cert: {}\n", config.certificate_file));
    output.push_str(&format!("  key: {}\n", config.key_file));
    output.push_str("auth:\n");
    output.push_str("  type: userpass\n");
    output.push_str("  userpass:\n");
    for user in &config.auth_users {
        output.push_str(&format!(
            "    {}: {}\n",
            yaml_quoted(&user.runtime_username),
            yaml_quoted(&user.password)
        ));
    }
    output.push_str("trafficStats:\n");
    output.push_str(&format!(
        "  listen: {}\n",
        yaml_quoted(&config.traffic_stats_listen)
    ));
    output.push_str(&format!(
        "  secret: {}\n",
        yaml_quoted(&config.traffic_stats_secret)
    ));
    output
}

fn yaml_quoted(value: &str) -> String {
    serde_json::to_string(value).expect("serializing a string cannot fail")
}

fn hysteria2_stats_secret(secret_key: &[u8], tag: &str) -> String {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret_key).expect("HMAC-SHA256 accepts keys of any length");
    mac.update(b"hydra-hysteria2-traffic-stats-v1\0");
    mac.update(tag.as_bytes());
    hex_bytes(&mac.finalize().into_bytes())
}

fn render_wireguard_candidate_config(config: &WireGuardRuntimeConfig) -> String {
    let mut output = String::new();
    output.push_str("[Interface]\n");
    output.push_str(&format!("PrivateKey = {}\n", config.interface_private_key));
    output.push_str(&format!("Address = {}\n", config.interface_address));
    if let Some(port) = config.listen_port {
        output.push_str(&format!("ListenPort = {port}\n"));
    }
    for peer in &config.peers {
        output.push_str("\n[Peer]\n");
        output.push_str(&format!("PublicKey = {}\n", peer.public_key));
        if let Some(endpoint) = peer.endpoint.as_ref() {
            output.push_str(&format!("Endpoint = {endpoint}\n"));
        }
        output.push_str(&format!("AllowedIPs = {}\n", peer.allowed_ips.join(", ")));
    }
    output.push('\n');
    output
}

fn wireguard_device_fingerprint(public_key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"hydra-wireguard-device-v1\0");
    hasher.update(public_key.as_bytes());
    let digest = hasher.finalize();
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(encoded, "{byte:02x}");
    }
    format!("wireguard-sha256:{encoded}")
}

fn is_wireguard_key(value: &str) -> bool {
    value.len() == 44
        && value.ends_with('=')
        && value[..43].chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '+' || character == '/'
        })
}

fn summarize_sidecar_runtime_config(
    config: &SidecarRuntimeConfigDocument,
) -> SidecarRuntimeSummary {
    SidecarRuntimeSummary {
        schema_version: config.schema_version,
        source_revision: config.source_revision.clone(),
        requirement_count: config.requirements.len(),
        blocked_count: config
            .requirements
            .iter()
            .filter(|requirement| requirement.status == SidecarRuntimeRequirementStatus::Blocked)
            .count(),
        created_at_unix: config.created_at_unix,
    }
}

fn build_wireguard_session_mapping(
    config: &SidecarRuntimeConfigDocument,
) -> Result<WireGuardSessionMappingDocument> {
    let mut peer_owners = HashMap::<(String, String), String>::new();
    let mut interface_names = BTreeSet::new();
    let mut interfaces = Vec::new();
    for runtime_config in &config.wireguard_configs {
        let interface_name = safe_credential_file_stem(&runtime_config.tag);
        if interface_name.is_empty() || interface_name.len() > 15 || interface_name.starts_with('-')
        {
            bail!("WireGuard config tag cannot form a safe Linux interface name");
        }
        if !interface_names.insert(interface_name.clone()) {
            bail!("WireGuard config tags resolve to a duplicate interface name");
        }
        let mut peers = Vec::new();
        for peer in &runtime_config.peers {
            if let Some(existing_owner) = peer_owners.insert(
                (interface_name.clone(), peer.public_key.clone()),
                peer.runtime_username.clone(),
            ) && existing_owner != peer.runtime_username
            {
                bail!("WireGuard peer public key is assigned to multiple runtime principals");
            }
            peers.push(WireGuardSessionPeerMapping {
                runtime_username: peer.runtime_username.clone(),
                public_key: peer.public_key.clone(),
                device_fingerprint: peer.device_fingerprint.clone(),
            });
        }
        peers.sort_by(|left, right| left.public_key.cmp(&right.public_key));
        peers.dedup_by(|left, right| left.public_key == right.public_key);
        interfaces.push(WireGuardSessionInterfaceMapping {
            interface_name,
            peers,
        });
    }
    interfaces.sort_by(|left, right| left.interface_name.cmp(&right.interface_name));
    Ok(WireGuardSessionMappingDocument {
        schema_version: 1,
        source_revision: config.source_revision.clone(),
        created_at_unix: config.created_at_unix,
        interfaces,
    })
}

fn sidecar_runtime_executor_envelopes(
    sidecar: LocalSidecarKind,
    protocol: RuntimeProtocolKind,
    source_ref: &str,
) -> Vec<SidecarRuntimeExecutorEnvelope> {
    [
        LocalSidecarAction::Validate,
        LocalSidecarAction::Start,
        LocalSidecarAction::Status,
    ]
    .into_iter()
    .map(|action| {
        let mut response = placeholder_sidecar_lifecycle_response(sidecar, action);
        response.plan.command_id = sidecar_envelope_command_id(sidecar, source_ref, action);
        SidecarRuntimeExecutorEnvelope {
            sidecar,
            action,
            command_id: response.plan.command_id.clone(),
            config_path: None,
            config_exists: false,
            plan: response.plan,
            acceptance: response.acceptance,
            reason: format!(
                "{:?} requirement {} is blocked until {} {:?} executor is implemented",
                protocol,
                source_ref,
                sidecar_name(sidecar),
                action
            ),
        }
    })
    .collect()
}

fn safe_command_ref(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn sidecar_envelope_command_id(
    sidecar: LocalSidecarKind,
    source_ref: &str,
    action: LocalSidecarAction,
) -> String {
    format!(
        "{}:{}:{:?}:placeholder",
        sidecar_name(sidecar),
        safe_command_ref(source_ref),
        action
    )
    .to_ascii_lowercase()
}

fn build_sidecar_executor_session(
    summary: Option<&SidecarRuntimeSummary>,
    requirements: Vec<SidecarRuntimeRequirement>,
) -> SidecarExecutorSession {
    let envelopes = requirements
        .iter()
        .flat_map(|requirement| requirement.planned_envelopes.iter().cloned())
        .collect::<Vec<_>>();
    let required_command_ids = envelopes
        .iter()
        .map(|envelope| envelope.command_id.clone())
        .collect::<Vec<_>>();
    let envelope_count = envelopes.len();
    let executable = envelope_count > 0
        && envelopes.iter().all(|envelope| {
            !envelope.plan.dry_run
                && envelope.config_exists
                && matches!(
                    envelope.acceptance.expected_status,
                    LocalSidecarStatus::Ready | LocalSidecarStatus::Running
                )
        });
    let source_revision = summary.map(|summary| summary.source_revision.clone());
    let session_id = sidecar_executor_session_id(source_revision.as_deref(), &required_command_ids);
    let detail = if envelope_count == 0 {
        "no sidecar executor work is planned for current runtime state".to_string()
    } else if executable {
        format!("{envelope_count} sidecar executor envelope(s) are executable from explicit argv")
    } else {
        format!(
            "{envelope_count} sidecar executor envelope(s) planned; session remains fail-closed until explicit argv or standard OS recipes are configured"
        )
    };
    SidecarExecutorSession {
        schema_version: 1,
        session_id,
        source_revision,
        created_at_unix: now_unix(),
        requirement_count: requirements.len(),
        envelope_count,
        executable,
        fail_closed: true,
        acceptance: SidecarExecutorSessionAcceptance {
            required_envelope_count: envelope_count,
            required_command_ids,
            fail_closed: true,
            timeout_seconds: envelopes
                .iter()
                .map(|envelope| envelope.acceptance.timeout_seconds)
                .max()
                .unwrap_or(0),
        },
        requirements,
        envelopes,
        detail,
    }
}

impl SidecarExecutorSession {
    fn summary(&self) -> SidecarExecutorSessionSummary {
        SidecarExecutorSessionSummary {
            session_id: self.session_id.clone(),
            source_revision: self.source_revision.clone(),
            requirement_count: self.requirement_count,
            envelope_count: self.envelope_count,
            executable: self.executable,
            fail_closed: self.fail_closed,
            detail: self.detail.clone(),
        }
    }
}

fn sidecar_executor_session_id(source_revision: Option<&str>, command_ids: &[String]) -> String {
    let source = source_revision.unwrap_or("none");
    format!(
        "sidecar-session:{}:{}:{}",
        source,
        command_ids.len(),
        command_ids.join(",")
    )
}

fn validate_sidecar_executor_session_result(
    session: &SidecarExecutorSession,
    result: &SidecarExecutorSessionResultRequest,
) -> SidecarExecutorSessionResultResponse {
    let mut failed_checks = Vec::new();
    if result.session_id != session.session_id {
        failed_checks.push(format!(
            "session_id mismatch: expected {}, got {}",
            session.session_id, result.session_id
        ));
    }
    if result.results.len() != session.acceptance.required_envelope_count {
        failed_checks.push(format!(
            "result count mismatch: expected {}, got {}",
            session.acceptance.required_envelope_count,
            result.results.len()
        ));
    }
    for command_id in &session.acceptance.required_command_ids {
        let matching_count = result
            .results
            .iter()
            .filter(|result| result.command_id == *command_id)
            .count();
        if matching_count == 0 {
            failed_checks.push(format!("missing result for command_id {command_id}"));
        } else if matching_count > 1 {
            failed_checks.push(format!("duplicate result for command_id {command_id}"));
        }
    }
    for result in &result.results {
        let Some(envelope) = session
            .envelopes
            .iter()
            .find(|envelope| envelope.command_id == result.command_id)
        else {
            failed_checks.push(format!("unexpected command_id {}", result.command_id));
            continue;
        };
        validate_sidecar_executor_result_against_envelope(envelope, result, &mut failed_checks);
    }
    let rejected_count = failed_checks.len();
    let accepted = failed_checks.is_empty();
    SidecarExecutorSessionResultResponse {
        session_id: result.session_id.clone(),
        accepted,
        expected_envelope_count: session.acceptance.required_envelope_count,
        accepted_count: if accepted { result.results.len() } else { 0 },
        rejected_count,
        failed_checks,
        detail: if accepted {
            format!(
                "sidecar executor session {} accepted with {} result(s)",
                session.session_id,
                result.results.len()
            )
        } else {
            format!(
                "sidecar executor session {} rejected fail-closed",
                session.session_id
            )
        },
        updated_at_unix: now_unix(),
    }
}

fn validate_sidecar_executor_result_against_envelope(
    envelope: &SidecarRuntimeExecutorEnvelope,
    result: &LocalSidecarExecutorResultRequest,
    failed_checks: &mut Vec<String>,
) {
    if result.status != envelope.acceptance.expected_status {
        failed_checks.push(format!(
            "{} status mismatch: expected {:?}, got {:?}",
            envelope.command_id, envelope.acceptance.expected_status, result.status
        ));
    }
    if result.exit_code.is_some_and(|exit_code| exit_code != 0) {
        failed_checks.push(format!("{} exit_code must be 0", envelope.command_id));
    }
    if !envelope.config_exists {
        failed_checks.push(format!(
            "{} generated sidecar config file is missing",
            envelope.command_id
        ));
    }
    for check in &envelope.acceptance.required_checks {
        if !result
            .completed_checks
            .iter()
            .any(|completed| completed == check)
        {
            failed_checks.push(format!(
                "{} required check missing: {}",
                envelope.command_id, check
            ));
        }
    }
}

fn sidecar_kind_for_component(component: RuntimeComponentKind) -> Option<LocalSidecarKind> {
    match component {
        RuntimeComponentKind::Hysteria2 => Some(LocalSidecarKind::Hysteria2),
        RuntimeComponentKind::WireGuard => Some(LocalSidecarKind::WireGuard),
        RuntimeComponentKind::Xray => None,
    }
}

fn collect_runtime_protocol_requirements(
    response: &NodeAgentConfigResponse,
    route_assignments: &[NodeRouteAssignment],
) -> Vec<RuntimeProtocolRequirement> {
    let mut requirements = BTreeSet::new();
    for inbound in &response.generated_config.inbounds {
        if let Some((protocol, component)) = classify_runtime_protocol(&inbound.protocol) {
            requirements.insert(RuntimeProtocolRequirement {
                protocol,
                required_component: component,
                source: "generated_inbound".to_string(),
                source_ref: inbound.tag.clone(),
            });
        }
    }
    for host in &response.generated_config.hosts {
        if let Some((protocol, component)) = classify_runtime_protocol(&host.security) {
            requirements.insert(RuntimeProtocolRequirement {
                protocol,
                required_component: component,
                source: "generated_host".to_string(),
                source_ref: host.id.clone(),
            });
        }
    }
    for assignment in route_assignments {
        if assignment.listen.is_some() || assignment.next_peer.is_some() {
            requirements.insert(RuntimeProtocolRequirement {
                protocol: RuntimeProtocolKind::VlessTlsWebSocket,
                required_component: RuntimeComponentKind::Xray,
                source: "route_assignment".to_string(),
                source_ref: assignment.route_id.clone(),
            });
        }
    }
    requirements
        .into_iter()
        .take(MAX_RUNTIME_PROTOCOL_REQUIREMENTS)
        .collect()
}

fn classify_runtime_protocol(value: &str) -> Option<(RuntimeProtocolKind, RuntimeComponentKind)> {
    let normalized = value.trim().to_ascii_lowercase().replace(['_', '-'], "");
    match normalized.as_str() {
        "xray" | "vless" | "vlesstls" | "vlesstlswebsocket" | "vlessws" | "vlesswebsocket" => {
            Some((
                RuntimeProtocolKind::VlessTlsWebSocket,
                RuntimeComponentKind::Xray,
            ))
        }
        "hysteria" | "hysteria2" | "hy2" => Some((
            RuntimeProtocolKind::Hysteria2,
            RuntimeComponentKind::Hysteria2,
        )),
        "wireguard" | "wg" => Some((
            RuntimeProtocolKind::WireGuard,
            RuntimeComponentKind::WireGuard,
        )),
        _ => None,
    }
}

#[cfg(test)]
fn render_xray_config(
    runtime_config: &NodeRuntimeConfigDocument,
    route_credentials: &RouteCredentialStore,
    xray_detected_version: Option<String>,
) -> XrayRenderPlan {
    render_xray_config_with_stats(
        runtime_config,
        route_credentials,
        xray_detected_version,
        None,
    )
}

fn render_xray_config_with_stats(
    runtime_config: &NodeRuntimeConfigDocument,
    route_credentials: &RouteCredentialStore,
    xray_detected_version: Option<String>,
    stats_api_address: Option<&str>,
) -> XrayRenderPlan {
    let mut feature_flags = vec![
        "minimal-valid-config".to_string(),
        "blackhole-default-outbound".to_string(),
    ];
    let uses_route_assignments = !runtime_config.route_assignments.is_empty();
    if uses_route_assignments {
        feature_flags.push("least-knowledge-route-assignments".to_string());
        if runtime_config.route_assignments.iter().any(|assignment| {
            route_assignment_requires_unavailable_security(assignment, route_credentials)
        }) {
            feature_flags.push("secure-route-material-pending-fail-closed".to_string());
        } else {
            feature_flags.push("secure-route-material-available".to_string());
        }
    }
    if !runtime_config.required_protocols.is_empty() {
        feature_flags.push(format!(
            "runtime-protocol-requirements:{}",
            runtime_config.required_protocols.len()
        ));
    }
    let issues = collect_xray_render_issues(runtime_config, route_credentials);
    if issues
        .iter()
        .any(|issue| issue.reason.starts_with("generated_inbound_"))
    {
        feature_flags.push("generated-inbound-material-pending-fail-closed".to_string());
    }

    let mut inbounds = if uses_route_assignments {
        render_route_assignment_inbounds(&runtime_config.route_assignments, route_credentials)
    } else {
        render_generated_inbounds(runtime_config, route_credentials)
    };
    let mut outbounds = vec![
        serde_json::json!({
            "tag": "direct",
            "protocol": "freedom",
            "settings": {}
        }),
        serde_json::json!({
            "tag": "blocked",
            "protocol": "blackhole",
            "settings": {}
        }),
    ];
    let mut routing_rules = if uses_route_assignments {
        let (assignment_outbounds, rules) = render_route_assignment_outbounds_and_rules(
            &runtime_config.route_assignments,
            route_credentials,
        );
        outbounds.extend(assignment_outbounds);
        rules
    } else {
        Vec::new()
    };
    let stats_api = stats_api_address.and_then(loopback_socket_address);
    if let Some(stats_api) = stats_api {
        feature_flags.push("xray-user-traffic-stats".to_string());
        inbounds.push(serde_json::json!({
            "tag": "hydra-stats-api",
            "listen": stats_api.ip().to_string(),
            "port": stats_api.port(),
            "protocol": "dokodemo-door",
            "settings": {
                "address": stats_api.ip().to_string()
            }
        }));
        routing_rules.push(serde_json::json!({
            "type": "field",
            "inboundTag": ["hydra-stats-api"],
            "outboundTag": "hydra-stats-api"
        }));
    }

    let mut config = serde_json::json!({
        "log": {
            "loglevel": "warning"
        },
        "inbounds": inbounds,
        "outbounds": outbounds,
        "routing": {
            "domainStrategy": "AsIs",
            "rules": routing_rules
        }
    });
    if stats_api.is_some() {
        let object = config
            .as_object_mut()
            .expect("Xray renderer root is always an object");
        object.insert(
            "api".to_string(),
            serde_json::json!({
                "tag": "hydra-stats-api",
                "services": ["StatsService"]
            }),
        );
        object.insert("stats".to_string(), serde_json::json!({}));
        object.insert(
            "policy".to_string(),
            serde_json::json!({
                "levels": {
                    "0": {
                        "statsUserUplink": true,
                        "statsUserDownlink": true
                    }
                }
            }),
        );
    }

    XrayRenderPlan {
        schema_version: 1,
        renderer_version: 1,
        source_revision: runtime_config.source_revision.clone(),
        created_at_unix: now_unix(),
        xray_detected_version,
        feature_flags,
        issues,
        config,
    }
}

fn loopback_socket_address(value: &str) -> Option<SocketAddr> {
    let address = value.parse::<SocketAddr>().ok()?;
    address.ip().is_loopback().then_some(address)
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    encoded
}

fn summarize_xray_render_plan(plan: &XrayRenderPlan) -> XrayRenderSummary {
    XrayRenderSummary {
        renderer_version: plan.renderer_version,
        source_revision: plan.source_revision.clone(),
        xray_detected_version: plan.xray_detected_version.clone(),
        feature_flags: plan.feature_flags.clone(),
        inbound_count: plan
            .config
            .get("inbounds")
            .and_then(serde_json::Value::as_array)
            .map_or(0, Vec::len),
        outbound_count: plan
            .config
            .get("outbounds")
            .and_then(serde_json::Value::as_array)
            .map_or(0, Vec::len),
        routing_rule_count: plan
            .config
            .get("routing")
            .and_then(|routing| routing.get("rules"))
            .and_then(serde_json::Value::as_array)
            .map_or(0, Vec::len),
        fail_closed: plan
            .feature_flags
            .iter()
            .any(|flag| flag.ends_with("pending-fail-closed"))
            || plan.issues.iter().any(|issue| issue.severity == "error"),
        issue_count: plan.issues.len(),
        issues: plan
            .issues
            .iter()
            .take(MAX_XRAY_RENDER_ISSUES)
            .cloned()
            .collect(),
        created_at_unix: plan.created_at_unix,
    }
}

fn format_xray_render_summary(summary: &XrayRenderSummary) -> String {
    format!(
        "renderer v{}, inbounds={}, outbounds={}, rules={}, fail_closed={}, issues={}, flags={}",
        summary.renderer_version,
        summary.inbound_count,
        summary.outbound_count,
        summary.routing_rule_count,
        summary.fail_closed,
        summary.issue_count,
        summary.feature_flags.join("|"),
    )
}

fn collect_xray_render_issues(
    runtime_config: &NodeRuntimeConfigDocument,
    route_credentials: &RouteCredentialStore,
) -> Vec<XrayRenderIssue> {
    let mut issues = Vec::new();
    for requirement in &runtime_config.required_protocols {
        if requirement.required_component != RuntimeComponentKind::Xray {
            push_xray_render_issue(
                &mut issues,
                &requirement.source_ref,
                &requirement.source,
                "non_xray_protocol_requires_sidecar",
            );
        } else if requirement.source == "generated_inbound" {
            let Some(inbound) = runtime_config
                .inbounds
                .iter()
                .find(|inbound| inbound.tag == requirement.source_ref)
            else {
                push_xray_render_issue(
                    &mut issues,
                    &requirement.source_ref,
                    &requirement.source,
                    "generated_inbound_missing",
                );
                continue;
            };
            if let Some(reason) = generated_inbound_block_reason(runtime_config, inbound) {
                push_xray_render_issue(
                    &mut issues,
                    &requirement.source_ref,
                    &requirement.source,
                    reason,
                );
            }
        }
        if issues.len() >= MAX_XRAY_RENDER_ISSUES {
            return issues;
        }
    }
    for inbound in &runtime_config.inbounds {
        if classify_runtime_protocol(&inbound.protocol).is_none() {
            push_xray_render_issue(
                &mut issues,
                &inbound.tag,
                "generated_inbound",
                "generated_inbound_protocol_unknown",
            );
        }
        if issues.len() >= MAX_XRAY_RENDER_ISSUES {
            return issues;
        }
    }
    for assignment in &runtime_config.route_assignments {
        if let Some(listen) = assignment.listen.as_ref() {
            collect_listen_render_issues(assignment, listen, route_credentials, &mut issues);
        } else {
            push_xray_render_issue(
                &mut issues,
                &assignment.route_id,
                "listen",
                "route_listen_missing",
            );
        }
        if let Some(peer) = assignment.next_peer.as_ref() {
            collect_next_peer_render_issues(assignment, peer, route_credentials, &mut issues);
        }
        if issues.len() >= MAX_XRAY_RENDER_ISSUES {
            break;
        }
    }
    issues
}

fn collect_listen_render_issues(
    assignment: &NodeRouteAssignment,
    listen: &node_domain::NodeRouteListen,
    route_credentials: &RouteCredentialStore,
    issues: &mut Vec<XrayRenderIssue>,
) {
    let Some(security) = listen.security.as_ref() else {
        return;
    };
    if !security.required || security.mode == NodeRouteSecurityMode::None {
        return;
    }
    match security.mode {
        NodeRouteSecurityMode::MutualTls => {
            let has_material = security
                .credential_ref
                .as_deref()
                .and_then(|credential_ref| route_credentials.find(credential_ref))
                .is_some_and(RouteCredential::has_mtls_server_material);
            if !has_material {
                push_xray_render_issue(
                    issues,
                    &assignment.route_id,
                    "listen",
                    "listen_mtls_material_missing",
                );
            }
        }
        NodeRouteSecurityMode::Reality => {
            push_xray_render_issue(
                issues,
                &assignment.route_id,
                "listen",
                "listen_reality_not_supported",
            );
        }
        NodeRouteSecurityMode::None => {}
    }
}

fn collect_next_peer_render_issues(
    assignment: &NodeRouteAssignment,
    peer: &node_domain::NodeRoutePeer,
    route_credentials: &RouteCredentialStore,
    issues: &mut Vec<XrayRenderIssue>,
) {
    if peer.address.is_none() || peer.port.is_none() {
        push_xray_render_issue(
            issues,
            &assignment.route_id,
            "next_peer",
            "next_peer_endpoint_missing",
        );
        return;
    }
    let Some(security) = peer.security.as_ref() else {
        return;
    };
    if !security.required || security.mode == NodeRouteSecurityMode::None {
        return;
    }
    match security.mode {
        NodeRouteSecurityMode::MutualTls => {
            let has_material = security
                .credential_ref
                .as_deref()
                .and_then(|credential_ref| route_credentials.find(credential_ref))
                .is_some_and(RouteCredential::has_mtls_client_material);
            if !has_material {
                push_xray_render_issue(
                    issues,
                    &assignment.route_id,
                    "next_peer",
                    "next_peer_mtls_material_missing",
                );
            }
        }
        NodeRouteSecurityMode::Reality => {
            push_xray_render_issue(
                issues,
                &assignment.route_id,
                "next_peer",
                "next_peer_reality_not_supported",
            );
        }
        NodeRouteSecurityMode::None => {}
    }
}

fn push_xray_render_issue(
    issues: &mut Vec<XrayRenderIssue>,
    route_id: &str,
    scope: &str,
    reason: &str,
) {
    if issues.len() >= MAX_XRAY_RENDER_ISSUES {
        return;
    }
    issues.push(XrayRenderIssue {
        route_id: route_id.to_string(),
        scope: scope.to_string(),
        severity: "error".to_string(),
        reason: reason.to_string(),
    });
}

fn render_generated_inbounds(
    runtime_config: &NodeRuntimeConfigDocument,
    route_credentials: &RouteCredentialStore,
) -> Vec<serde_json::Value> {
    runtime_config
        .inbounds
        .iter()
        .filter_map(|inbound| {
            classify_runtime_protocol(&inbound.protocol).and_then(|(protocol, component)| {
                if component != RuntimeComponentKind::Xray {
                    return None;
                }
                let clients = generated_inbound_client_materials(runtime_config, inbound);
                generated_inbound_can_render(runtime_config, inbound).then(|| {
                    let stream_settings = xray_generated_inbound_stream_settings(
                        runtime_config,
                        inbound,
                        route_credentials,
                    );
                    serde_json::json!({
                        "tag": inbound.tag,
                        "listen": "0.0.0.0",
                        "port": inbound.port,
                        "protocol": xray_generated_inbound_protocol(protocol),
                        "settings": xray_generated_inbound_settings(protocol, &clients),
                        "streamSettings": stream_settings
                    })
                })
            })
        })
        .collect()
}

fn generated_inbound_can_render(
    runtime_config: &NodeRuntimeConfigDocument,
    inbound: &GeneratedInbound,
) -> bool {
    generated_inbound_block_reason(runtime_config, inbound).is_none()
}

fn generated_inbound_block_reason(
    runtime_config: &NodeRuntimeConfigDocument,
    inbound: &GeneratedInbound,
) -> Option<&'static str> {
    let Some((protocol, component)) = classify_runtime_protocol(&inbound.protocol) else {
        return Some("generated_inbound_protocol_unknown");
    };
    if component != RuntimeComponentKind::Xray {
        return Some("non_xray_protocol_requires_sidecar");
    }
    if generated_profile_settings_invalid_for_inbound(runtime_config, inbound, protocol) {
        return Some("generated_inbound_profile_settings_invalid");
    }
    if generated_inbound_client_materials(runtime_config, inbound).is_empty() {
        return Some("generated_inbound_client_material_missing");
    }
    if inbound.tls_enabled && generated_inbound_tls_material(runtime_config, inbound).is_none() {
        return Some("generated_inbound_tls_material_missing");
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GeneratedInboundClientMaterial {
    username: String,
    profile_id: String,
    id: Option<String>,
    password: Option<String>,
    method: Option<String>,
}

fn generated_inbound_client_materials(
    runtime_config: &NodeRuntimeConfigDocument,
    inbound: &GeneratedInbound,
) -> Vec<GeneratedInboundClientMaterial> {
    let Some((protocol, component)) = classify_runtime_protocol(&inbound.protocol) else {
        return Vec::new();
    };
    if component != RuntimeComponentKind::Xray {
        return Vec::new();
    }
    runtime_config
        .users
        .iter()
        .filter(|user| user_allows_inbound(user, &inbound.tag))
        .flat_map(|user| {
            user.proxy_profiles
                .iter()
                .filter(|profile| profile_matches_inbound(profile, &inbound.tag, protocol))
                .filter_map(|profile| parse_generated_client_material(user, profile))
                .filter(move |material| material.is_valid_for(protocol))
        })
        .collect()
}

fn generated_profiles_for_inbound<'a>(
    runtime_config: &'a NodeRuntimeConfigDocument,
    inbound: &GeneratedInbound,
    protocol: RuntimeProtocolKind,
) -> Vec<&'a GeneratedProxyProfile> {
    runtime_config
        .users
        .iter()
        .filter(|user| user_allows_inbound(user, &inbound.tag))
        .flat_map(|user| user.proxy_profiles.iter())
        .filter(|profile| profile_matches_inbound(profile, &inbound.tag, protocol))
        .collect()
}

fn generated_profile_settings_invalid_for_inbound(
    runtime_config: &NodeRuntimeConfigDocument,
    inbound: &GeneratedInbound,
    protocol: RuntimeProtocolKind,
) -> bool {
    runtime_config
        .users
        .iter()
        .filter(|user| user_allows_inbound(user, &inbound.tag))
        .flat_map(|user| user.proxy_profiles.iter())
        .any(|profile| {
            profile_protocol_matches(profile, protocol) && parse_profile_settings(profile).is_none()
        })
}

fn user_allows_inbound(user: &NodeRuntimeUserConfig, inbound_tag: &str) -> bool {
    user.inbounds
        .iter()
        .any(|inbound| inbound.tag == inbound_tag)
}

fn profile_matches_inbound(
    profile: &GeneratedProxyProfile,
    inbound_tag: &str,
    protocol: RuntimeProtocolKind,
) -> bool {
    if !profile_protocol_matches(profile, protocol) {
        return false;
    }
    let Some(settings) = parse_profile_settings(profile) else {
        return false;
    };
    let explicit_inbounds = settings
        .get("inbounds")
        .and_then(serde_json::Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !explicit_inbounds.is_empty() {
        return explicit_inbounds.contains(&inbound_tag);
    }
    settings
        .get("inbound")
        .and_then(serde_json::Value::as_str)
        .is_none_or(|value| value == inbound_tag)
}

fn profile_protocol_matches(
    profile: &GeneratedProxyProfile,
    protocol: RuntimeProtocolKind,
) -> bool {
    let Some((profile_protocol, _component)) = classify_runtime_protocol(&profile.proxy_type)
    else {
        return false;
    };
    profile_protocol == protocol
}

fn parse_generated_client_material(
    user: &NodeRuntimeUserConfig,
    profile: &GeneratedProxyProfile,
) -> Option<GeneratedInboundClientMaterial> {
    let settings = parse_profile_settings(profile)?;
    Some(GeneratedInboundClientMaterial {
        username: string_setting(&settings, &["runtime_username"])
            .unwrap_or_else(|| user.username.clone()),
        profile_id: profile.id.clone(),
        id: string_setting(&settings, &["id", "uuid"]),
        password: string_setting(&settings, &["password"]),
        method: string_setting(&settings, &["method"]),
    })
}

impl GeneratedInboundClientMaterial {
    fn is_valid_for(&self, protocol: RuntimeProtocolKind) -> bool {
        match protocol {
            RuntimeProtocolKind::VlessTlsWebSocket => self.id.as_deref().is_some_and(is_nonempty),
            RuntimeProtocolKind::Hysteria2 | RuntimeProtocolKind::WireGuard => false,
        }
    }
}

fn is_nonempty(value: &str) -> bool {
    !value.trim().is_empty()
}

fn parse_profile_settings(profile: &GeneratedProxyProfile) -> Option<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(&profile.settings_json).ok()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GeneratedInboundTlsMaterial {
    certificate_file: String,
    key_file: String,
}

fn generated_inbound_tls_material(
    runtime_config: &NodeRuntimeConfigDocument,
    inbound: &GeneratedInbound,
) -> Option<GeneratedInboundTlsMaterial> {
    let (protocol, component) = classify_runtime_protocol(&inbound.protocol)?;
    if component != RuntimeComponentKind::Xray {
        return None;
    }
    generated_profiles_for_inbound(runtime_config, inbound, protocol)
        .iter()
        .filter_map(|profile| {
            let settings = parse_profile_settings(profile)?;
            let certificate_file =
                string_setting(&settings, &["tls_certificate_file", "certificate_file"])?;
            let key_file = string_setting(&settings, &["tls_key_file", "key_file"])?;
            (path_is_nonempty_file(&certificate_file) && path_is_nonempty_file(&key_file))
                .then_some(GeneratedInboundTlsMaterial {
                    certificate_file,
                    key_file,
                })
        })
        .next()
}

fn path_is_nonempty_file(path: &str) -> bool {
    std::fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.len() > 0)
        .unwrap_or(false)
}

fn string_setting(settings: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| settings.get(*key).and_then(serde_json::Value::as_str))
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
}

fn string_array_setting(settings: &serde_json::Value, key: &str) -> Option<Vec<String>> {
    settings
        .get(key)
        .and_then(serde_json::Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(serde_json::Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
}

fn xray_generated_inbound_protocol(protocol: RuntimeProtocolKind) -> &'static str {
    match protocol {
        RuntimeProtocolKind::VlessTlsWebSocket => "vless",
        RuntimeProtocolKind::Hysteria2 | RuntimeProtocolKind::WireGuard => "blackhole",
    }
}

fn xray_generated_inbound_settings(
    protocol: RuntimeProtocolKind,
    clients: &[GeneratedInboundClientMaterial],
) -> serde_json::Value {
    match protocol {
        RuntimeProtocolKind::VlessTlsWebSocket => serde_json::json!({
            "clients": clients
                .iter()
                .filter_map(|client| client.id.as_ref().map(|id| serde_json::json!({
                    "id": id,
                    "email": client.username,
                    "level": 0,
                    "flow": ""
                })))
                .collect::<Vec<_>>(),
            "decryption": "none"
        }),
        RuntimeProtocolKind::Hysteria2 | RuntimeProtocolKind::WireGuard => serde_json::json!({}),
    }
}

fn xray_generated_inbound_stream_settings(
    runtime_config: &NodeRuntimeConfigDocument,
    inbound: &GeneratedInbound,
    route_credentials: &RouteCredentialStore,
) -> serde_json::Value {
    let network = inbound.network.trim();
    // Reality displaces TLS: it has its own handshake and its own material. The
    // private key arrives from the panel over a separate channel and is
    // substituted here, the same way certificate paths are.
    if let Some(material) = route_credentials
        .reality_materials
        .iter()
        .find(|candidate| candidate.inbound_tag == inbound.tag)
    {
        return serde_json::json!({
            "network": if network.is_empty() { "tcp" } else { network },
            "security": "reality",
            "realitySettings": {
                "dest": material.dest,
                "xver": 0,
                "serverNames": material.server_names,
                "privateKey": material.private_key_b64,
                "shortIds": material.short_ids
            }
        });
    }
    if !inbound.tls_enabled {
        return serde_json::json!({
            "network": if network.is_empty() { "tcp" } else { network },
            "security": "none"
        });
    }
    let material = generated_inbound_tls_material(runtime_config, inbound)
        .expect("generated TLS inbound must be checked before rendering");
    serde_json::json!({
        "network": if network.is_empty() { "tcp" } else { network },
        "security": "tls",
        "tlsSettings": {
            "certificates": [{
                "certificateFile": material.certificate_file,
                "keyFile": material.key_file
            }]
        }
    })
}

fn render_route_assignment_inbounds(
    assignments: &[NodeRouteAssignment],
    route_credentials: &RouteCredentialStore,
) -> Vec<serde_json::Value> {
    assignments
        .iter()
        .filter_map(|assignment| assignment.listen.as_ref())
        .filter(|listen| can_render_listen_security(listen, route_credentials))
        .map(|listen| {
            let stream_settings = render_listen_stream_settings(listen, route_credentials);
            serde_json::json!({
                "tag": listen.tag,
                "listen": "0.0.0.0",
                "port": listen.port,
                "protocol": xray_inbound_protocol(&listen.protocol),
                "settings": xray_inbound_settings(listen),
                "streamSettings": stream_settings
            })
        })
        .collect()
}

fn render_route_assignment_outbounds_and_rules(
    assignments: &[NodeRouteAssignment],
    route_credentials: &RouteCredentialStore,
) -> (Vec<serde_json::Value>, Vec<serde_json::Value>) {
    let mut outbounds = Vec::new();
    let mut rules = Vec::new();
    let mut seen_outbound_tags = BTreeSet::new();

    for assignment in assignments {
        let Some(listen) = assignment.listen.as_ref() else {
            continue;
        };
        if !can_render_listen_security(listen, route_credentials) {
            continue;
        }
        let outbound_tag = if let Some(next_peer) = assignment.next_peer.as_ref() {
            let tag = format!("route-{}-next", assignment.route_id);
            if next_peer_requires_unavailable_security(next_peer, route_credentials) {
                "blocked".to_string()
            } else if seen_outbound_tags.insert(tag.clone()) {
                let settings = render_next_peer_vless_settings(next_peer);
                outbounds.push(serde_json::json!({
                    "tag": tag,
                    "protocol": "vless",
                    "settings": settings,
                    "streamSettings": render_peer_stream_settings(next_peer, route_credentials)
                }));
                tag
            } else {
                tag
            }
        } else {
            "direct".to_string()
        };
        rules.push(serde_json::json!({
            "type": "field",
            "inboundTag": [listen.tag],
            "outboundTag": outbound_tag
        }));
    }

    (outbounds, rules)
}

fn route_assignment_requires_unavailable_security(
    assignment: &NodeRouteAssignment,
    route_credentials: &RouteCredentialStore,
) -> bool {
    assignment
        .listen
        .as_ref()
        .is_some_and(|listen| !can_render_listen_security(listen, route_credentials))
        || assignment
            .next_peer
            .as_ref()
            .is_some_and(|peer| next_peer_requires_unavailable_security(peer, route_credentials))
}

fn can_render_listen_security(
    listen: &node_domain::NodeRouteListen,
    route_credentials: &RouteCredentialStore,
) -> bool {
    let Some(security) = listen.security.as_ref() else {
        return true;
    };
    if !security.required || security.mode == NodeRouteSecurityMode::None {
        return true;
    }
    match security.mode {
        NodeRouteSecurityMode::MutualTls => security
            .credential_ref
            .as_deref()
            .and_then(|credential_ref| route_credentials.find(credential_ref))
            .is_some_and(RouteCredential::has_mtls_server_material),
        NodeRouteSecurityMode::Reality => false,
        NodeRouteSecurityMode::None => true,
    }
}

fn next_peer_requires_unavailable_security(
    peer: &node_domain::NodeRoutePeer,
    route_credentials: &RouteCredentialStore,
) -> bool {
    if peer.address.is_none() || peer.port.is_none() {
        return true;
    }
    let Some(security) = peer.security.as_ref() else {
        return false;
    };
    if !security.required || security.mode == NodeRouteSecurityMode::None {
        return false;
    }
    match security.mode {
        NodeRouteSecurityMode::MutualTls => security
            .credential_ref
            .as_deref()
            .and_then(|credential_ref| route_credentials.find(credential_ref))
            .is_none_or(|credential| !credential.has_mtls_client_material()),
        NodeRouteSecurityMode::Reality => true,
        NodeRouteSecurityMode::None => false,
    }
}

impl RouteCredentialStore {
    fn find(&self, credential_ref: &str) -> Option<&RouteCredential> {
        self.credentials
            .iter()
            .find(|credential| credential.credential_ref == credential_ref)
    }
}

impl RouteCredential {
    fn has_mtls_server_material(&self) -> bool {
        self.kind == "mutual_tls"
            && self
                .certificate_file
                .as_deref()
                .is_some_and(path_exists_nonempty)
            && self
                .private_key_file
                .as_deref()
                .is_some_and(path_exists_nonempty)
            && self
                .ca_certificate_file
                .as_deref()
                .is_some_and(path_exists_nonempty)
    }

    fn has_mtls_client_material(&self) -> bool {
        self.has_mtls_server_material()
    }
}

fn path_exists_nonempty(path: &str) -> bool {
    fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.len() > 0)
        .unwrap_or(false)
}

/// Inserts a key only when a value is present.
///
/// `json!` with an `Option` writes `null`, and Xray distinguishes an absent key
/// from a key carrying a value: the removed `allowInsecure` is rejected by its
/// presence, not by what it holds. Only values actually set reach the config.
fn insert_if_some<T: Into<serde_json::Value>>(
    target: &mut serde_json::Value,
    key: &str,
    value: Option<T>,
) {
    if let Some(value) = value {
        target[key] = value.into();
    }
}

fn render_listen_stream_settings(
    listen: &node_domain::NodeRouteListen,
    route_credentials: &RouteCredentialStore,
) -> serde_json::Value {
    let Some(security) = listen.security.as_ref() else {
        return serde_json::json!({
            "network": listen.network,
            "security": if listen.tls_enabled { "tls" } else { "none" }
        });
    };
    if security.mode != NodeRouteSecurityMode::MutualTls {
        return serde_json::json!({
            "network": listen.network,
            "security": if listen.tls_enabled { "tls" } else { "none" }
        });
    }
    let credential = security
        .credential_ref
        .as_deref()
        .and_then(|credential_ref| route_credentials.find(credential_ref));
    match credential {
        Some(credential) if credential.has_mtls_server_material() => {
            serde_json::json!({
                "network": listen.network,
                "security": "tls",
                // `allowInsecure` was removed from Xray in favour of
                // `pinnedPeerCertSha256`. The key is not emitted. This is the
                // listen side; pinning is configured by the client.
                "tlsSettings": {
                    "certificates": [
                        {
                            "certificateFile": credential.certificate_file,
                            "keyFile": credential.private_key_file,
                            "usage": "encipherment"
                        },
                        {
                            "certificateFile": credential.ca_certificate_file,
                            "usage": "verify"
                        }
                    ]
                }
            })
        }
        _ => serde_json::json!({
            "network": listen.network,
            "security": "none"
        }),
    }
}

fn render_peer_stream_settings(
    peer: &node_domain::NodeRoutePeer,
    route_credentials: &RouteCredentialStore,
) -> serde_json::Value {
    let Some(security) = peer.security.as_ref() else {
        return serde_json::json!({
            "network": peer.transport.as_deref().unwrap_or("tcp"),
            "security": "none"
        });
    };
    if security.mode != NodeRouteSecurityMode::MutualTls {
        return serde_json::json!({
            "network": peer.transport.as_deref().unwrap_or("tcp"),
            "security": "none"
        });
    }
    let credential = security
        .credential_ref
        .as_deref()
        .and_then(|credential_ref| route_credentials.find(credential_ref));
    match credential {
        Some(credential) if credential.has_mtls_client_material() => {
            // `allowInsecure` was removed from Xray. This is the peer, i.e. the
            // client side, so its replacement belongs here: instead of trusting
            // anything, a specific certificate is pinned.
            let mut tls_settings = serde_json::json!({
                "certificates": [
                    {
                        "certificateFile": credential.certificate_file,
                        "keyFile": credential.private_key_file,
                        "usage": "encipherment"
                    },
                    {
                        "certificateFile": credential.ca_certificate_file,
                        "usage": "verify"
                    }
                ]
            });
            // serverName is emitted only when actually set: `null` here would
            // mean "a name exists and is empty" rather than "there is no name".
            insert_if_some(
                &mut tls_settings,
                "serverName",
                security
                    .server_name
                    .as_deref()
                    .or(credential.server_name.as_deref())
                    .or(peer.sni.as_deref()),
            );
            // Xray expects a single string; multiple pins are comma-separated.
            // The key is omitted when the panel issued no pins.
            if !credential.certificate_pins.is_empty() {
                tls_settings["pinnedPeerCertSha256"] =
                    serde_json::Value::String(credential.certificate_pins.join(","));
            }
            serde_json::json!({
                "network": "tcp",
                "security": "tls",
                "tlsSettings": tls_settings
            })
        }
        _ => serde_json::json!({
            "network": "tcp",
            "security": "none"
        }),
    }
}

fn render_next_peer_vless_settings(peer: &node_domain::NodeRoutePeer) -> serde_json::Value {
    let identity_ref = peer
        .auth
        .as_ref()
        .and_then(|auth| auth.identity_ref.as_deref())
        .unwrap_or(&peer.opaque_peer_id);
    let user_id = stable_vless_uuid(identity_ref);

    match peer.address.as_ref().zip(peer.port) {
        Some((address, port)) => serde_json::json!({
            "vnext": [
                {
                    "address": address,
                    "port": port,
                    "users": [
                        {
                            "id": user_id,
                            "encryption": "none",
                            "level": 0
                        }
                    ]
                }
            ]
        }),
        None => serde_json::json!({
            "vnext": []
        }),
    }
}

fn stable_vless_uuid(identity_ref: &str) -> String {
    let digest = Sha256::digest(identity_ref.as_bytes());
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15],
    )
}

fn xray_inbound_protocol(protocol: &str) -> &str {
    if protocol == "xray" {
        "vless"
    } else {
        protocol
    }
}

fn xray_inbound_settings(listen: &node_domain::NodeRouteListen) -> serde_json::Value {
    if listen.protocol == "xray" {
        let identity_ref = listen
            .auth
            .as_ref()
            .and_then(|auth| auth.identity_ref.as_deref())
            .unwrap_or(&listen.tag);
        serde_json::json!({
            "clients": [
                {
                    "id": stable_vless_uuid(identity_ref),
                    "level": 0,
                    "email": "hydra-route"
                }
            ],
            "decryption": "none"
        })
    } else {
        serde_json::json!({})
    }
}

fn build_cluster_runtime_intents(
    targets: &[GeneratedClusterNodeTarget],
) -> Vec<ClusterRuntimeIntent> {
    #[derive(Default)]
    struct IntentBuilder {
        cluster_name: String,
        cluster_revision: String,
        local_cluster_node_ids: BTreeSet<String>,
        roles: BTreeSet<String>,
        upstream_node_ids: BTreeSet<String>,
        downstream_node_ids: BTreeSet<String>,
        route_edge_ids: BTreeSet<String>,
        accepts_client_entry: bool,
        relays_cluster_traffic: bool,
        handles_cluster_egress: bool,
    }

    let mut builders: HashMap<String, IntentBuilder> = HashMap::new();
    for target in targets {
        let builder = builders.entry(target.cluster_id.clone()).or_default();
        builder.cluster_name = target.cluster_name.clone();
        builder.cluster_revision = target.cluster_revision.clone();
        builder
            .local_cluster_node_ids
            .insert(target.cluster_node_id.clone());
        builder
            .roles
            .insert(cluster_role_name(target.role).to_string());
        builder
            .upstream_node_ids
            .extend(target.upstream_node_ids.iter().cloned());
        builder
            .downstream_node_ids
            .extend(target.downstream_node_ids.iter().cloned());
        builder
            .route_edge_ids
            .extend(target.route_edge_ids.iter().cloned());

        match target.role {
            ClusterNodeRole::Entry => builder.accepts_client_entry = true,
            ClusterNodeRole::Relay => builder.relays_cluster_traffic = true,
            ClusterNodeRole::Exit => builder.handles_cluster_egress = true,
        }
    }

    let mut intents = builders
        .into_iter()
        .map(|(cluster_id, builder)| ClusterRuntimeIntent {
            cluster_id,
            cluster_name: builder.cluster_name,
            cluster_revision: builder.cluster_revision,
            local_cluster_node_ids: builder.local_cluster_node_ids.into_iter().collect(),
            roles: builder.roles.into_iter().collect(),
            upstream_node_ids: builder.upstream_node_ids.into_iter().collect(),
            downstream_node_ids: builder.downstream_node_ids.into_iter().collect(),
            route_edge_ids: builder.route_edge_ids.into_iter().collect(),
            accepts_client_entry: builder.accepts_client_entry,
            relays_cluster_traffic: builder.relays_cluster_traffic,
            handles_cluster_egress: builder.handles_cluster_egress,
        })
        .collect::<Vec<_>>();
    intents.sort_by(|left, right| left.cluster_id.cmp(&right.cluster_id));
    intents
}

fn cluster_role_name(role: ClusterNodeRole) -> &'static str {
    match role {
        ClusterNodeRole::Entry => "entry",
        ClusterNodeRole::Relay => "relay",
        ClusterNodeRole::Exit => "exit",
    }
}

fn load_apply_history(path: &str) -> Result<Vec<ApplyHistoryEntry>> {
    Ok(read_persisted_json::<Vec<ApplyHistoryEntry>>(path)?.unwrap_or_default())
}

fn persist_apply_history(
    path: &str,
    history: &[ApplyHistoryEntry],
    max_entries: usize,
) -> Result<()> {
    let start = history.len().saturating_sub(max_entries);
    // Bounded telemetry: losing the last few records is acceptable.
    persist_json_pretty_atomic(
        path,
        &history[start..],
        "failed to serialize apply history",
        Durability::BestEffort,
    )
}

fn load_runtime_events(path: &str) -> Result<Vec<RuntimeEventEntry>> {
    Ok(read_persisted_json::<Vec<RuntimeEventEntry>>(path)?.unwrap_or_default())
}

fn persist_runtime_events(
    path: &str,
    events: &[RuntimeEventEntry],
    max_entries: usize,
) -> Result<()> {
    let start = events.len().saturating_sub(max_entries);
    persist_json_pretty_atomic(
        path,
        &events[start..],
        "failed to serialize runtime events",
        Durability::BestEffort,
    )
}

/// Whether a write needs fsync.
///
/// Split by recoverability of the content, not by size: unrecoverable and secret
/// data is synced, bounded telemetry is not, because an fsync per event at
/// 1 vCPU costs more than the last few records are worth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Durability {
    /// Loss is unrecoverable, or the content is secret material.
    Fsync,
    /// A bounded telemetry buffer: losing the last few records is acceptable.
    BestEffort,
}

fn persist_json_pretty_atomic<T>(
    path: &str,
    value: &T,
    serialize_context: &'static str,
    durability: Durability,
) -> Result<()>
where
    T: Serialize + ?Sized,
{
    let path = PathBuf::from(path);
    ensure_parent_dir(&path)?;
    let temp_path = temp_path(&path);
    let data = serde_json::to_vec_pretty(value).context(serialize_context)?;
    if durability == Durability::Fsync {
        return replace_file_durably(&temp_path, &path, &data);
    }
    fs::write(&temp_path, data)
        .with_context(|| format!("failed to write {}", temp_path.display()))?;
    fs::rename(&temp_path, &path).with_context(|| {
        format!(
            "failed to move {} to {}",
            temp_path.display(),
            path.display()
        )
    })?;
    Ok(())
}

fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    Ok(())
}

fn sidecar_generated_config_dir(sidecar_runtime_config_path: &str) -> PathBuf {
    let path = PathBuf::from(sidecar_runtime_config_path);
    path.parent()
        .map(|parent| parent.join("sidecar-generated"))
        .unwrap_or_else(|| PathBuf::from("sidecar-generated"))
}

fn wireguard_session_mapping_path(sidecar_runtime_config_path: &str) -> PathBuf {
    sidecar_generated_config_dir(sidecar_runtime_config_path).join("wireguard-session-map.json")
}

fn sidecar_generated_config_path(
    sidecar_runtime_config_path: &str,
    sidecar: LocalSidecarKind,
    source_ref: &str,
) -> PathBuf {
    let base = sidecar_generated_config_dir(sidecar_runtime_config_path);
    match sidecar {
        LocalSidecarKind::Hysteria2 => base
            .join("hysteria2")
            .join(format!("{}.yaml", safe_credential_file_stem(source_ref))),
        LocalSidecarKind::WireGuard => base
            .join("wireguard")
            .join(format!("{}.conf", safe_credential_file_stem(source_ref))),
    }
}

fn temp_path(path: &Path) -> PathBuf {
    let mut temp = path.to_path_buf();
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| format!("{name}.tmp"))
        .unwrap_or_else(|| "node-state.tmp".to_string());
    temp.set_file_name(file_name);
    temp
}

fn backup_path(path: &Path) -> PathBuf {
    let mut backup = path.to_path_buf();
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| format!("{name}.bak"))
        .unwrap_or_else(|| "generated-config.json.bak".to_string());
    backup.set_file_name(file_name);
    backup
}

fn rollback_marker_path(path: &Path) -> PathBuf {
    let mut marker = path.to_path_buf();
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| format!("{name}.rollback-required.json"))
        .unwrap_or_else(|| "generated-config.json.rollback-required.json".to_string());
    marker.set_file_name(file_name);
    marker
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_secs()
}

fn parse_xray_principal_counters(data: &[u8]) -> Result<HashMap<String, u64>> {
    let response = serde_json::from_slice::<XrayStatsResponse>(data)
        .context("Xray stats response is not valid JSON")?;
    let mut counters = HashMap::new();
    for stat in response.stat {
        let Some(principal) = stat
            .name
            .strip_prefix("user>>>")
            .and_then(|value| value.split_once(">>>traffic>>>"))
            .map(|(principal, _)| principal)
            .filter(|principal| !principal.is_empty() && principal.len() <= 256)
        else {
            continue;
        };
        let value = stat
            .value
            .as_u64()
            .or_else(|| stat.value.as_str().and_then(|value| value.parse().ok()))
            .context("Xray stats counter is not an unsigned integer")?;
        let counter = counters.entry(principal.to_string()).or_insert(0_u64);
        *counter = counter.saturating_add(value);
    }
    Ok(counters)
}

fn runtime_activity_session_id(source: &str, principal: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"hydra-runtime-activity-session-v1\0");
    hasher.update(source.as_bytes());
    hasher.update(b"\0");
    hasher.update(principal.as_bytes());
    format!("activity-{}", hex_bytes(&hasher.finalize()))
}

fn bounded_backoff_seconds(base_seconds: u64, max_seconds: u64, failures: u32) -> u64 {
    if failures == 0 {
        return 0;
    }
    let base_seconds = base_seconds.max(1);
    let max_seconds = max_seconds.max(base_seconds);
    let shift = failures.saturating_sub(1).min(20);
    base_seconds.saturating_mul(1_u64 << shift).min(max_seconds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Json, Router,
        extract::State,
        routing::{get, post},
    };
    use node_domain::{
        ClusterNodeRole, GeneratedUserConfig, NodePeerDirection, NodeRouteListen,
        NodeRouteListenAuth, NodeRoutePeer, NodeRoutePeerAuth, NodeRouteSecurityMode,
        NodeRouteTransportSecurity, SubscriptionSessionObservation,
        SubscriptionSessionRuntimeCapability,
    };
    use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair};
    use std::sync::Arc;
    use tokio::sync::Mutex;

    fn route_security(credential_ref: &str) -> NodeRouteTransportSecurity {
        NodeRouteTransportSecurity {
            mode: NodeRouteSecurityMode::MutualTls,
            required: true,
            server_name: Some("relay.local".to_string()),
            public_key: None,
            short_id: None,
            fingerprint: None,
            certificate_pins: Vec::new(),
            credential_ref: Some(credential_ref.to_string()),
            allow_insecure: false,
        }
    }

    fn route_listen(identity_ref: &str) -> NodeRouteListen {
        NodeRouteListen {
            tag: "route-a".to_string(),
            port: 62050,
            protocol: "xray".to_string(),
            network: "tcp".to_string(),
            tls_enabled: false,
            security: None,
            auth: Some(NodeRouteListenAuth {
                method: "mutual_tls".to_string(),
                identity_ref: Some(identity_ref.to_string()),
                credential_ref: Some("listen-ref".to_string()),
                allowed_public_keys: Vec::new(),
                certificate_pins: Vec::new(),
            }),
        }
    }

    fn next_peer(identity_ref: &str) -> NodeRoutePeer {
        NodeRoutePeer {
            direction: NodePeerDirection::Next,
            opaque_peer_id: identity_ref.to_string(),
            address: Some("203.0.113.10".to_string()),
            port: Some(62050),
            public_key: None,
            sni: Some("relay.local".to_string()),
            transport: Some("xray".to_string()),
            security: None,
            auth: Some(NodeRoutePeerAuth {
                method: "mutual_tls".to_string(),
                identity_ref: Some(identity_ref.to_string()),
                credential_ref: Some("peer-ref".to_string()),
                public_key: None,
                certificate_pins: Vec::new(),
            }),
        }
    }

    fn secured_assignment() -> NodeRouteAssignment {
        let mut listen = route_listen("opaque-local");
        listen.security = Some(route_security("local-mtls"));
        let mut peer = next_peer("opaque-next");
        peer.security = Some(route_security("local-mtls"));
        NodeRouteAssignment {
            node_id: "node-a".to_string(),
            route_id: "route-a".to_string(),
            cluster_id: "cluster-a".to_string(),
            cluster_revision: "cluster-rev-a".to_string(),
            role: ClusterNodeRole::Relay,
            listen: Some(listen),
            previous_peer: None,
            next_peer: Some(peer),
        }
    }

    fn route_credential_store() -> (RouteCredentialStore, std::path::PathBuf) {
        let base = std::env::temp_dir().join(format!(
            "hydra-node-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&base).unwrap();
        let cert = base.join("cert.pem");
        let key = base.join("key.pem");
        let ca = base.join("ca.pem");
        fs::write(&cert, "test-cert").unwrap();
        fs::write(&key, "test-key").unwrap();
        fs::write(&ca, "test-ca").unwrap();
        (
            RouteCredentialStore {
                reality_materials: Vec::new(),
                credentials: vec![RouteCredential {
                    credential_ref: "local-mtls".to_string(),
                    kind: "mutual_tls".to_string(),
                    certificate_file: Some(cert.to_string_lossy().to_string()),
                    private_key_file: Some(key.to_string_lossy().to_string()),
                    ca_certificate_file: Some(ca.to_string_lossy().to_string()),
                    public_key: None,
                    server_name: Some("credential.local".to_string()),
                    short_id: None,
                    // SHA-256 of the certificate in hex without colons, exactly
                    // what the panel puts into the material at issuance.
                    certificate_pins: vec![
                        "1a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f809"
                            .to_string(),
                    ],
                }],
            },
            base,
        )
    }

    pub(super) fn temp_test_dir(name: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "hydra-node-{name}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&base).unwrap();
        base
    }

    /// Minimum Xray version worth checking the renderer against.
    ///
    /// Below it the later-removed `allowInsecure` still exists, so an older binary
    /// would accept a config that a current Xray refuses — a green run on it
    /// proves nothing.
    const MINIMUM_XRAY_VERSION: (u32, u32, u32) = (26, 1, 31);

    /// The check is mandatory; set in CI.
    fn xray_integration_test_is_required() -> bool {
        std::env::var("HYDRA_REQUIRE_XRAY_TEST")
            .map(|value| value.trim() == "1")
            .unwrap_or(false)
    }

    /// `Xray 26.6.27 (Xray, Penetrates Everything.) ...` -> `(26, 6, 27)`.
    fn parse_xray_version(output: &str) -> Option<(u32, u32, u32)> {
        let token = output.split_whitespace().nth(1)?;
        let mut parts = token.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts
            .next()
            .unwrap_or("0")
            .trim_end_matches(|symbol: char| !symbol.is_ascii_digit())
            .parse()
            .ok()?;
        Some((major, minor, patch))
    }

    /// A binary that is present but older than the minimum is a failure, not a skip.
    fn assert_xray_binary_is_supported(binary: &Path) {
        let output = std::process::Command::new(binary)
            .arg("version")
            .output()
            .unwrap_or_else(|error| panic!("cannot execute {}: {error}", binary.display()));
        let stdout = String::from_utf8_lossy(&output.stdout);
        let version = parse_xray_version(&stdout)
            .unwrap_or_else(|| panic!("cannot parse the Xray version from output: {stdout}"));

        assert!(
            version >= MINIMUM_XRAY_VERSION,
            "Xray {}.{}.{} is older than the minimum {}.{}.{}: on that version this \
             check proves nothing, because fields removed later are still accepted",
            version.0,
            version.1,
            version.2,
            MINIMUM_XRAY_VERSION.0,
            MINIMUM_XRAY_VERSION.1,
            MINIMUM_XRAY_VERSION.2
        );
    }

    #[test]
    fn xray_version_parser_reads_the_reference_banner() {
        assert_eq!(
            parse_xray_version("Xray 26.6.27 (Xray, Penetrates Everything.) 45cf289"),
            Some((26, 6, 27))
        );
        assert_eq!(parse_xray_version("Xray 26.1.31 (Xray)"), Some((26, 1, 31)));
        assert!(parse_xray_version("").is_none());
        assert!(parse_xray_version("Xray").is_none());

        // Versions compare component-wise, not lexicographically.
        assert!((26, 6, 27) >= MINIMUM_XRAY_VERSION);
        assert!((26, 1, 31) >= MINIMUM_XRAY_VERSION);
        assert!((26, 1, 30) < MINIMUM_XRAY_VERSION);
        assert!((25, 9, 99) < MINIMUM_XRAY_VERSION);
    }

    /// Protocols the node serves through Xray.
    ///
    /// Derived from the same classification the renderer uses, so adding a
    /// protocol must show up here and therefore in the fixture of the real-binary
    /// check.
    fn xray_backed_protocols() -> Vec<&'static str> {
        // Goes through the same classification as the renderer: a protocol that
        // declares itself Xray-backed must appear in the fixture.
        let mut protocols: Vec<&'static str> = [
            "vless",
            "hysteria2",
            "wireguard",
            "vmess",
            "trojan",
            "shadowsocks",
        ]
        .iter()
        .filter_map(|candidate| classify_runtime_protocol(candidate))
        .filter(|(_, component)| *component == RuntimeComponentKind::Xray)
        .map(|(protocol, _)| xray_generated_inbound_protocol(protocol))
        .collect();
        protocols.sort_unstable();
        protocols.dedup();
        protocols
    }

    fn real_xray_test_dir(xray_binary: &Path) -> PathBuf {
        let name = format!(
            "hydra-node-real-xray-generated-protocols-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        if cfg!(unix)
            && xray_binary
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
            && let Some(parent) = xray_binary.parent()
        {
            let base = parent.join("tmp").join(name);
            fs::create_dir_all(&base).unwrap();
            return base;
        }
        let base = std::env::temp_dir().join(name);
        fs::create_dir_all(&base).unwrap();
        base
    }

    fn xray_process_path(path: &Path, xray_binary: &Path) -> String {
        let path_string = path.to_string_lossy();
        if cfg!(unix)
            && xray_binary
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
            && let Some(rest) = path_string.strip_prefix("/mnt/")
        {
            let mut parts = rest.splitn(2, '/');
            if let (Some(drive), Some(tail)) = (parts.next(), parts.next())
                && drive.len() == 1
                && drive
                    .chars()
                    .all(|character| character.is_ascii_alphabetic())
            {
                return format!(
                    "{}:\\{}",
                    drive.to_ascii_uppercase(),
                    tail.replace('/', "\\")
                );
            }
        }
        path_string.to_string()
    }

    fn rewrite_xray_json_paths_for_process(value: &mut serde_json::Value, xray_binary: &Path) {
        match value {
            serde_json::Value::String(string) => {
                let rewritten = xray_process_path(Path::new(string), xray_binary);
                if rewritten != *string {
                    *string = rewritten;
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    rewrite_xray_json_paths_for_process(value, xray_binary);
                }
            }
            serde_json::Value::Object(values) => {
                for value in values.values_mut() {
                    rewrite_xray_json_paths_for_process(value, xray_binary);
                }
            }
            _ => {}
        }
    }

    fn write_test_tls_material(dir: &Path, stem: &str) -> (PathBuf, PathBuf) {
        let subject = format!("{stem}.hydra.test");
        let mut params = CertificateParams::new(vec![subject.clone()]).unwrap();
        params.distinguished_name = DistinguishedName::new();
        params.distinguished_name.push(DnType::CommonName, subject);
        let key_pair = KeyPair::generate().unwrap();
        let certificate = params.self_signed(&key_pair).unwrap();
        let certificate_file = dir.join(format!("{stem}.crt"));
        let key_file = dir.join(format!("{stem}.key"));
        fs::write(&certificate_file, certificate.pem()).unwrap();
        fs::write(&key_file, key_pair.serialize_pem()).unwrap();
        (certificate_file, key_file)
    }

    pub(super) fn fake_failure_binary(dir: &Path) -> PathBuf {
        stub_binary(dir, "fake-xray-fail", None, Some("validation failed"), 1)
    }

    /// Path to the built `hydra-test-stub`.
    ///
    /// The stub is declared as an example rather than a `[[bin]]`: `cargo test`
    /// builds examples automatically while `cargo build --release` does not build
    /// them at all, so a host-only artefact never lands beside the product
    /// binaries.
    ///
    /// The test binary lives in `target/<profile>/deps/` and examples in
    /// `target/<profile>/examples/`. Deriving the path relies on that layout; if
    /// it ever changes, every fixture fails on the explicit assert below rather
    /// than silently. The clean alternative is moving these tests to `tests/`,
    /// where cargo provides `CARGO_BIN_EXE_hydra-test-stub` directly.
    fn test_stub_binary() -> PathBuf {
        let test_binary = std::env::current_exe().expect("test binary path");
        let profile_dir = test_binary
            .parent()
            .and_then(Path::parent)
            .expect("target/<profile>");
        let stub = profile_dir.join("examples").join(if cfg!(windows) {
            "hydra-test-stub.exe"
        } else {
            "hydra-test-stub"
        });
        assert!(
            stub.is_file(),
            "stub is not built: {}. Build it with \
             `cargo build -p node-core --example hydra-test-stub`",
            stub.display()
        );
        stub
    }

    /// A copy of the stub under the required name, emitting a given string.
    ///
    /// A real binary rather than a `#!/bin/sh` script: the interpreter leaves the
    /// exec path together with the dependency on `/bin/sh`.
    fn stub_binary(
        dir: &Path,
        name: &str,
        stdout: Option<&str>,
        stderr: Option<&str>,
        exit_code: i32,
    ) -> PathBuf {
        let path = dir.join(if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.to_string()
        });
        fs::copy(test_stub_binary(), &path).expect("stub copied");
        #[cfg(not(windows))]
        {
            let mut permissions = fs::metadata(&path).unwrap().permissions();
            permissions.set_mode(0o700);
            fs::set_permissions(&path, permissions).unwrap();
        }
        let write_sidecar = |suffix: &str, payload: Option<String>| {
            let mut sidecar = path.clone().into_os_string();
            sidecar.push(suffix);
            match payload {
                Some(payload) => fs::write(&sidecar, payload).unwrap(),
                None => {
                    let _ = fs::remove_file(&sidecar);
                }
            }
        };
        write_sidecar(".out", stdout.map(|value| format!("{value}\n")));
        write_sidecar(".err", stderr.map(|value| format!("{value}\n")));
        write_sidecar(".code", (exit_code != 0).then(|| exit_code.to_string()));
        path
    }

    fn fake_success_binary(dir: &Path) -> PathBuf {
        stub_binary(dir, "fake-xray", None, None, 0)
    }

    pub(super) fn fake_version_binary(dir: &Path, name: &str, version_output: &str) -> PathBuf {
        stub_binary(dir, name, Some(version_output), None, 0)
    }

    fn empty_generated_response(revision: &str) -> NodeAgentConfigResponse {
        NodeAgentConfigResponse {
            node_id: "node-a".to_string(),
            revision: revision.to_string(),
            generated_config: GeneratedCoreConfig {
                generated_at_unix: 1,
                revision: revision.to_string(),
                users: Vec::new(),
                inbounds: Vec::new(),
                hosts: Vec::new(),
                nodes: Vec::new(),
                clusters: Vec::new(),
                cluster_node_targets: Vec::new(),
                node_route_assignments: Vec::new(),
            },
        }
    }

    fn route_credential_bundle(
        credential_ref: &str,
        certificate_pem: &str,
        private_key_pem: &str,
        ca_certificate_pem: &str,
    ) -> NodeRouteCredentialBundle {
        NodeRouteCredentialBundle {
            node_id: "node-a".to_string(),
            revision: "rev-a".to_string(),
            generated_at_unix: 1,
            reality_materials: Vec::new(),
            credentials: vec![node_domain::NodeRouteCredentialMaterial {
                credential_ref: credential_ref.to_string(),
                kind: "mutual_tls".to_string(),
                certificate_pem: certificate_pem.to_string(),
                private_key_pem: private_key_pem.to_string(),
                ca_certificate_pem: ca_certificate_pem.to_string(),
                server_name: Some("relay.local".to_string()),
                certificate_pins: vec!["pin-a".to_string()],
            }],
        }
    }

    fn route_config_response(revision: &str) -> NodeAgentConfigResponse {
        let mut response = empty_generated_response(revision);
        response.generated_config.node_route_assignments = vec![secured_assignment()];
        response
    }

    pub(super) fn runtime_user(
        username: &str,
        inbound: &GeneratedInbound,
        proxy_type: &str,
        settings_json: &str,
    ) -> NodeRuntimeUserConfig {
        NodeRuntimeUserConfig {
            username: username.to_string(),
            inbounds: vec![inbound.clone()],
            proxy_profiles: vec![GeneratedProxyProfile {
                id: format!("{username}-{proxy_type}"),
                name: format!("{username} {proxy_type}"),
                proxy_type: proxy_type.to_string(),
                settings_json: settings_json.to_string(),
            }],
        }
    }

    pub(super) fn runtime_with_generated_inbounds(
        inbounds: Vec<GeneratedInbound>,
        users: Vec<NodeRuntimeUserConfig>,
    ) -> NodeRuntimeConfigDocument {
        let required_protocols = inbounds
            .iter()
            .filter_map(|inbound| {
                classify_runtime_protocol(&inbound.protocol).map(|(protocol, component)| {
                    RuntimeProtocolRequirement {
                        protocol,
                        required_component: component,
                        source: "generated_inbound".to_string(),
                        source_ref: inbound.tag.clone(),
                    }
                })
            })
            .collect();
        NodeRuntimeConfigDocument {
            schema_version: 1,
            node_id: Some("node-a".to_string()),
            source_revision: "rev-generated".to_string(),
            source_generated_at_unix: 1,
            created_at_unix: 2,
            source_user_count: users.len(),
            source_node_count: 1,
            users,
            inbounds,
            hosts: Vec::new(),
            cluster_intents: Vec::new(),
            route_assignments: Vec::new(),
            required_protocols,
        }
    }

    #[derive(Clone)]
    struct TestPanelState {
        config: NodeAgentConfigResponse,
        route_credentials: Arc<Mutex<NodeRouteCredentialBundle>>,
        sync_reports: Arc<Mutex<Vec<NodeSyncRequest>>>,
        subscription_session_reports: Arc<Mutex<Vec<ReportSubscriptionSessionsRequest>>>,
        subscription_enforcement_results:
            Arc<Mutex<Vec<ReportSubscriptionSessionEnforcementResultRequest>>>,
    }

    async fn spawn_test_panel(
        config: NodeAgentConfigResponse,
        route_credentials: NodeRouteCredentialBundle,
    ) -> (
        String,
        tokio::task::JoinHandle<()>,
        Arc<Mutex<Vec<NodeSyncRequest>>>,
        Arc<Mutex<Vec<ReportSubscriptionSessionsRequest>>>,
        Arc<Mutex<Vec<ReportSubscriptionSessionEnforcementResultRequest>>>,
    ) {
        let sync_reports = Arc::new(Mutex::new(Vec::new()));
        let subscription_session_reports = Arc::new(Mutex::new(Vec::new()));
        let subscription_enforcement_results = Arc::new(Mutex::new(Vec::new()));
        let state = TestPanelState {
            config,
            route_credentials: Arc::new(Mutex::new(route_credentials)),
            sync_reports: sync_reports.clone(),
            subscription_session_reports: subscription_session_reports.clone(),
            subscription_enforcement_results: subscription_enforcement_results.clone(),
        };
        let app = Router::new()
            .route("/api/node-agent/me", get(test_panel_me))
            .route("/api/node-agent/config", get(test_panel_config))
            .route(
                "/api/node-agent/route-credentials",
                get(test_panel_route_credentials),
            )
            .route(
                "/api/node-agent/cluster-targets",
                get(test_panel_cluster_targets),
            )
            .route("/api/node-agent/heartbeat", post(test_panel_ok))
            .route("/api/node-agent/sync", post(test_panel_sync))
            .route("/api/node-agent/metrics", post(test_panel_ok))
            .route("/api/node-agent/logs", post(test_panel_ok))
            .route(
                "/api/node-agent/subscription-sessions/report",
                post(test_panel_subscription_sessions),
            )
            .route(
                "/api/node-agent/subscription-sessions/enforcement-result",
                post(test_panel_subscription_enforcement_result),
            )
            .with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (
            url,
            handle,
            sync_reports,
            subscription_session_reports,
            subscription_enforcement_results,
        )
    }

    async fn test_panel_me(State(state): State<TestPanelState>) -> Json<NodeAgentIdentity> {
        Json(NodeAgentIdentity {
            node_id: state.config.node_id.clone(),
            name: "node-a".to_string(),
            status: NodeStatus::Healthy,
            sync_status: NodeSyncStatus::Pending,
            last_applied_revision: None,
        })
    }

    async fn test_panel_config(
        State(state): State<TestPanelState>,
    ) -> Json<NodeAgentConfigResponse> {
        Json(state.config)
    }

    async fn test_panel_route_credentials(
        State(state): State<TestPanelState>,
    ) -> Json<NodeRouteCredentialBundle> {
        Json(state.route_credentials.lock().await.clone())
    }

    async fn test_panel_cluster_targets() -> Json<Vec<GeneratedClusterNodeTarget>> {
        Json(Vec::new())
    }

    async fn test_panel_sync(
        State(state): State<TestPanelState>,
        Json(payload): Json<NodeSyncRequest>,
    ) {
        state.sync_reports.lock().await.push(payload);
    }

    async fn test_panel_ok() {}

    async fn test_panel_subscription_sessions(
        State(state): State<TestPanelState>,
        Json(payload): Json<ReportSubscriptionSessionsRequest>,
    ) -> Json<ReportSubscriptionSessionsResponse> {
        let reported_count = payload.observations.len();
        state
            .subscription_session_reports
            .lock()
            .await
            .push(payload.clone());
        let verdicts = payload
            .observations
            .iter()
            .filter(|_| exact_subscription_session_capabilities(&payload.runtime_capabilities))
            .map(|observation| node_domain::SubscriptionSessionVerdictView {
                session_id: observation.session_id.clone(),
                verdict: node_domain::SubscriptionSessionVerdict::Block,
                reason: "test exact action".to_string(),
                enforcement: Some(node_domain::SubscriptionSessionEnforcementView {
                    action_id: format!("action-{}", observation.session_id),
                    session_id: observation.session_id.clone(),
                    action: node_domain::SubscriptionSessionEnforcementAction::TerminateSession,
                    status: SubscriptionSessionEnforcementStatus::Pending,
                    reason: "test exact action".to_string(),
                    required_adapter: SubscriptionSessionRuntimeAdapter::NodeManagedExactSession,
                    runtime_session_ref_present: true,
                    requires_absence_verification: true,
                    issued_at_unix: 1,
                    updated_at_unix: 1,
                    detail: None,
                }),
                enforcement_unavailable_reason: None,
            })
            .collect::<Vec<_>>();
        let blocked_count = verdicts.len();
        Json(ReportSubscriptionSessionsResponse {
            node_id: state.config.node_id.clone(),
            reported_count,
            allowed_count: reported_count - blocked_count,
            blocked_count,
            verdicts,
        })
    }

    async fn test_panel_subscription_enforcement_result(
        State(state): State<TestPanelState>,
        Json(payload): Json<ReportSubscriptionSessionEnforcementResultRequest>,
    ) {
        state
            .subscription_enforcement_results
            .lock()
            .await
            .push(payload);
    }

    struct TestRuntimeBuilder {
        dir: PathBuf,
        mode: XrayApplyMode,
        binary_path: Option<PathBuf>,
        applied_revision: Option<String>,
        last_config_backup_path: Option<PathBuf>,
        rollback_marker_path: Option<PathBuf>,
    }

    impl TestRuntimeBuilder {
        fn from_dir(dir: PathBuf) -> Self {
            Self {
                dir,
                mode: XrayApplyMode::ValidateJson,
                binary_path: None,
                applied_revision: None,
                last_config_backup_path: None,
                rollback_marker_path: None,
            }
        }

        fn external_process(mut self, binary_path: PathBuf) -> Self {
            self.mode = XrayApplyMode::ExternalProcess;
            self.binary_path = Some(binary_path);
            self
        }

        fn configured_xray_binary(mut self, binary_path: PathBuf) -> Self {
            self.binary_path = Some(binary_path);
            self
        }

        fn applied_revision(mut self, revision: &str) -> Self {
            self.applied_revision = Some(revision.to_string());
            self
        }

        fn rollback_paths(mut self, backup_path: PathBuf, marker_path: PathBuf) -> Self {
            self.last_config_backup_path = Some(backup_path);
            self.rollback_marker_path = Some(marker_path);
            self
        }

        fn xray_config_path(&self) -> PathBuf {
            self.dir.join("xray.json")
        }

        fn state_path(&self) -> PathBuf {
            self.dir.join("node-state.json")
        }

        fn events_path(&self) -> PathBuf {
            self.dir.join("runtime-events.json")
        }

        fn build(self) -> (NodeRuntime, PathBuf) {
            let xray_config_path = self.xray_config_path();
            let mode_name = match self.mode {
                XrayApplyMode::Noop => "noop",
                XrayApplyMode::ValidateJson => "validate_json",
                XrayApplyMode::ExternalValidateOnly => "external_validate_only",
                XrayApplyMode::ExternalProcess => "external_process",
            };
            let config = NodeConfig {
                node_token: "token".to_string(),
                local_state_path: self.state_path().to_string_lossy().to_string(),
                local_config_path: self
                    .dir
                    .join("generated-config.json")
                    .to_string_lossy()
                    .to_string(),
                local_runtime_config_path: self
                    .dir
                    .join("node-runtime-config.json")
                    .to_string_lossy()
                    .to_string(),
                local_sidecar_runtime_config_path: self
                    .dir
                    .join("sidecar-runtime-config.json")
                    .to_string_lossy()
                    .to_string(),
                local_xray_config_path: xray_config_path.to_string_lossy().to_string(),
                route_credentials_path: self
                    .dir
                    .join("route-credentials.json")
                    .to_string_lossy()
                    .to_string(),
                route_credentials_dir: self
                    .dir
                    .join("route-credentials")
                    .to_string_lossy()
                    .to_string(),
                apply_history_path: self
                    .dir
                    .join("apply-history.json")
                    .to_string_lossy()
                    .to_string(),
                runtime_event_history_path: self.events_path().to_string_lossy().to_string(),
                xray_binary_path: self
                    .binary_path
                    .as_ref()
                    .map(|path| path.to_string_lossy().to_string()),
                xray_apply_mode: mode_name.to_string(),
                ..NodeConfig::default()
            };
            let runtime = NodeRuntime {
                config,
                client: reqwest::Client::new(),
                runtime_stats_client: reqwest::Client::new(),
                state: PersistedNodeState {
                    node_id: Some("node-a".to_string()),
                    applied_revision: self.applied_revision,
                    last_config_backup_path: self
                        .last_config_backup_path
                        .map(|path| path.to_string_lossy().to_string()),
                    rollback_marker_path: self
                        .rollback_marker_path
                        .map(|path| path.to_string_lossy().to_string()),
                    ..PersistedNodeState::default()
                },
                xray_manager: XrayProcessManager {
                    mode: self.mode,
                    binary_path: self.binary_path,
                    validate_args: Vec::new(),
                    run_args: Vec::new(),
                },
                xray_child: None,
                apply_history: Vec::new(),
                runtime_events: Vec::new(),
                buffered_logs: Vec::new(),
                active_subscription_session_adapter: None,
                staged_subscription_sessions: None,
                runtime_activity: RuntimeActivityState::default(),
                pending_subscription_session_enforcements: Vec::new(),
            };
            (runtime, self.dir)
        }
    }

    #[test]
    fn external_xray_validation_has_safe_default_args() {
        let manager = XrayProcessManager {
            mode: XrayApplyMode::ExternalProcess,
            binary_path: Some(PathBuf::from("/usr/local/bin/xray")),
            validate_args: Vec::new(),
            run_args: Vec::new(),
        };

        let args = manager.effective_validate_args();
        assert_eq!(args, ["run", "-test", "-config", "{config_path}"]);

        let expanded = manager.expand_args(&args, Path::new("/etc/hydra/xray.json"));
        assert_eq!(
            expanded,
            ["run", "-test", "-config", "/etc/hydra/xray.json"]
        );
    }

    #[test]
    fn runtime_validation_report_exposes_disabled_xray_when_binary_is_not_configured() {
        let dir = temp_test_dir("runtime-validation-disabled-xray");
        let (runtime, dir) = TestRuntimeBuilder::from_dir(dir).build();

        let report = runtime.snapshot().runtime_validation_report;

        fs::remove_dir_all(dir).ok();
        assert!(!report.ready);
        let xray = report
            .components
            .iter()
            .find(|component| component.component == RuntimeComponentKind::Xray)
            .unwrap();
        assert_eq!(xray.readiness, RuntimeComponentReadiness::Disabled);
        assert!(xray.required);
        assert!(
            report
                .disabled_reasons
                .iter()
                .any(|reason| reason.contains("Xray"))
        );
        assert_eq!(report.protocol_count, 3);
        assert!(
            report
                .protocols
                .iter()
                .all(|protocol| protocol.readiness != RuntimeProtocolReadiness::Ready)
        );
    }

    #[test]
    fn runtime_alerts_summarize_active_failures_without_secret_material() {
        let dir = temp_test_dir("runtime-alerts");
        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir).build();
        runtime.state.consecutive_tick_failures = 2;
        runtime.state.last_error =
            Some("panel transport failed\nAuthorization: Bearer should-not-leak".to_string());
        runtime.state.xray_runtime.status = Some(XrayRuntimeStatus::Failed);
        runtime.state.xray_runtime.last_detail =
            Some("xray exited with code 23 after validation failure".to_string());
        runtime.state.last_xray_update_status = Some(XrayUpdateStatus::Failed);
        runtime.state.last_xray_update_detail =
            Some("updated xray binary failed validation".to_string());

        let alerts = runtime.runtime_alerts();

        fs::remove_dir_all(dir).ok();
        assert!(alerts.iter().any(|alert| {
            alert.kind == RuntimeAlertKind::PollBackoff
                && alert.severity == RuntimeAlertSeverity::Warning
        }));
        assert!(alerts.iter().any(|alert| {
            alert.kind == RuntimeAlertKind::XrayRuntimeFailed
                && alert.severity == RuntimeAlertSeverity::Critical
        }));
        assert!(alerts.iter().any(|alert| {
            alert.kind == RuntimeAlertKind::XrayUpdateFailed
                && alert.severity == RuntimeAlertSeverity::Critical
        }));
        assert!(alerts.iter().any(|alert| {
            alert.kind == RuntimeAlertKind::RuntimeValidationFailed
                && alert.source == RuntimeAlertSource::RuntimeValidation
        }));
        let serialized = serde_json::to_string(&alerts).expect("alerts serialize");
        assert!(!serialized.contains('\n'));
        assert!(!serialized.contains("should-not-leak"));
        assert!(!serialized.contains("PRIVATE KEY"));
        assert!(alerts.len() <= MAX_RUNTIME_ALERTS);

        let snapshot = runtime.snapshot();
        assert_eq!(snapshot.runtime_alerts.len(), alerts.len());
    }

    #[test]
    fn runtime_validation_report_marks_xray_ready_after_successful_validation() {
        let dir = temp_test_dir("runtime-validation-ready-xray");
        let binary = fake_success_binary(&dir);
        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir)
            .external_process(binary)
            .build();
        fs::write(&runtime.config.local_xray_config_path, "{}").unwrap();
        runtime.state.xray_runtime.last_validated_at_unix = Some(123);
        runtime.state.xray_detected_version = Some("25.1.1".to_string());

        let report = runtime.snapshot().runtime_validation_report;

        fs::remove_dir_all(dir).ok();
        assert!(report.ready);
        let xray = report
            .components
            .iter()
            .find(|component| component.component == RuntimeComponentKind::Xray)
            .unwrap();
        assert_eq!(xray.readiness, RuntimeComponentReadiness::Ready);
        assert_eq!(xray.detected_version.as_deref(), Some("25.1.1"));
        assert_eq!(xray.last_validated_at_unix, Some(123));
        let vless = report
            .protocols
            .iter()
            .find(|protocol| protocol.protocol == RuntimeProtocolKind::VlessTlsWebSocket)
            .unwrap();
        assert_eq!(vless.readiness, RuntimeProtocolReadiness::Ready);
        let hysteria = report
            .protocols
            .iter()
            .find(|protocol| protocol.protocol == RuntimeProtocolKind::Hysteria2)
            .unwrap();
        assert_eq!(hysteria.readiness, RuntimeProtocolReadiness::Disabled);
    }

    #[test]
    fn runtime_validation_report_blocks_required_sidecar_protocols() {
        let dir = temp_test_dir("runtime-validation-required-sidecar");
        let binary = fake_success_binary(&dir);
        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir)
            .external_process(binary)
            .build();
        fs::write(&runtime.config.local_xray_config_path, "{}").unwrap();
        runtime.state.xray_runtime.last_validated_at_unix = Some(123);
        runtime.state.last_runtime_protocol_requirements = vec![RuntimeProtocolRequirement {
            protocol: RuntimeProtocolKind::Hysteria2,
            required_component: RuntimeComponentKind::Hysteria2,
            source: "generated_inbound".to_string(),
            source_ref: "hy2-in".to_string(),
        }];

        let report = runtime.snapshot().runtime_validation_report;

        fs::remove_dir_all(dir).ok();
        assert!(!report.ready);
        assert_eq!(report.required_protocol_count, 1);
        assert_eq!(
            report.required_protocols[0].readiness,
            RuntimeProtocolReadiness::Blocked
        );
        assert!(
            report.required_protocols[0]
                .detail
                .contains("generated_inbound:hy2-in")
        );
    }

    #[test]
    fn sidecar_lifecycle_placeholder_is_fail_closed() {
        let dir = temp_test_dir("sidecar-placeholder");
        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir).build();

        let response = runtime
            .execute_local_sidecar_action(LocalSidecarKind::Hysteria2, LocalSidecarAction::Start)
            .unwrap();

        assert_eq!(response.sidecar, LocalSidecarKind::Hysteria2);
        assert_eq!(response.action, LocalSidecarAction::Start);
        assert_eq!(response.status, LocalSidecarStatus::Disabled);
        assert!(!response.supported);
        assert!(!response.plan.executor_required);
        assert!(response.plan.dry_run);
        assert!(response.acceptance.fail_closed);
        assert_eq!(
            response.acceptance.expected_status,
            LocalSidecarStatus::Disabled
        );
        assert!(response.logs.is_empty());

        let snapshot = runtime.snapshot();
        let sidecar = snapshot
            .sidecars
            .iter()
            .find(|sidecar| sidecar.sidecar == LocalSidecarKind::Hysteria2)
            .unwrap();
        assert_eq!(sidecar.status, LocalSidecarStatus::Disabled);
        assert_eq!(sidecar.last_action, Some(LocalSidecarAction::Start));
        assert!(
            sidecar
                .last_detail
                .as_deref()
                .unwrap()
                .contains("not configured")
        );
        assert_eq!(sidecar.logs.len(), 1);

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn sidecar_executor_result_accepts_matching_acceptance_contract() {
        let dir = temp_test_dir("sidecar-result-accepted");
        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir).build();
        let plan = runtime
            .execute_local_sidecar_action(LocalSidecarKind::Hysteria2, LocalSidecarAction::Start)
            .unwrap();

        let response = runtime
            .complete_local_sidecar_action(
                LocalSidecarKind::Hysteria2,
                LocalSidecarAction::Start,
                LocalSidecarExecutorResultRequest {
                    command_id: plan.plan.command_id,
                    status: plan.acceptance.expected_status,
                    completed_checks: plan.acceptance.required_checks,
                    exit_code: Some(0),
                    detail: Some("placeholder executor contract confirmed".to_string()),
                    completed_at_unix: Some(123),
                },
            )
            .unwrap();

        assert!(response.accepted);
        assert_eq!(response.status, LocalSidecarStatus::Disabled);
        assert!(response.failed_checks.is_empty());
        let sidecar = runtime
            .snapshot()
            .sidecars
            .into_iter()
            .find(|sidecar| sidecar.sidecar == LocalSidecarKind::Hysteria2)
            .unwrap();
        assert_eq!(sidecar.status, LocalSidecarStatus::Disabled);
        assert!(
            sidecar
                .last_detail
                .as_deref()
                .unwrap()
                .contains("placeholder executor contract confirmed")
        );

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn sidecar_executor_result_rejects_mismatched_acceptance_contract() {
        let dir = temp_test_dir("sidecar-result-rejected");
        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir).build();

        let response = runtime
            .complete_local_sidecar_action(
                LocalSidecarKind::WireGuard,
                LocalSidecarAction::Restart,
                LocalSidecarExecutorResultRequest {
                    command_id: "wrong-command".to_string(),
                    status: LocalSidecarStatus::Running,
                    completed_checks: Vec::new(),
                    exit_code: Some(1),
                    detail: Some("should not be accepted".to_string()),
                    completed_at_unix: Some(123),
                },
            )
            .unwrap();

        assert!(!response.accepted);
        assert_eq!(response.status, LocalSidecarStatus::Failed);
        assert!(
            response
                .failed_checks
                .iter()
                .any(|check| check.contains("command_id mismatch"))
        );
        assert!(
            response
                .failed_checks
                .iter()
                .any(|check| check.contains("status mismatch"))
        );
        let sidecar = runtime
            .snapshot()
            .sidecars
            .into_iter()
            .find(|sidecar| sidecar.sidecar == LocalSidecarKind::WireGuard)
            .unwrap();
        assert_eq!(sidecar.status, LocalSidecarStatus::Failed);
        assert!(
            sidecar
                .last_detail
                .as_deref()
                .unwrap()
                .contains("executor result rejected")
        );

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn configured_sidecar_start_command_executes_without_shell_and_updates_state() {
        let dir = temp_test_dir("configured-sidecar-start");
        let command = fake_version_binary(&dir, "sidecar-start", "started");
        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir).build();
        runtime.config.hysteria2_start_args = vec![command.to_string_lossy().to_string()];

        let response = runtime
            .execute_local_sidecar_action(LocalSidecarKind::Hysteria2, LocalSidecarAction::Start)
            .unwrap();

        assert_eq!(response.status, LocalSidecarStatus::Running);
        assert!(response.supported);
        assert!(!response.plan.dry_run);
        assert_eq!(
            response.acceptance.expected_status,
            LocalSidecarStatus::Running
        );
        assert!(
            response
                .logs
                .iter()
                .any(|line| line.contains("stdout: started"))
        );
        let sidecar = runtime
            .snapshot()
            .sidecars
            .into_iter()
            .find(|sidecar| sidecar.sidecar == LocalSidecarKind::Hysteria2)
            .unwrap();
        assert_eq!(sidecar.status, LocalSidecarStatus::Running);

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn sidecar_logs_action_returns_bounded_state_logs() {
        let dir = temp_test_dir("sidecar-logs");
        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir).build();

        for _ in 0..(MAX_SIDECAR_STATE_LOGS + 4) {
            runtime
                .execute_local_sidecar_action(
                    LocalSidecarKind::WireGuard,
                    LocalSidecarAction::Status,
                )
                .unwrap();
        }
        let response = runtime
            .execute_local_sidecar_action(LocalSidecarKind::WireGuard, LocalSidecarAction::Logs)
            .unwrap();

        assert_eq!(response.sidecar, LocalSidecarKind::WireGuard);
        assert_eq!(response.action, LocalSidecarAction::Logs);
        assert!(!response.plan.executor_required);
        assert!(response.acceptance.fail_closed);
        assert_eq!(response.logs.len(), MAX_SIDECAR_STATE_LOGS);

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn sidecar_status_reports_missing_when_configured_binary_is_absent() {
        let dir = temp_test_dir("sidecar-missing-binary");
        let missing = dir.join("missing-hysteria");
        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir).build();
        runtime.config.hysteria2_binary_path = Some(missing.to_string_lossy().to_string());

        let response = runtime
            .execute_local_sidecar_action(LocalSidecarKind::Hysteria2, LocalSidecarAction::Status)
            .unwrap();

        let missing_path = missing.to_string_lossy().to_string();
        assert_eq!(response.status, LocalSidecarStatus::Missing);
        assert!(!response.supported);
        assert_eq!(response.binary_path.as_deref(), Some(missing_path.as_str()));
        assert!(response.detected_version.is_none());
        let component = runtime
            .snapshot()
            .runtime_validation_report
            .components
            .into_iter()
            .find(|component| component.component == RuntimeComponentKind::Hysteria2)
            .unwrap();
        assert_eq!(component.readiness, RuntimeComponentReadiness::Missing);

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn sidecar_validate_reports_ready_with_detected_version() {
        let dir = temp_test_dir("sidecar-ready-binary");
        let binary = fake_version_binary(&dir, "hysteria", "hysteria version v2.6.0");
        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir).build();
        runtime.config.hysteria2_binary_path = Some(binary.to_string_lossy().to_string());

        let response = runtime
            .execute_local_sidecar_action(LocalSidecarKind::Hysteria2, LocalSidecarAction::Validate)
            .unwrap();

        assert_eq!(response.status, LocalSidecarStatus::Ready);
        assert!(response.supported);
        assert_eq!(
            response.detected_version.as_deref(),
            Some("hysteria version v2.6.0")
        );
        assert!(response.validated_at_unix.is_some());
        let snapshot = runtime.snapshot();
        let sidecar = snapshot
            .sidecars
            .iter()
            .find(|sidecar| sidecar.sidecar == LocalSidecarKind::Hysteria2)
            .unwrap();
        assert_eq!(sidecar.status, LocalSidecarStatus::Ready);
        assert_eq!(
            sidecar.detected_version.as_deref(),
            Some("hysteria version v2.6.0")
        );
        let report = snapshot.runtime_validation_report;
        let component = report
            .components
            .iter()
            .cloned()
            .into_iter()
            .find(|component| component.component == RuntimeComponentKind::Hysteria2)
            .unwrap();
        assert_eq!(component.readiness, RuntimeComponentReadiness::Ready);
        assert_eq!(
            component.detected_version.as_deref(),
            Some("hysteria version v2.6.0")
        );
        let protocol = report
            .protocols
            .iter()
            .find(|protocol| protocol.protocol == RuntimeProtocolKind::Hysteria2)
            .unwrap();
        assert_eq!(protocol.readiness, RuntimeProtocolReadiness::Blocked);
        assert!(
            protocol
                .disabled_reason
                .as_deref()
                .unwrap()
                .contains("generated sidecar config")
        );

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn wireguard_preflight_is_degraded_without_wg_quick() {
        let dir = temp_test_dir("wireguard-degraded-without-wg-quick");
        let wg = fake_version_binary(&dir, "wg", "wireguard-tools v1.0.20210914");
        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir).build();
        runtime.config.wireguard_binary_path = Some(wg.to_string_lossy().to_string());

        let response = runtime
            .execute_local_sidecar_action(LocalSidecarKind::WireGuard, LocalSidecarAction::Validate)
            .unwrap();

        assert_eq!(response.status, LocalSidecarStatus::Degraded);
        assert!(!response.supported);
        assert_eq!(
            response.detected_version.as_deref(),
            Some("wireguard-tools v1.0.20210914")
        );
        assert!(
            response
                .detail
                .contains("wg-quick helper is not configured")
        );
        let component = runtime
            .snapshot()
            .runtime_validation_report
            .components
            .into_iter()
            .find(|component| component.component == RuntimeComponentKind::WireGuard)
            .unwrap();
        assert_eq!(component.readiness, RuntimeComponentReadiness::Failed);

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn wireguard_preflight_is_ready_with_wg_and_wg_quick() {
        let dir = temp_test_dir("wireguard-ready-with-wg-quick");
        let wg = fake_version_binary(&dir, "wg", "wireguard-tools v1.0.20210914");
        let wg_quick = fake_version_binary(&dir, "wg-quick", "wg-quick helper");
        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir).build();
        runtime.config.wireguard_binary_path = Some(wg.to_string_lossy().to_string());
        runtime.config.wg_quick_binary_path = Some(wg_quick.to_string_lossy().to_string());

        let response = runtime
            .execute_local_sidecar_action(LocalSidecarKind::WireGuard, LocalSidecarAction::Validate)
            .unwrap();

        assert_eq!(response.status, LocalSidecarStatus::Ready);
        assert!(response.supported);
        assert!(response.detail.contains("wg-quick helper configured"));
        let component = runtime
            .snapshot()
            .runtime_validation_report
            .components
            .into_iter()
            .find(|component| component.component == RuntimeComponentKind::WireGuard)
            .unwrap();
        assert_eq!(component.readiness, RuntimeComponentReadiness::Ready);

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn runtime_config_records_required_protocols_from_generated_inbounds() {
        let mut response = empty_generated_response("rev-a");
        response.generated_config.inbounds = vec![
            GeneratedInbound {
                tag: "vless-in".to_string(),
                port: 10001,
                protocol: "vless".to_string(),
                network: "ws".to_string(),
                tls_enabled: true,
            },
            GeneratedInbound {
                tag: "hy2-in".to_string(),
                port: 10002,
                protocol: "hysteria2".to_string(),
                network: "udp".to_string(),
                tls_enabled: true,
            },
        ];

        let runtime_config =
            build_node_runtime_config_document(&response, &Some("node-a".to_string()), &[], &[]);

        assert_eq!(runtime_config.required_protocols.len(), 2);
        assert!(runtime_config.required_protocols.iter().any(|requirement| {
            requirement.protocol == RuntimeProtocolKind::VlessTlsWebSocket
                && requirement.required_component == RuntimeComponentKind::Xray
        }));
        assert!(runtime_config.required_protocols.iter().any(|requirement| {
            requirement.protocol == RuntimeProtocolKind::Hysteria2
                && requirement.required_component == RuntimeComponentKind::Hysteria2
        }));
    }

    #[test]
    fn sidecar_runtime_config_records_sidecar_requirements_fail_closed() {
        let mut response = empty_generated_response("rev-a");
        response.generated_config.inbounds = vec![GeneratedInbound {
            tag: "hy2-in".to_string(),
            port: 10002,
            protocol: "hysteria2".to_string(),
            network: "udp".to_string(),
            tls_enabled: true,
        }];
        let runtime_config =
            build_node_runtime_config_document(&response, &Some("node-a".to_string()), &[], &[]);

        let sidecar_config = build_sidecar_runtime_config_document(&runtime_config);

        assert_eq!(sidecar_config.schema_version, 1);
        assert_eq!(sidecar_config.source_revision, "rev-a");
        assert_eq!(sidecar_config.requirements.len(), 1);
        let requirement = sidecar_config.requirements.first().unwrap();
        assert_eq!(requirement.sidecar, LocalSidecarKind::Hysteria2);
        assert_eq!(requirement.protocol, RuntimeProtocolKind::Hysteria2);
        assert_eq!(requirement.source_ref, "hy2-in");
        assert_eq!(requirement.status, SidecarRuntimeRequirementStatus::Blocked);
        assert!(requirement.reason.contains("generated config exists"));
        assert_eq!(requirement.planned_envelopes.len(), 3);
        assert!(requirement.planned_envelopes.iter().any(|envelope| {
            envelope.action == LocalSidecarAction::Validate
                && envelope.plan.command_id == "hysteria2:hy2-in:validate:placeholder"
                && envelope.acceptance.fail_closed
        }));
        assert!(requirement.planned_envelopes.iter().any(|envelope| {
            envelope.action == LocalSidecarAction::Start
                && envelope.plan.command_id == "hysteria2:hy2-in:start:placeholder"
                && envelope.acceptance.expected_status == LocalSidecarStatus::Disabled
        }));
    }

    #[test]
    fn sidecar_runtime_config_renders_hysteria2_payload_when_material_is_complete() {
        let dir = temp_test_dir("sidecar-runtime-hysteria2-payload");
        let cert = dir.join("hy2.crt");
        let key = dir.join("hy2.key");
        fs::write(&cert, "CERT").unwrap();
        fs::write(&key, "KEY").unwrap();
        let inbound = GeneratedInbound {
            tag: "hy2-in".to_string(),
            port: 8443,
            protocol: "hysteria2".to_string(),
            network: "udp".to_string(),
            tls_enabled: true,
        };
        let settings = serde_json::json!({
            "inbound": "hy2-in",
            "password": "hy2-secret",
            "tls_certificate_file": cert,
            "tls_key_file": key
        })
        .to_string();
        let runtime_config = runtime_with_generated_inbounds(
            vec![inbound.clone()],
            vec![runtime_user("alice", &inbound, "hysteria2", &settings)],
        );

        let sidecar_config = build_sidecar_runtime_config_document(&runtime_config);

        fs::remove_dir_all(dir).ok();
        assert_eq!(sidecar_config.hysteria2_configs.len(), 1);
        let config = &sidecar_config.hysteria2_configs[0];
        assert_eq!(config.tag, "hy2-in");
        assert_eq!(config.port, 8443);
        assert_eq!(
            config.auth_users,
            vec![Hysteria2RuntimeUser {
                runtime_username: "alice".to_string(),
                password: "hy2-secret".to_string(),
            }]
        );
        assert!(config.traffic_stats_listen.starts_with("127.0.0.1:"));
        assert_eq!(config.traffic_stats_secret.len(), 64);
        assert!(sidecar_config.wireguard_configs.is_empty());
    }

    #[test]
    fn sidecar_runtime_config_renders_wireguard_payload_when_material_is_complete() {
        let inbound = GeneratedInbound {
            tag: "wg-in".to_string(),
            port: 51820,
            protocol: "wireguard".to_string(),
            network: "udp".to_string(),
            tls_enabled: false,
        };
        let runtime_config = runtime_with_generated_inbounds(
            vec![inbound.clone()],
            vec![runtime_user(
                "alice",
                &inbound,
                "wireguard",
                r#"{"inbound":"wg-in","private_key":"priv","address":"10.0.0.1/24","peer_public_key":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=","peer_endpoint":"198.51.100.1:51820","allowed_ips":["10.0.0.2/32"]}"#,
            )],
        );

        let sidecar_config = build_sidecar_runtime_config_document(&runtime_config);

        assert_eq!(sidecar_config.wireguard_configs.len(), 1);
        let config = &sidecar_config.wireguard_configs[0];
        assert_eq!(config.tag, "wg-in");
        assert_eq!(config.interface_private_key, "priv");
        assert_eq!(config.peers.len(), 1);
        assert_eq!(
            config.peers[0].endpoint.as_deref(),
            Some("198.51.100.1:51820")
        );
        assert_eq!(config.peers[0].allowed_ips, vec!["10.0.0.2/32".to_string()]);
    }

    #[test]
    fn sidecar_runtime_config_renders_all_wireguard_device_profiles_for_one_client() {
        let inbound = GeneratedInbound {
            tag: "wg-in".to_string(),
            port: 51820,
            protocol: "wireguard".to_string(),
            network: "udp".to_string(),
            tls_enabled: false,
        };
        let mut user = runtime_user(
            "catalog/client-a",
            &inbound,
            "wireguard",
            r#"{"inbound":"wg-in","private_key":"priv","address":"10.0.0.1/24","peer_public_key":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=","allowed_ips":["10.0.0.2/32"],"device_fingerprint":"wireguard-sha256:device-a"}"#,
        );
        user.proxy_profiles.push(GeneratedProxyProfile {
            id: "catalog-client-a-wireguard-device-b".to_string(),
            name: "device b".to_string(),
            proxy_type: "wireguard".to_string(),
            settings_json: r#"{"inbound":"wg-in","private_key":"priv","address":"10.0.0.1/24","peer_public_key":"BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB=","allowed_ips":["10.0.0.3/32"],"device_fingerprint":"wireguard-sha256:device-b"}"#.to_string(),
        });
        let runtime_config = runtime_with_generated_inbounds(vec![inbound], vec![user]);

        let sidecar_config = build_sidecar_runtime_config_document(&runtime_config);

        assert_eq!(sidecar_config.wireguard_configs.len(), 1);
        let peers = &sidecar_config.wireguard_configs[0].peers;
        assert_eq!(peers.len(), 2);
        assert_eq!(
            peers[0].public_key,
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
        );
        assert_eq!(
            peers[1].public_key,
            "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB="
        );
        assert_eq!(peers[0].runtime_username, "catalog/client-a");
        assert_eq!(peers[1].runtime_username, "catalog/client-a");
    }

    #[test]
    fn sidecar_generated_config_files_are_written_from_payloads() {
        let dir = temp_test_dir("sidecar-generated-config-files");
        let sidecar_runtime_path = dir.join("sidecar-runtime-config.json");
        let config = SidecarRuntimeConfigDocument {
            schema_version: 1,
            source_revision: "rev-a".to_string(),
            created_at_unix: 1,
            requirements: Vec::new(),
            hysteria2_configs: vec![Hysteria2RuntimeConfig {
                tag: "hy2-in".to_string(),
                listen: "0.0.0.0".to_string(),
                port: 8443,
                auth_users: vec![Hysteria2RuntimeUser {
                    runtime_username: "catalog/client-a/device/device-a".to_string(),
                    password: "secret".to_string(),
                }],
                traffic_stats_listen: "127.0.0.1:19090".to_string(),
                traffic_stats_secret: "stats-secret".to_string(),
                certificate_file: "/etc/hy2.crt".to_string(),
                key_file: "/etc/hy2.key".to_string(),
            }],
            wireguard_configs: vec![WireGuardRuntimeConfig {
                tag: "wg-in".to_string(),
                interface_private_key: "priv".to_string(),
                interface_address: "10.0.0.1/24".to_string(),
                listen_port: Some(51820),
                peers: vec![WireGuardRuntimePeer {
                    runtime_username: "catalog/client-a".to_string(),
                    public_key: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".to_string(),
                    endpoint: Some("198.51.100.1:51820".to_string()),
                    allowed_ips: vec!["10.0.0.2/32".to_string()],
                    device_fingerprint: "wireguard-sha256:device-a".to_string(),
                }],
            }],
        };

        let written = persist_sidecar_generated_config_files(
            &sidecar_runtime_path.to_string_lossy(),
            &config,
        )
        .unwrap();
        let base = sidecar_generated_config_dir(&sidecar_runtime_path.to_string_lossy());
        let hysteria = fs::read_to_string(base.join("hysteria2").join("hy2-in.yaml")).unwrap();
        let wireguard = fs::read_to_string(base.join("wireguard").join("wg-in.conf")).unwrap();

        fs::remove_dir_all(dir).ok();
        assert_eq!(written, 2);
        assert!(hysteria.contains("listen: :8443"));
        assert!(hysteria.contains("type: userpass"));
        assert!(hysteria.contains("\"catalog/client-a/device/device-a\": \"secret\""));
        assert!(hysteria.contains("trafficStats:"));
        assert!(hysteria.contains("listen: \"127.0.0.1:19090\""));
        assert!(wireguard.contains("[Interface]"));
        assert!(wireguard.contains("PrivateKey = priv"));
        assert!(wireguard.contains("[Peer]"));
    }

    #[test]
    fn wireguard_session_mapping_is_secret_safe_and_peer_scoped() {
        let dir = temp_test_dir("wireguard-session-mapping");
        let sidecar_runtime_path = dir.join("sidecar-runtime-config.json");
        let config = SidecarRuntimeConfigDocument {
            schema_version: 1,
            source_revision: "rev-a".to_string(),
            created_at_unix: 42,
            requirements: Vec::new(),
            hysteria2_configs: Vec::new(),
            wireguard_configs: vec![WireGuardRuntimeConfig {
                tag: "wg-in".to_string(),
                interface_private_key: "must-not-leak".to_string(),
                interface_address: "10.0.0.1/24".to_string(),
                listen_port: Some(51820),
                peers: vec![WireGuardRuntimePeer {
                    runtime_username: "catalog/client-a".to_string(),
                    public_key: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".to_string(),
                    endpoint: None,
                    allowed_ips: vec!["10.0.0.2/32".to_string()],
                    device_fingerprint: "wireguard-sha256:device-a".to_string(),
                }],
            }],
        };

        let mapping = build_wireguard_session_mapping(&config).unwrap();
        persist_wireguard_session_mapping(&sidecar_runtime_path.to_string_lossy(), &mapping)
            .unwrap();
        let path = wireguard_session_mapping_path(&sidecar_runtime_path.to_string_lossy());
        let persisted = fs::read_to_string(&path).unwrap();

        fs::remove_dir_all(dir).ok();
        assert_eq!(mapping.interfaces.len(), 1);
        assert_eq!(mapping.interfaces[0].peers.len(), 1);
        assert!(persisted.contains("catalog/client-a"));
        assert!(!persisted.contains("must-not-leak"));
        assert!(!persisted.contains("allowed_ips"));
    }

    #[test]
    fn sidecar_runtime_config_omits_hysteria2_payload_when_material_is_incomplete() {
        let inbound = GeneratedInbound {
            tag: "hy2-in".to_string(),
            port: 8443,
            protocol: "hysteria2".to_string(),
            network: "udp".to_string(),
            tls_enabled: true,
        };
        let runtime_config = runtime_with_generated_inbounds(
            vec![inbound.clone()],
            vec![runtime_user(
                "alice",
                &inbound,
                "hysteria2",
                r#"{"inbound":"hy2-in","password":"hy2-secret"}"#,
            )],
        );

        let sidecar_config = build_sidecar_runtime_config_document(&runtime_config);

        assert!(sidecar_config.hysteria2_configs.is_empty());
        assert_eq!(sidecar_config.requirements.len(), 1);
        assert_eq!(
            sidecar_config.requirements[0].status,
            SidecarRuntimeRequirementStatus::Blocked
        );
    }

    #[test]
    fn sidecar_executor_session_groups_requirements_with_unique_envelopes() {
        let dir = temp_test_dir("sidecar-session-groups-requirements");
        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir).build();
        runtime.state.last_sidecar_runtime_summary = Some(SidecarRuntimeSummary {
            schema_version: 1,
            source_revision: "rev-sidecar".to_string(),
            requirement_count: 2,
            blocked_count: 2,
            created_at_unix: 1,
        });
        runtime.state.last_runtime_protocol_requirements = vec![
            RuntimeProtocolRequirement {
                protocol: RuntimeProtocolKind::Hysteria2,
                required_component: RuntimeComponentKind::Hysteria2,
                source: "generated_inbound".to_string(),
                source_ref: "hy2-a".to_string(),
            },
            RuntimeProtocolRequirement {
                protocol: RuntimeProtocolKind::Hysteria2,
                required_component: RuntimeComponentKind::Hysteria2,
                source: "generated_inbound".to_string(),
                source_ref: "hy2-b".to_string(),
            },
        ];

        let session = runtime.sidecar_executor_session();
        let report = runtime.snapshot().runtime_validation_report;

        fs::remove_dir_all(dir).ok();
        assert_eq!(session.source_revision.as_deref(), Some("rev-sidecar"));
        assert_eq!(session.requirement_count, 2);
        assert_eq!(session.envelope_count, 6);
        assert!(!session.executable);
        assert!(session.fail_closed);
        assert_eq!(
            session.acceptance.required_command_ids.len(),
            session.envelope_count
        );
        let mut command_ids = session.acceptance.required_command_ids.clone();
        command_ids.sort();
        command_ids.dedup();
        assert_eq!(command_ids.len(), session.envelope_count);
        assert!(
            session
                .acceptance
                .required_command_ids
                .contains(&"hysteria2:hy2-a:start:placeholder".to_string())
        );
        assert_eq!(report.sidecar_runtime.executor_session.envelope_count, 6);
        assert_eq!(
            report.sidecar_runtime.executor_session.session_id,
            session.session_id
        );
    }

    #[test]
    fn sidecar_executor_session_marks_configured_envelopes_executable() {
        let dir = temp_test_dir("sidecar-session-configured-envelopes");
        let command = fake_success_binary(&dir);
        let binary = fake_version_binary(&dir, "hysteria", "hysteria version v2.6.0");
        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir).build();
        runtime.config.hysteria2_binary_path = Some(binary.to_string_lossy().to_string());
        runtime.config.hysteria2_start_args = vec![command.to_string_lossy().to_string()];
        runtime.config.hysteria2_status_args = vec![command.to_string_lossy().to_string()];
        let config_path = sidecar_generated_config_path(
            &runtime.config.local_sidecar_runtime_config_path,
            LocalSidecarKind::Hysteria2,
            "hy2-a",
        );
        write_secret_file_if_changed(&config_path, b"listen: :8443\n").unwrap();
        runtime.state.last_sidecar_runtime_summary = Some(SidecarRuntimeSummary {
            schema_version: 1,
            source_revision: "rev-sidecar".to_string(),
            requirement_count: 1,
            blocked_count: 1,
            created_at_unix: 1,
        });
        runtime.state.last_runtime_protocol_requirements = vec![RuntimeProtocolRequirement {
            protocol: RuntimeProtocolKind::Hysteria2,
            required_component: RuntimeComponentKind::Hysteria2,
            source: "generated_inbound".to_string(),
            source_ref: "hy2-a".to_string(),
        }];

        let session = runtime.sidecar_executor_session();

        fs::remove_dir_all(dir).ok();
        assert!(session.executable);
        assert!(session.envelopes.iter().all(|envelope| {
            envelope.config_exists
                && envelope
                    .config_path
                    .as_deref()
                    .is_some_and(|path| path.ends_with("hy2-a.yaml"))
        }));
        assert!(session.envelopes.iter().any(|envelope| {
            envelope.action == LocalSidecarAction::Start && !envelope.plan.dry_run
        }));
        assert!(session.envelopes.iter().any(|envelope| {
            envelope.action == LocalSidecarAction::Status && !envelope.plan.dry_run
        }));
        assert!(session.envelopes.iter().any(|envelope| {
            envelope.action == LocalSidecarAction::Validate && !envelope.plan.dry_run
        }));
    }

    #[test]
    fn standard_hysteria2_recipe_builds_service_lifecycle_argv() {
        let start =
            standard_hysteria2_action_args(LocalSidecarAction::Start, "hydra-hysteria2.service");
        if OS == "linux" {
            assert_eq!(
                start,
                Some(vec![
                    "systemctl".to_string(),
                    "start".to_string(),
                    "hydra-hysteria2.service".to_string()
                ])
            );
            let logs =
                standard_hysteria2_action_args(LocalSidecarAction::Logs, "hydra-hysteria2.service")
                    .expect("linux logs recipe exists");
            assert_eq!(logs.first().map(String::as_str), Some("journalctl"));
            assert!(logs.contains(&"hydra-hysteria2.service".to_string()));
        }
        assert!(standard_hysteria2_action_args(LocalSidecarAction::Start, "bad;service").is_none());
        assert!(
            standard_hysteria2_action_args(LocalSidecarAction::Install, "hydra.service").is_none()
        );
    }

    #[test]
    fn standard_wireguard_recipe_uses_generated_config_path() {
        let dir = temp_test_dir("wireguard-standard-recipe");
        let config_path = dir.join("wg0.conf");
        let start = standard_wireguard_action_args(
            LocalSidecarAction::Start,
            Some("/usr/bin/wg"),
            Some("/usr/bin/wg-quick"),
            "hydra-wg0",
            Some(&config_path),
        )
        .unwrap();
        assert_eq!(
            start,
            vec![
                "/usr/bin/wg-quick".to_string(),
                "up".to_string(),
                config_path.to_string_lossy().to_string()
            ]
        );
        let validate = standard_wireguard_action_args(
            LocalSidecarAction::Validate,
            Some("/usr/bin/wg"),
            Some("/usr/bin/wg-quick"),
            "hydra-wg0",
            Some(&config_path),
        )
        .unwrap();
        assert_eq!(validate[1], "strip");
        assert!(
            standard_wireguard_action_args(
                LocalSidecarAction::Status,
                Some("/usr/bin/wg"),
                Some("/usr/bin/wg-quick"),
                "bad;if",
                None,
            )
            .is_none()
        );
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn standard_recipe_session_is_executable_for_wireguard_requirement() {
        let dir = temp_test_dir("wireguard-standard-session");
        let wg = fake_version_binary(&dir, "wg", "wireguard-tools v1.0.20210914");
        let wg_quick = fake_version_binary(&dir, "wg-quick", "wg-quick helper");
        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir).build();
        runtime.config.sidecar_recipe_mode = "standard".to_string();
        runtime.config.wireguard_binary_path = Some(wg.to_string_lossy().to_string());
        runtime.config.wg_quick_binary_path = Some(wg_quick.to_string_lossy().to_string());
        let config_path = sidecar_generated_config_path(
            &runtime.config.local_sidecar_runtime_config_path,
            LocalSidecarKind::WireGuard,
            "wg-a",
        );
        write_secret_file_if_changed(&config_path, b"[Interface]\n").unwrap();
        runtime.state.last_sidecar_runtime_summary = Some(SidecarRuntimeSummary {
            schema_version: 1,
            source_revision: "rev-wireguard-standard".to_string(),
            requirement_count: 1,
            blocked_count: 1,
            created_at_unix: 1,
        });
        runtime.state.last_runtime_protocol_requirements = vec![RuntimeProtocolRequirement {
            protocol: RuntimeProtocolKind::WireGuard,
            required_component: RuntimeComponentKind::WireGuard,
            source: "generated_inbound".to_string(),
            source_ref: "wg-a".to_string(),
        }];

        let session = runtime.sidecar_executor_session();

        fs::remove_dir_all(dir).ok();
        assert!(session.executable);
        assert_eq!(session.envelope_count, 3);
        assert!(session.envelopes.iter().all(|envelope| {
            envelope.config_exists
                && !envelope.plan.dry_run
                && envelope
                    .acceptance
                    .required_checks
                    .iter()
                    .any(|check| check.contains("argv is configured"))
        }));
        assert!(session.envelopes.iter().any(|envelope| {
            envelope.action == LocalSidecarAction::Start
                && envelope
                    .plan
                    .steps
                    .iter()
                    .any(|step| step.contains("wg-quick"))
        }));
    }

    #[test]
    fn sidecar_executor_session_result_accepts_complete_matching_results() {
        let dir = temp_test_dir("sidecar-session-result-accepted");
        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir).build();
        let config_path = sidecar_generated_config_path(
            &runtime.config.local_sidecar_runtime_config_path,
            LocalSidecarKind::WireGuard,
            "wg-a",
        );
        write_secret_file_if_changed(&config_path, b"[Interface]\n").unwrap();
        runtime.state.last_sidecar_runtime_summary = Some(SidecarRuntimeSummary {
            schema_version: 1,
            source_revision: "rev-sidecar".to_string(),
            requirement_count: 1,
            blocked_count: 1,
            created_at_unix: 1,
        });
        runtime.state.last_runtime_protocol_requirements = vec![RuntimeProtocolRequirement {
            protocol: RuntimeProtocolKind::WireGuard,
            required_component: RuntimeComponentKind::WireGuard,
            source: "generated_inbound".to_string(),
            source_ref: "wg-a".to_string(),
        }];
        let session = runtime.sidecar_executor_session();
        let results = session
            .envelopes
            .iter()
            .map(|envelope| LocalSidecarExecutorResultRequest {
                command_id: envelope.command_id.clone(),
                status: envelope.acceptance.expected_status,
                completed_checks: envelope.acceptance.required_checks.clone(),
                exit_code: Some(0),
                detail: Some("session envelope accepted".to_string()),
                completed_at_unix: Some(123),
            })
            .collect();

        let response = runtime
            .complete_sidecar_executor_session(SidecarExecutorSessionResultRequest {
                session_id: session.session_id,
                results,
            })
            .unwrap();

        fs::remove_dir_all(dir).ok();
        assert!(response.accepted);
        assert_eq!(response.expected_envelope_count, 3);
        assert_eq!(response.accepted_count, 3);
        assert!(response.failed_checks.is_empty());
    }

    #[test]
    fn sidecar_executor_session_result_rejects_incomplete_results_fail_closed() {
        let dir = temp_test_dir("sidecar-session-result-rejected");
        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir).build();
        runtime.state.last_sidecar_runtime_summary = Some(SidecarRuntimeSummary {
            schema_version: 1,
            source_revision: "rev-sidecar".to_string(),
            requirement_count: 1,
            blocked_count: 1,
            created_at_unix: 1,
        });
        runtime.state.last_runtime_protocol_requirements = vec![RuntimeProtocolRequirement {
            protocol: RuntimeProtocolKind::Hysteria2,
            required_component: RuntimeComponentKind::Hysteria2,
            source: "generated_inbound".to_string(),
            source_ref: "hy2-a".to_string(),
        }];
        let session = runtime.sidecar_executor_session();

        let response = runtime
            .complete_sidecar_executor_session(SidecarExecutorSessionResultRequest {
                session_id: session.session_id,
                results: Vec::new(),
            })
            .unwrap();

        assert!(!response.accepted);
        assert_eq!(response.expected_envelope_count, 3);
        assert!(
            response
                .failed_checks
                .iter()
                .any(|check| check.contains("result count mismatch"))
        );
        let sidecar = runtime
            .snapshot()
            .sidecars
            .into_iter()
            .find(|sidecar| sidecar.sidecar == LocalSidecarKind::Hysteria2)
            .unwrap();
        fs::remove_dir_all(dir).ok();
        assert_eq!(sidecar.status, LocalSidecarStatus::Failed);
        assert!(
            sidecar
                .last_detail
                .as_deref()
                .unwrap()
                .contains("rejected fail-closed")
        );
    }

    #[test]
    fn sidecar_runtime_requirement_becomes_ready_after_matching_session_acceptance() {
        let dir = temp_test_dir("sidecar-requirement-ready-after-session");
        let binary = fake_version_binary(&dir, "hysteria", "hysteria version v2.6.0");
        let command = fake_success_binary(&dir);
        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir).build();
        runtime.config.hysteria2_binary_path = Some(binary.to_string_lossy().to_string());
        runtime.config.hysteria2_start_args = vec![command.to_string_lossy().to_string()];
        runtime.config.hysteria2_status_args = vec![command.to_string_lossy().to_string()];
        let cert = dir.join("hy2.crt");
        let key = dir.join("hy2.key");
        fs::write(&cert, "CERT").unwrap();
        fs::write(&key, "KEY").unwrap();
        let inbound = GeneratedInbound {
            tag: "hy2-a".to_string(),
            port: 8443,
            protocol: "hysteria2".to_string(),
            network: "udp".to_string(),
            tls_enabled: true,
        };
        let settings = serde_json::json!({
            "inbound": "hy2-a",
            "password": "secret",
            "tls_certificate_file": cert,
            "tls_key_file": key
        })
        .to_string();
        let runtime_config = runtime_with_generated_inbounds(
            vec![inbound.clone()],
            vec![runtime_user("alice", &inbound, "hysteria2", &settings)],
        );
        persist_node_runtime_config(&runtime.config.local_runtime_config_path, &runtime_config)
            .unwrap();
        let sidecar_config = build_sidecar_runtime_config_document(&runtime_config);
        persist_sidecar_generated_config_files(
            &runtime.config.local_sidecar_runtime_config_path,
            &sidecar_config,
        )
        .unwrap();
        runtime.state.last_runtime_protocol_requirements =
            runtime_config.required_protocols.clone();
        runtime.state.last_sidecar_runtime_summary =
            Some(summarize_sidecar_runtime_config(&sidecar_config));
        let session = runtime.sidecar_executor_session();
        runtime.state.sidecar_states.push(PersistedSidecarState {
            sidecar: LocalSidecarKind::Hysteria2,
            status: LocalSidecarStatus::Running,
            supported: true,
            binary_path: runtime.config.hysteria2_binary_path.clone(),
            detected_version: Some("hysteria version v2.6.0".to_string()),
            last_action: Some(LocalSidecarAction::Start),
            last_detail: Some("running".to_string()),
            last_validated_at_unix: Some(123),
            updated_at_unix: Some(123),
            logs: Vec::new(),
        });
        runtime.state.last_accepted_sidecar_executor_session =
            Some(PersistedSidecarExecutorSessionAcceptance {
                session_id: session.session_id,
                source_revision: session.source_revision,
                accepted_at_unix: 123,
                command_ids: session.acceptance.required_command_ids,
            });

        let report = runtime.snapshot().runtime_validation_report;

        fs::remove_dir_all(dir).ok();
        assert!(report.sidecar_runtime.ready);
        assert_eq!(report.sidecar_runtime.blocked_count, 0);
        assert_eq!(
            report.sidecar_runtime.requirements[0].status,
            SidecarRuntimeRequirementStatus::Ready
        );
        assert_eq!(
            report.required_protocols[0].readiness,
            RuntimeProtocolReadiness::Ready
        );
    }

    #[test]
    fn xray_renderer_omits_sidecar_inbounds_and_reports_issue() {
        let runtime_config = NodeRuntimeConfigDocument {
            schema_version: 1,
            node_id: Some("node-a".to_string()),
            source_revision: "rev-a".to_string(),
            source_generated_at_unix: 1,
            created_at_unix: 2,
            source_user_count: 0,
            source_node_count: 1,
            users: Vec::new(),
            inbounds: vec![
                GeneratedInbound {
                    tag: "vless-in".to_string(),
                    port: 10001,
                    protocol: "vless".to_string(),
                    network: "ws".to_string(),
                    tls_enabled: false,
                },
                GeneratedInbound {
                    tag: "hy2-in".to_string(),
                    port: 10002,
                    protocol: "hysteria2".to_string(),
                    network: "udp".to_string(),
                    tls_enabled: true,
                },
            ],
            hosts: Vec::new(),
            cluster_intents: Vec::new(),
            route_assignments: Vec::new(),
            required_protocols: vec![
                RuntimeProtocolRequirement {
                    protocol: RuntimeProtocolKind::VlessTlsWebSocket,
                    required_component: RuntimeComponentKind::Xray,
                    source: "generated_inbound".to_string(),
                    source_ref: "vless-in".to_string(),
                },
                RuntimeProtocolRequirement {
                    protocol: RuntimeProtocolKind::Hysteria2,
                    required_component: RuntimeComponentKind::Hysteria2,
                    source: "generated_inbound".to_string(),
                    source_ref: "hy2-in".to_string(),
                },
            ],
        };

        let plan = render_xray_config(&runtime_config, &RouteCredentialStore::default(), None);

        let inbounds = plan.config["inbounds"].as_array().unwrap();
        assert!(inbounds.is_empty());
        assert!(
            plan.feature_flags
                .iter()
                .any(|flag| { flag == "generated-inbound-material-pending-fail-closed" })
        );
        assert!(summarize_xray_render_plan(&plan).fail_closed);
        assert!(plan.issues.iter().any(|issue| {
            issue.route_id == "vless-in"
                && issue.reason == "generated_inbound_client_material_missing"
        }));
        assert!(plan.issues.iter().any(|issue| {
            issue.route_id == "hy2-in" && issue.reason == "non_xray_protocol_requires_sidecar"
        }));
    }

    #[test]
    fn xray_renderer_emits_generated_inbounds_with_explicit_client_material() {
        let inbounds = vec![GeneratedInbound {
            tag: "vless-in".to_string(),
            port: 10001,
            protocol: "vless".to_string(),
            network: "ws".to_string(),
            tls_enabled: false,
        }];
        let users = vec![runtime_user(
            "alice",
            &inbounds[0],
            "vless",
            r#"{"inbound":"vless-in","uuid":"11111111-1111-5111-8111-111111111111"}"#,
        )];
        let runtime_config = runtime_with_generated_inbounds(inbounds, users);

        let plan = render_xray_config(&runtime_config, &RouteCredentialStore::default(), None);
        let summary = summarize_xray_render_plan(&plan);

        assert!(!summary.fail_closed);
        assert!(plan.issues.is_empty());
        let rendered = plan.config["inbounds"].as_array().unwrap();
        assert_eq!(rendered.len(), 1);
        assert_eq!(rendered[0]["protocol"], "vless");
        assert_eq!(rendered[0]["settings"]["clients"][0]["email"], "alice");
        assert_eq!(rendered[0]["settings"]["decryption"], "none");
    }

    #[test]
    fn xray_renderer_keeps_all_device_credentials_and_runtime_principals() {
        let inbound = GeneratedInbound {
            tag: "vless-in".to_string(),
            port: 10001,
            protocol: "vless".to_string(),
            network: "tcp".to_string(),
            tls_enabled: false,
        };
        let mut user = runtime_user(
            "catalog/client-a",
            &inbound,
            "vless",
            r#"{"inbound":"vless-in","id":"11111111-1111-4111-8111-111111111111","runtime_username":"catalog/client-a/device/device-a"}"#,
        );
        user.proxy_profiles.push(GeneratedProxyProfile {
            id: "client-a-device-b-vless".to_string(),
            name: "device b".to_string(),
            proxy_type: "vless".to_string(),
            settings_json: r#"{"inbound":"vless-in","id":"22222222-2222-4222-8222-222222222222","runtime_username":"catalog/client-a/device/device-b"}"#.to_string(),
        });
        let runtime_config = runtime_with_generated_inbounds(vec![inbound], vec![user]);

        let plan = render_xray_config(&runtime_config, &RouteCredentialStore::default(), None);

        assert!(plan.issues.is_empty());
        let clients = plan.config["inbounds"][0]["settings"]["clients"]
            .as_array()
            .expect("VLESS clients are rendered");
        assert_eq!(clients.len(), 2);
        assert_eq!(clients[0]["email"], "catalog/client-a/device/device-a");
        assert_eq!(clients[1]["email"], "catalog/client-a/device/device-b");
        assert_ne!(clients[0]["id"], clients[1]["id"]);
    }

    #[test]
    fn real_xray_accepts_generated_production_protocol_documents_when_configured() {
        let configured = std::env::var("HYDRA_TEST_XRAY_BINARY")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);

        let Some(xray_binary) = configured else {
            // On a developer machine the test stays optional, but skipping is
            // forbidden in CI: that silent skip is what let the removed
            // `allowInsecure` flag and the missing Reality renderer survive a
            // green run.
            assert!(
                !xray_integration_test_is_required(),
                "HYDRA_TEST_XRAY_BINARY is unset while HYDRA_REQUIRE_XRAY_TEST=1: \
                 the check against a real Xray is mandatory in CI"
            );
            eprintln!("skipping real Xray compatibility test: HYDRA_TEST_XRAY_BINARY is not set");
            return;
        };
        assert!(
            xray_binary.is_file(),
            "HYDRA_TEST_XRAY_BINARY must point to an existing Xray binary: {}",
            xray_binary.display()
        );
        assert_xray_binary_is_supported(&xray_binary);

        // The protocol list under test is derived from the runtime classification
        // rather than hardcoded. A protocol declared as served but never checked
        // against the binary is exactly what produced the Shadowsocks defect: the
        // panel forced AEAD-2022, the node rendered it without a PSK, and the test
        // skipped silently.
        assert_eq!(
            xray_backed_protocols(),
            vec!["vless"],
            "the set of Xray-backed protocols changed: the fixture must cover every \
             one of them, or declared support drifts from the binary again"
        );

        let dir = real_xray_test_dir(&xray_binary);
        let (vless_cert, vless_key) = write_test_tls_material(&dir, "vless-ws");

        let inbounds = vec![GeneratedInbound {
            tag: "vless-ws-in".to_string(),
            port: 11001,
            protocol: "vless".to_string(),
            network: "ws".to_string(),
            tls_enabled: true,
        }];

        let users = vec![runtime_user(
            "alice",
            &inbounds[0],
            "vless",
            &serde_json::json!({
                "inbound": "vless-ws-in",
                "uuid": "11111111-1111-5111-8111-111111111111",
                "tls_certificate_file": vless_cert,
                "tls_key_file": vless_key,
                "path": "/ws"
            })
            .to_string(),
        )];
        let runtime_config = runtime_with_generated_inbounds(inbounds, users);
        let plan = render_xray_config_with_stats(
            &runtime_config,
            &RouteCredentialStore::default(),
            None,
            Some("127.0.0.1:10085"),
        );
        let summary = summarize_xray_render_plan(&plan);
        assert!(!summary.fail_closed);
        assert!(plan.issues.is_empty());
        let rendered = plan.config["inbounds"].as_array().unwrap();
        assert_eq!(rendered[0]["protocol"], "vless");
        assert_eq!(rendered[0]["streamSettings"]["network"], "ws");
        assert_eq!(rendered[0]["streamSettings"]["security"], "tls");
        assert_eq!(rendered[1]["tag"], "hydra-stats-api");

        let config_path = dir.join("xray.json");
        let mut xray_config = plan.config.clone();
        rewrite_xray_json_paths_for_process(&mut xray_config, &xray_binary);
        fs::write(
            &config_path,
            serde_json::to_vec_pretty(&xray_config).unwrap(),
        )
        .unwrap();
        let output = std::process::Command::new(&xray_binary)
            .args(["run", "-test", "-config"])
            .arg(xray_process_path(&config_path, &xray_binary))
            .output()
            .unwrap_or_else(|error| {
                panic!(
                    "failed to execute Xray binary {}: {error}",
                    xray_binary.display()
                )
            });
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        fs::remove_dir_all(dir).ok();

        assert!(
            output.status.success(),
            "real Xray rejected generated protocol document with status {:?}\nstdout:\n{}\nstderr:\n{}",
            output.status.code(),
            stdout,
            stderr
        );
    }

    #[test]
    fn xray_renderer_blocks_generated_tls_inbound_without_tls_material() {
        let inbound = GeneratedInbound {
            tag: "vless-tls-in".to_string(),
            port: 10001,
            protocol: "vless".to_string(),
            network: "ws".to_string(),
            tls_enabled: true,
        };
        let runtime_config = runtime_with_generated_inbounds(
            vec![inbound.clone()],
            vec![runtime_user(
                "alice",
                &inbound,
                "vless",
                r#"{"inbound":"vless-tls-in","id":"11111111-1111-5111-8111-111111111111"}"#,
            )],
        );

        let plan = render_xray_config(&runtime_config, &RouteCredentialStore::default(), None);

        assert!(plan.config["inbounds"].as_array().unwrap().is_empty());
        assert!(summarize_xray_render_plan(&plan).fail_closed);
        assert!(plan.issues.iter().any(|issue| {
            issue.route_id == "vless-tls-in"
                && issue.reason == "generated_inbound_tls_material_missing"
        }));
    }

    #[test]
    fn xray_renderer_emits_generated_tls_inbound_with_explicit_tls_material() {
        let dir = temp_test_dir("generated-tls-inbound-material");
        let certificate_file = dir.join("server.crt");
        let key_file = dir.join("server.key");
        fs::write(&certificate_file, "CERT").unwrap();
        fs::write(&key_file, "KEY").unwrap();
        let inbound = GeneratedInbound {
            tag: "vless-tls-in".to_string(),
            port: 10001,
            protocol: "vless".to_string(),
            network: "ws".to_string(),
            tls_enabled: true,
        };
        let settings = serde_json::json!({
            "inbound": "vless-tls-in",
            "id": "11111111-1111-5111-8111-111111111111",
            "tls_certificate_file": certificate_file,
            "tls_key_file": key_file
        })
        .to_string();
        let runtime_config = runtime_with_generated_inbounds(
            vec![inbound.clone()],
            vec![runtime_user("alice", &inbound, "vless", &settings)],
        );

        let plan = render_xray_config(&runtime_config, &RouteCredentialStore::default(), None);
        let summary = summarize_xray_render_plan(&plan);

        fs::remove_dir_all(dir).ok();
        assert!(!summary.fail_closed);
        assert!(plan.issues.is_empty());
        let rendered = plan.config["inbounds"].as_array().unwrap();
        assert_eq!(rendered.len(), 1);
        assert_eq!(rendered[0]["streamSettings"]["security"], "tls");
        assert_eq!(
            rendered[0]["streamSettings"]["tlsSettings"]["certificates"][0]["certificateFile"],
            certificate_file.to_string_lossy().as_ref()
        );
        assert_eq!(
            rendered[0]["streamSettings"]["tlsSettings"]["certificates"][0]["keyFile"],
            key_file.to_string_lossy().as_ref()
        );
    }

    #[test]
    fn xray_renderer_blocks_invalid_generated_profile_settings_json() {
        let inbound = GeneratedInbound {
            tag: "vless-in".to_string(),
            port: 10001,
            protocol: "vless".to_string(),
            network: "ws".to_string(),
            tls_enabled: false,
        };
        let runtime_config = runtime_with_generated_inbounds(
            vec![inbound.clone()],
            vec![runtime_user("alice", &inbound, "vless", "{not-json")],
        );

        let plan = render_xray_config(&runtime_config, &RouteCredentialStore::default(), None);

        assert!(plan.config["inbounds"].as_array().unwrap().is_empty());
        assert!(summarize_xray_render_plan(&plan).fail_closed);
        assert!(plan.issues.iter().any(|issue| {
            issue.route_id == "vless-in"
                && issue.reason == "generated_inbound_profile_settings_invalid"
        }));
    }

    #[test]
    fn xray_stats_parser_aggregates_uplink_and_downlink_per_device_principal() {
        let counters = parse_xray_principal_counters(
            br#"{
                "stat": [
                    {
                        "name": "user>>>catalog/client-a/device/phone>>>traffic>>>uplink",
                        "value": "12"
                    },
                    {
                        "name": "user>>>catalog/client-a/device/phone>>>traffic>>>downlink",
                        "value": 30
                    },
                    {
                        "name": "inbound>>>ignored>>>traffic>>>uplink",
                        "value": "99"
                    }
                ]
            }"#,
        )
        .expect("Xray stats response is parsed");

        assert_eq!(counters.get("catalog/client-a/device/phone"), Some(&42));
        assert_eq!(counters.len(), 1);
    }

    #[test]
    fn xray_renderer_exposes_stats_service_only_on_loopback() {
        let runtime_config = runtime_with_generated_inbounds(Vec::new(), Vec::new());
        let plan = render_xray_config_with_stats(
            &runtime_config,
            &RouteCredentialStore::default(),
            None,
            Some("127.0.0.1:10085"),
        );

        assert_eq!(plan.config["api"]["services"][0], "StatsService");
        assert_eq!(plan.config["inbounds"][0]["listen"], "127.0.0.1");
        assert_eq!(plan.config["inbounds"][0]["port"], 10085);
        assert_eq!(
            plan.config["policy"]["levels"]["0"]["statsUserUplink"],
            true
        );
        assert!(
            plan.feature_flags
                .iter()
                .any(|flag| flag == "xray-user-traffic-stats")
        );

        let rejected = render_xray_config_with_stats(
            &runtime_config,
            &RouteCredentialStore::default(),
            None,
            Some("0.0.0.0:10085"),
        );
        assert!(rejected.config.get("api").is_none());
    }

    #[test]
    fn runtime_activity_snapshot_is_observation_only_and_device_scoped() {
        let dir = temp_test_dir("runtime-activity-snapshot");
        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir).build();
        let now = now_unix();
        runtime.state.last_runtime_activity_collected_at_unix = Some(now);
        runtime.runtime_activity.enabled = true;
        runtime
            .runtime_activity
            .xray_last_activity
            .insert("catalog/client-a/device/phone".to_string(), now);
        runtime
            .runtime_activity
            .hysteria2_online
            .insert("catalog/client-a/device/laptop".to_string(), 2);

        let snapshot = runtime
            .runtime_activity_snapshot()
            .expect("runtime activity snapshot is available");

        assert!(snapshot.runtime_capabilities.is_empty());
        assert_eq!(snapshot.observations.len(), 2);
        assert!(
            snapshot
                .observations
                .iter()
                .all(|observation| observation.runtime_session_ref.is_none())
        );
        assert!(snapshot.observations.iter().any(|observation| {
            observation.runtime_username == "catalog/client-a/device/phone"
        }));
        assert!(snapshot.observations.iter().any(|observation| {
            observation.runtime_username == "catalog/client-a/device/laptop"
        }));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn tick_failure_backoff_is_bounded_and_resets_to_poll_interval_on_success_state() {
        let dir = temp_test_dir("tick-backoff");
        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir).build();
        runtime.config.poll_interval_seconds = 15;
        runtime.config.tick_failure_backoff_base_seconds = 5;
        runtime.config.tick_failure_backoff_max_seconds = 60;

        assert_eq!(runtime.next_poll_delay(), Duration::from_secs(15));

        runtime.state.consecutive_tick_failures = 1;
        assert_eq!(runtime.next_poll_delay(), Duration::from_secs(15));

        runtime.state.consecutive_tick_failures = 3;
        assert_eq!(runtime.next_poll_delay(), Duration::from_secs(20));

        runtime.state.consecutive_tick_failures = 10;
        assert_eq!(runtime.next_poll_delay(), Duration::from_secs(60));

        runtime.state.consecutive_tick_failures = 0;
        assert_eq!(runtime.next_poll_delay(), Duration::from_secs(15));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn snapshot_does_not_advertise_exact_session_termination_without_an_adapter() {
        let dir = temp_test_dir("subscription-session-adapter-status");
        let (runtime, dir) = TestRuntimeBuilder::from_dir(dir).build();

        let adapter = runtime.snapshot().subscription_session_adapter;

        assert_eq!(
            adapter.status,
            SubscriptionSessionAdapterStatus::Unsupported
        );
        assert!(!adapter.exact_session_termination_ready);
        assert!(adapter.observation_source.is_none());
        assert!(adapter.runtime_capabilities.is_empty());
        assert!(
            adapter
                .disabled_reason
                .as_deref()
                .unwrap()
                .contains("not exact per-session enforcement")
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn snapshot_exposes_runtime_artifact_manifest_without_reading_secrets() {
        let dir = temp_test_dir("runtime-artifact-manifest-empty");
        let (runtime, dir) = TestRuntimeBuilder::from_dir(dir).build();

        let snapshot = runtime.snapshot();

        assert_eq!(snapshot.runtime_artifacts.len(), 9);
        let xray = snapshot
            .runtime_artifacts
            .iter()
            .find(|artifact| artifact.kind == RuntimeArtifactKind::XrayConfig)
            .unwrap();
        assert_eq!(xray.path, runtime.config.local_xray_config_path);
        assert!(!xray.exists);
        assert!(xray.executable_runtime_input);
        assert!(!xray.secret_sensitive);
        let route_manifest = snapshot
            .runtime_artifacts
            .iter()
            .find(|artifact| artifact.kind == RuntimeArtifactKind::RouteCredentialManifest)
            .unwrap();
        assert_eq!(route_manifest.path, runtime.config.route_credentials_path);
        assert!(route_manifest.secret_sensitive);
        assert!(!route_manifest.executable_runtime_input);
        let hysteria_dir = snapshot
            .runtime_artifacts
            .iter()
            .find(|artifact| artifact.kind == RuntimeArtifactKind::Hysteria2ConfigDirectory)
            .unwrap();
        assert!(hysteria_dir.secret_sensitive);
        assert!(hysteria_dir.executable_runtime_input);
        let wireguard_dir = snapshot
            .runtime_artifacts
            .iter()
            .find(|artifact| artifact.kind == RuntimeArtifactKind::WireGuardConfigDirectory)
            .unwrap();
        assert!(wireguard_dir.secret_sensitive);
        assert!(wireguard_dir.executable_runtime_input);
        let wireguard_mapping = snapshot
            .runtime_artifacts
            .iter()
            .find(|artifact| artifact.kind == RuntimeArtifactKind::WireGuardSessionMapping)
            .unwrap();
        assert!(wireguard_mapping.secret_sensitive);
        assert!(wireguard_mapping.executable_runtime_input);

        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn observation_only_adapter_accepts_bounded_snapshot_without_exact_handles() {
        let dir = temp_test_dir("subscription-session-observation-only");
        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir).build();
        runtime.config.subscription_session_adapter_token = Some("adapter-token".to_string());
        runtime
            .register_subscription_session_adapter(RegisterLocalSubscriptionSessionAdapterRequest {
                adapter_instance_id: "adapter-a".to_string(),
                runtime_capabilities: Vec::new(),
            })
            .await
            .unwrap();
        assert!(
            runtime
                .register_subscription_session_adapter(
                    RegisterLocalSubscriptionSessionAdapterRequest {
                        adapter_instance_id: "adapter-b".to_string(),
                        runtime_capabilities: Vec::new(),
                    },
                )
                .await
                .is_err()
        );

        let view = runtime
            .stage_subscription_session_observations(
                "adapter-a",
                ReportSubscriptionSessionsRequest {
                    observation_source:
                        SubscriptionSessionObservationSource::NodeManagedRuntimeTable,
                    runtime_capabilities: Vec::new(),
                    observations: vec![SubscriptionSessionObservation {
                        session_id: "session-a".to_string(),
                        runtime_username: "catalog/client-a".to_string(),
                        runtime_session_ref: None,
                        device_fingerprint: Some("fingerprint-a".to_string()),
                        source_ip: Some("198.51.100.1".to_string()),
                        connected_at_unix: Some(1),
                    }],
                },
            )
            .unwrap();

        assert_eq!(
            view.status,
            SubscriptionSessionAdapterStatus::ObservationOnly
        );
        assert_eq!(view.buffered_observation_count, 1);
        assert!(!view.exact_session_termination_ready);
        assert!(view.runtime_capabilities.is_empty());

        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn observation_only_adapter_rejects_exact_session_claims() {
        let dir = temp_test_dir("subscription-session-exact-rejected");
        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir).build();
        runtime
            .register_subscription_session_adapter(RegisterLocalSubscriptionSessionAdapterRequest {
                adapter_instance_id: "adapter-a".to_string(),
                runtime_capabilities: Vec::new(),
            })
            .await
            .unwrap();

        let with_capability = runtime.stage_subscription_session_observations(
            "adapter-a",
            ReportSubscriptionSessionsRequest {
                observation_source: SubscriptionSessionObservationSource::NodeManagedRuntimeTable,
                runtime_capabilities: vec![
                    SubscriptionSessionRuntimeCapability::ExactSessionTermination,
                ],
                observations: Vec::new(),
            },
        );
        assert!(with_capability.is_err());

        let with_handle = runtime.stage_subscription_session_observations(
            "adapter-a",
            ReportSubscriptionSessionsRequest {
                observation_source: SubscriptionSessionObservationSource::NodeManagedRuntimeTable,
                runtime_capabilities: Vec::new(),
                observations: vec![SubscriptionSessionObservation {
                    session_id: "session-a".to_string(),
                    runtime_username: "catalog/client-a".to_string(),
                    runtime_session_ref: Some("runtime-ref-a".to_string()),
                    device_fingerprint: None,
                    source_ip: None,
                    connected_at_unix: None,
                }],
            },
        );
        assert!(with_handle.is_err());

        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn failed_runtime_apply_does_not_mark_revision_applied() {
        let dir = temp_test_dir("runtime-fail");
        let fake_xray = fake_success_binary(&dir);
        let xray_config_path = dir.join("xray.json");
        fs::write(&xray_config_path, "{\"old\":true}").unwrap();
        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir)
            .external_process(fake_xray)
            .applied_revision("old-rev")
            .build();
        fs::write(
            runtime.config.local_xray_config_path.as_str(),
            "{\"old\":true}",
        )
        .unwrap();

        let error = runtime
            .apply_config(&empty_generated_response("new-rev"))
            .await
            .expect_err("runtime start must fail without run args");

        assert!(error.to_string().contains("HYDRA_NODE_XRAY_RUN_ARGS_JSON"));
        assert_eq!(runtime.state.applied_revision.as_deref(), Some("old-rev"));
        assert!(runtime.state.rollback_marker_path.is_some());
        assert!(runtime.state.last_xray_render_summary.is_some());
        assert_eq!(
            runtime.state.xray_runtime.status,
            Some(XrayRuntimeStatus::Failed)
        );
        assert!(
            runtime
                .state
                .last_apply_detail
                .as_deref()
                .unwrap()
                .contains("runtime apply failed")
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn failed_validation_does_not_mark_revision_applied() {
        let dir = temp_test_dir("validation-fail");
        let fake_xray = fake_failure_binary(&dir);
        let xray_config_path = dir.join("xray.json");
        fs::write(&xray_config_path, "{\"old\":true}").unwrap();
        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir)
            .external_process(fake_xray)
            .applied_revision("old-rev")
            .build();
        fs::write(
            runtime.config.local_xray_config_path.as_str(),
            "{\"old\":true}",
        )
        .unwrap();

        let error = runtime
            .apply_config(&empty_generated_response("new-rev"))
            .await
            .expect_err("external validation must fail");

        assert!(
            error
                .to_string()
                .contains("external xray validation failed")
        );
        assert_eq!(runtime.state.applied_revision.as_deref(), Some("old-rev"));
        assert!(runtime.state.rollback_marker_path.is_some());
        assert!(runtime.state.last_xray_render_summary.is_some());
        assert_eq!(
            runtime.state.xray_runtime.status,
            Some(XrayRuntimeStatus::Failed)
        );
        assert!(
            runtime
                .state
                .last_apply_detail
                .as_deref()
                .unwrap()
                .contains("validation failed")
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn rollback_success_restores_backup_and_clears_marker() {
        let dir = temp_test_dir("rollback-success");
        let xray_config_path = dir.join("xray.json");
        let backup_path = dir.join("xray.json.bak");
        let marker_path = rollback_marker_path(&xray_config_path);
        fs::write(&xray_config_path, "{\"broken\":true}").unwrap();
        fs::write(&backup_path, "{\"restored\":true}").unwrap();
        fs::write(&marker_path, "{}").unwrap();

        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir)
            .rollback_paths(backup_path.clone(), marker_path.clone())
            .build();

        let detail = runtime.rollback_last_config().unwrap();

        assert!(detail.contains("rolled back config"));
        assert_eq!(
            fs::read_to_string(&xray_config_path).unwrap(),
            "{\"restored\":true}"
        );
        assert!(runtime.state.rollback_marker_path.is_none());
        assert!(!marker_path.exists());
        assert!(
            runtime
                .state
                .last_apply_detail
                .as_deref()
                .unwrap()
                .contains("rolled back config")
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn rollback_failure_keeps_marker_and_records_detail() {
        let dir = temp_test_dir("rollback-failure");
        let fake_xray = fake_failure_binary(&dir);
        let xray_config_path = dir.join("xray.json");
        let backup_path = dir.join("xray.json.bak");
        let marker_path = rollback_marker_path(&xray_config_path);
        fs::write(&xray_config_path, "{\"broken\":true}").unwrap();
        fs::write(&backup_path, "{\"restored\":true}").unwrap();
        fs::write(&marker_path, "{}").unwrap();

        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir)
            .external_process(fake_xray)
            .rollback_paths(backup_path.clone(), marker_path.clone())
            .build();

        let error = runtime
            .rollback_last_config()
            .expect_err("rollback runtime apply must fail");

        assert!(error.to_string().contains("HYDRA_NODE_XRAY_RUN_ARGS_JSON"));
        assert_eq!(
            fs::read_to_string(&xray_config_path).unwrap(),
            "{\"restored\":true}"
        );
        assert_eq!(
            runtime.state.rollback_marker_path.as_deref(),
            Some(marker_path.to_string_lossy().as_ref())
        );
        assert!(marker_path.exists());
        assert_eq!(
            runtime.state.xray_runtime.status,
            Some(XrayRuntimeStatus::Failed)
        );
        assert!(
            runtime
                .state
                .last_apply_detail
                .as_deref()
                .unwrap()
                .contains("rollback failed")
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn xray_update_failure_is_persisted_for_operator_visibility() {
        let dir = temp_test_dir("xray-update-failure");
        let state_path = dir.join("node-state.json");
        let events_path = dir.join("runtime-events.json");
        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir).build();

        runtime
            .record_xray_update_failure("updated xray binary failed validation".to_string())
            .unwrap();

        assert_eq!(
            runtime.state.last_xray_update_status,
            Some(XrayUpdateStatus::Failed)
        );
        assert_eq!(
            runtime.state.xray_runtime.status,
            Some(XrayRuntimeStatus::Failed)
        );
        assert_eq!(
            runtime.state.xray_runtime.last_action.as_deref(),
            Some("xray_update")
        );
        assert!(
            runtime
                .state
                .last_xray_update_detail
                .as_deref()
                .unwrap()
                .contains("failed validation")
        );
        assert!(
            runtime
                .runtime_events
                .iter()
                .any(|event| event.kind == "xray_core_update_failed")
        );
        assert!(state_path.exists());
        assert!(events_path.exists());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn xray_update_phase_is_persisted_for_operator_visibility() {
        let dir = temp_test_dir("xray-update-phase");
        let state_path = dir.join("node-state.json");
        let events_path = dir.join("runtime-events.json");
        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir).build();

        runtime
            .record_xray_update_phase("download_binary", "downloading official xray asset")
            .unwrap();

        assert_eq!(
            runtime.state.last_xray_update_status,
            Some(XrayUpdateStatus::Running)
        );
        assert_eq!(
            runtime.state.last_xray_update_phase.as_deref(),
            Some("download_binary")
        );
        assert!(
            runtime
                .state
                .last_xray_update_detail
                .as_deref()
                .unwrap()
                .contains("downloading official xray asset")
        );
        assert!(
            runtime
                .runtime_events
                .iter()
                .any(|event| event.kind == "xray_core_update_phase")
        );
        assert!(state_path.exists());
        assert!(events_path.exists());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn xray_download_url_accepts_trusted_github_hosts() {
        assert!(
            validate_xray_download_url(
                "https://github.com/XTLS/Xray-core/releases/download/v1/Xray-linux-64.zip"
            )
            .is_ok()
        );
        assert!(
            validate_xray_download_url(
                "https://objects.githubusercontent.com/github-production-release-asset-2e65be/asset"
            )
            .is_ok()
        );
        assert!(
            validate_xray_download_url("https://release-assets.githubusercontent.com/github-production-release-asset-2e65be/asset")
                .is_ok()
        );
    }

    #[test]
    fn xray_download_url_rejects_non_https() {
        assert!(
            validate_xray_download_url(
                "http://github.com/XTLS/Xray-core/releases/download/v1/Xray-linux-64.zip"
            )
            .is_err()
        );
    }

    #[test]
    fn xray_download_url_rejects_untrusted_hosts() {
        assert!(
            validate_xray_download_url(
                "https://example.com/XTLS/Xray-core/releases/download/v1/Xray-linux-64.zip"
            )
            .is_err()
        );
    }

    #[test]
    fn xray_binary_backup_and_restore_preserves_previous_binary() {
        let dir = temp_test_dir("xray-binary-backup-restore");
        let binary_path = dir.join("xray");
        fs::write(&binary_path, "old-binary").unwrap();

        let backup_path = backup_xray_binary_before_update(&binary_path)
            .unwrap()
            .expect("existing binary should be backed up");
        fs::write(&binary_path, "new-broken-binary").unwrap();

        let detail =
            restore_xray_binary_after_failed_update(&binary_path, Some(&backup_path)).unwrap();

        assert!(detail.contains("restored previous xray binary"));
        assert_eq!(fs::read_to_string(&binary_path).unwrap(), "old-binary");
        assert_eq!(fs::read_to_string(&backup_path).unwrap(), "old-binary");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn xray_binary_restore_removes_invalid_binary_when_no_backup_exists() {
        let dir = temp_test_dir("xray-binary-no-backup");
        let binary_path = dir.join("xray");
        fs::write(&binary_path, "new-broken-binary").unwrap();

        let detail = restore_xray_binary_after_failed_update(&binary_path, None).unwrap();

        assert!(detail.contains("removed invalid xray binary"));
        assert!(!binary_path.exists());

        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn tick_applies_when_route_credentials_change_even_with_same_revision() {
        let panel_config = route_config_response("rev-a");
        let credential_bundle =
            route_credential_bundle("local-mtls", "CERT-A", "PRIVATE-KEY-A", "CA-A");
        let (panel_url, panel_handle, sync_reports, _, _) =
            spawn_test_panel(panel_config, credential_bundle).await;
        let dir = temp_test_dir("tick-route-credentials-changed");
        let binary = fake_success_binary(&dir);
        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir)
            .applied_revision("rev-a")
            .configured_xray_binary(binary)
            .build();
        runtime.config.panel_url = panel_url;

        runtime.tick().await.unwrap();

        assert_eq!(runtime.state.applied_revision.as_deref(), Some("rev-a"));
        assert!(
            runtime
                .state
                .last_apply_detail
                .as_deref()
                .unwrap()
                .contains("revision rev-a applied")
        );
        assert!(Path::new(&runtime.config.route_credentials_path).exists());
        assert!(Path::new(&runtime.config.route_credentials_dir).is_dir());
        assert!(runtime.state.last_route_credentials_saved_at_unix.is_some());
        let snapshot = runtime.snapshot();
        let route_manifest = snapshot
            .runtime_artifacts
            .iter()
            .find(|artifact| artifact.kind == RuntimeArtifactKind::RouteCredentialManifest)
            .unwrap();
        assert!(route_manifest.exists);
        assert!(route_manifest.secret_sensitive);
        assert_eq!(
            route_manifest.last_saved_at_unix,
            runtime.state.last_route_credentials_saved_at_unix
        );
        let route_dir = snapshot
            .runtime_artifacts
            .iter()
            .find(|artifact| artifact.kind == RuntimeArtifactKind::RouteCredentialDirectory)
            .unwrap();
        assert!(route_dir.exists);
        assert!(route_dir.secret_sensitive);
        let xray_artifact = snapshot
            .runtime_artifacts
            .iter()
            .find(|artifact| artifact.kind == RuntimeArtifactKind::XrayConfig)
            .unwrap();
        assert!(xray_artifact.exists);
        assert!(xray_artifact.executable_runtime_input);
        assert!(
            runtime
                .runtime_events
                .iter()
                .any(|event| event.kind == "route_credentials_synced")
        );
        let reports = sync_reports.lock().await;
        let report = reports.last().expect("sync report should be sent");
        assert_eq!(report.sync_status, NodeSyncStatus::Synced);
        assert_eq!(report.applied_revision.as_deref(), Some("rev-a"));

        panel_handle.abort();
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn tick_full_apply_sync_smoke_records_artifacts_validation_and_panel_report() {
        let panel_config = route_config_response("rev-smoke");
        let credential_bundle =
            route_credential_bundle("local-mtls", "CERT-SMOKE", "PRIVATE-KEY-SMOKE", "CA-SMOKE");
        let (panel_url, panel_handle, sync_reports, _, _) =
            spawn_test_panel(panel_config, credential_bundle).await;
        let dir = temp_test_dir("tick-full-apply-sync-smoke");
        let binary = fake_success_binary(&dir);
        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir)
            .configured_xray_binary(binary)
            .build();
        runtime.config.panel_url = panel_url;

        runtime.tick().await.unwrap();

        assert_eq!(runtime.state.node_id.as_deref(), Some("node-a"));
        assert_eq!(runtime.state.applied_revision.as_deref(), Some("rev-smoke"));
        assert!(runtime.state.last_config_saved_at_unix.is_some());
        assert!(runtime.state.last_runtime_config_saved_at_unix.is_some());
        assert!(
            runtime
                .state
                .last_sidecar_runtime_config_saved_at_unix
                .is_some()
        );
        assert!(runtime.state.last_xray_config_saved_at_unix.is_some());
        assert!(runtime.state.xray_runtime.last_validated_at_unix.is_some());
        assert!(runtime.state.last_route_credentials_saved_at_unix.is_some());

        let generated_config_path = Path::new(&runtime.config.local_config_path);
        let runtime_config_path = Path::new(&runtime.config.local_runtime_config_path);
        let sidecar_config_path = Path::new(&runtime.config.local_sidecar_runtime_config_path);
        let xray_config_path = Path::new(&runtime.config.local_xray_config_path);
        assert!(generated_config_path.is_file());
        assert!(runtime_config_path.is_file());
        assert!(sidecar_config_path.is_file());
        assert!(xray_config_path.is_file());
        assert!(Path::new(&runtime.config.route_credentials_path).is_file());
        assert!(Path::new(&runtime.config.route_credentials_dir).is_dir());

        let xray_json: serde_json::Value =
            serde_json::from_slice(&fs::read(xray_config_path).unwrap()).unwrap();
        assert!(xray_json.get("inbounds").is_some());
        assert!(xray_json.get("outbounds").is_some());
        assert!(xray_json.get("routing").is_some());

        let snapshot = runtime.snapshot();
        assert!(snapshot.runtime_validation_report.ready);
        assert_eq!(
            snapshot.runtime_validation_report.required_protocol_count,
            snapshot.runtime_validation_report.required_protocols.len()
        );
        for kind in [
            RuntimeArtifactKind::GeneratedConfig,
            RuntimeArtifactKind::NodeRuntimeConfig,
            RuntimeArtifactKind::SidecarRuntimeConfig,
            RuntimeArtifactKind::WireGuardSessionMapping,
            RuntimeArtifactKind::XrayConfig,
            RuntimeArtifactKind::RouteCredentialManifest,
            RuntimeArtifactKind::RouteCredentialDirectory,
        ] {
            assert!(
                snapshot
                    .runtime_artifacts
                    .iter()
                    .any(|artifact| artifact.kind == kind && artifact.exists),
                "missing runtime artifact: {kind:?}"
            );
        }
        assert_eq!(runtime.apply_history.len(), 1);
        assert_eq!(
            runtime.apply_history[0].revision.as_deref(),
            Some("rev-smoke")
        );
        assert_eq!(runtime.apply_history[0].status, NodeSyncStatus::Synced);

        let reports = sync_reports.lock().await;
        assert_eq!(reports.len(), 1);
        let report = reports.last().expect("sync report should be sent");
        assert_eq!(report.sync_status, NodeSyncStatus::Synced);
        assert_eq!(report.applied_revision.as_deref(), Some("rev-smoke"));
        assert!(
            report
                .detail
                .as_deref()
                .unwrap()
                .contains("revision rev-smoke applied")
        );

        panel_handle.abort();
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn tick_applies_when_route_assignments_change_even_with_same_revision() {
        let panel_config = route_config_response("rev-a");
        let credential_bundle =
            route_credential_bundle("local-mtls", "CERT-A", "PRIVATE-KEY-A", "CA-A");
        let (panel_url, panel_handle, sync_reports, _, _) =
            spawn_test_panel(panel_config, credential_bundle.clone()).await;
        let dir = temp_test_dir("tick-route-assignments-changed");
        let binary = fake_success_binary(&dir);
        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir)
            .applied_revision("rev-a")
            .configured_xray_binary(binary)
            .build();
        runtime.config.panel_url = panel_url;
        install_route_credentials(
            &runtime.config.route_credentials_dir,
            &runtime.config.route_credentials_path,
            &credential_bundle,
        )
        .unwrap();

        runtime.tick().await.unwrap();

        assert_eq!(runtime.state.applied_revision.as_deref(), Some("rev-a"));
        assert!(
            runtime
                .state
                .last_apply_detail
                .as_deref()
                .unwrap()
                .contains("revision rev-a applied")
        );
        assert!(
            runtime
                .runtime_events
                .iter()
                .any(|event| event.kind == "node_route_assignments_updated")
        );
        let reports = sync_reports.lock().await;
        let report = reports.last().expect("sync report should be sent");
        assert_eq!(report.sync_status, NodeSyncStatus::Synced);
        assert_eq!(report.applied_revision.as_deref(), Some("rev-a"));

        panel_handle.abort();
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn tick_skips_apply_when_revision_and_runtime_inputs_are_unchanged() {
        let panel_config = route_config_response("rev-a");
        let credential_bundle =
            route_credential_bundle("local-mtls", "CERT-A", "PRIVATE-KEY-A", "CA-A");
        let (panel_url, panel_handle, sync_reports, _, _) =
            spawn_test_panel(panel_config, credential_bundle).await;
        let dir = temp_test_dir("tick-unchanged-runtime-inputs");
        let binary = fake_success_binary(&dir);
        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir)
            .configured_xray_binary(binary)
            .build();
        runtime.config.panel_url = panel_url;

        runtime.tick().await.unwrap();
        assert_eq!(runtime.apply_history.len(), 1);

        runtime.tick().await.unwrap();

        assert_eq!(runtime.apply_history.len(), 1);
        let reports = sync_reports.lock().await;
        assert_eq!(reports.len(), 2);
        let report = reports.last().expect("second sync report should be sent");
        assert_eq!(report.sync_status, NodeSyncStatus::Synced);
        assert_eq!(report.applied_revision.as_deref(), Some("rev-a"));
        assert!(
            report
                .detail
                .as_deref()
                .unwrap()
                .contains("local revision already matches panel revision")
        );

        panel_handle.abort();
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn tick_reports_drifted_when_required_protocol_is_not_ready() {
        let mut panel_config = empty_generated_response("rev-sidecar");
        panel_config.generated_config.inbounds = vec![GeneratedInbound {
            tag: "hy2-in".to_string(),
            port: 2443,
            protocol: "hysteria2".to_string(),
            network: "udp".to_string(),
            tls_enabled: true,
        }];
        let credential_bundle =
            route_credential_bundle("local-mtls", "CERT-A", "PRIVATE-KEY-A", "CA-A");
        let (panel_url, panel_handle, sync_reports, _, _) =
            spawn_test_panel(panel_config, credential_bundle).await;
        let dir = temp_test_dir("tick-required-protocol-drifted");
        let binary = fake_success_binary(&dir);
        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir)
            .configured_xray_binary(binary)
            .build();
        runtime.config.panel_url = panel_url;

        runtime.tick().await.unwrap();

        let reports = sync_reports.lock().await;
        let report = reports.last().expect("sync report should be sent");
        assert_eq!(report.sync_status, NodeSyncStatus::Drifted);
        assert!(
            report
                .detail
                .as_deref()
                .unwrap()
                .contains("runtime protocol requirements not ready")
        );
        assert_eq!(runtime.snapshot().status, NodeStatus::Degraded);

        panel_handle.abort();
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn tick_reports_drifted_when_generated_inbound_lacks_client_material() {
        let mut panel_config = empty_generated_response("rev-generated-vless");
        panel_config.generated_config.inbounds = vec![GeneratedInbound {
            tag: "vless-in".to_string(),
            port: 2443,
            protocol: "vless".to_string(),
            network: "ws".to_string(),
            tls_enabled: false,
        }];
        let credential_bundle =
            route_credential_bundle("local-mtls", "CERT-A", "PRIVATE-KEY-A", "CA-A");
        let (panel_url, panel_handle, sync_reports, _, _) =
            spawn_test_panel(panel_config, credential_bundle).await;
        let dir = temp_test_dir("tick-generated-inbound-client-material-missing");
        let binary = fake_success_binary(&dir);
        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir)
            .configured_xray_binary(binary)
            .build();
        runtime.config.panel_url = panel_url;

        runtime.tick().await.unwrap();

        let reports = sync_reports.lock().await;
        let report = reports.last().expect("sync report should be sent");
        assert_eq!(report.sync_status, NodeSyncStatus::Drifted);
        assert!(
            report
                .detail
                .as_deref()
                .unwrap()
                .contains("generated_inbound_client_material_missing")
        );
        let snapshot = runtime.snapshot();
        assert_eq!(snapshot.status, NodeStatus::Degraded);
        assert!(
            snapshot
                .runtime_validation_report
                .disabled_reasons
                .iter()
                .any(|reason| reason.contains("xray render is fail-closed"))
        );

        panel_handle.abort();
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn tick_keeps_sidecar_protocol_drifted_until_renderer_exists() {
        let mut panel_config = empty_generated_response("rev-sidecar-ready-binary");
        panel_config.generated_config.inbounds = vec![GeneratedInbound {
            tag: "hy2-in".to_string(),
            port: 2443,
            protocol: "hysteria2".to_string(),
            network: "udp".to_string(),
            tls_enabled: true,
        }];
        let credential_bundle =
            route_credential_bundle("local-mtls", "CERT-A", "PRIVATE-KEY-A", "CA-A");
        let (panel_url, panel_handle, sync_reports, _, _) =
            spawn_test_panel(panel_config, credential_bundle).await;
        let dir = temp_test_dir("tick-sidecar-protocol-renderer-missing");
        let xray_binary = fake_success_binary(&dir);
        let hysteria_binary = fake_version_binary(&dir, "hysteria", "hysteria version v2.6.0");
        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir)
            .configured_xray_binary(xray_binary)
            .build();
        runtime.config.panel_url = panel_url;
        runtime.config.hysteria2_binary_path = Some(hysteria_binary.to_string_lossy().to_string());

        runtime.tick().await.unwrap();

        let reports = sync_reports.lock().await;
        let report = reports.last().expect("sync report should be sent");
        assert_eq!(report.sync_status, NodeSyncStatus::Drifted);
        assert!(
            report
                .detail
                .as_deref()
                .unwrap()
                .contains("sidecar runtime config material is missing or invalid")
        );
        let runtime_report = runtime.snapshot().runtime_validation_report;
        let component = runtime_report
            .components
            .iter()
            .find(|component| component.component == RuntimeComponentKind::Hysteria2)
            .unwrap();
        // Hysteria2 readiness depends on exactly one non-deterministic step:
        // spawning `hysteria version`. Its helper preflight is unconditionally
        // ready, so nothing else can make it non-Ready. The subprocess error
        // arrives in last_error, so print it; otherwise the failure reads as
        // "Ready != Degraded" with no hint at all.
        assert_eq!(
            component.readiness,
            RuntimeComponentReadiness::Ready,
            "hysteria version detection failed: version={:?} last_error={:?} detail={}",
            component.detected_version,
            component.last_error,
            component.detail
        );
        let required = runtime_report.required_protocols.first().unwrap();
        assert_eq!(required.readiness, RuntimeProtocolReadiness::Blocked);
        let sidecar_config_path = PathBuf::from(&runtime.config.local_sidecar_runtime_config_path);
        assert!(sidecar_config_path.is_file());
        let sidecar_config = serde_json::from_slice::<SidecarRuntimeConfigDocument>(
            &fs::read(&sidecar_config_path).unwrap(),
        )
        .unwrap();
        assert_eq!(sidecar_config.source_revision, "rev-sidecar-ready-binary");
        assert_eq!(sidecar_config.requirements.len(), 1);
        assert_eq!(
            sidecar_config.requirements[0].status,
            SidecarRuntimeRequirementStatus::Blocked
        );
        let snapshot = runtime.snapshot();
        assert_eq!(
            snapshot.local_sidecar_runtime_config_path,
            sidecar_config_path.to_string_lossy()
        );
        assert_eq!(
            snapshot.last_sidecar_runtime_config_saved_at_unix,
            Some(sidecar_config.created_at_unix)
        );
        let summary = snapshot.last_sidecar_runtime_summary.unwrap();
        assert_eq!(summary.source_revision, "rev-sidecar-ready-binary");
        assert_eq!(summary.requirement_count, 1);
        assert_eq!(summary.blocked_count, 1);
        let runtime_report = snapshot.runtime_validation_report;
        assert!(!runtime_report.sidecar_runtime.ready);
        assert_eq!(
            runtime_report.sidecar_runtime.config_path,
            sidecar_config_path.to_string_lossy()
        );
        assert_eq!(runtime_report.sidecar_runtime.requirement_count, 1);
        assert_eq!(runtime_report.sidecar_runtime.blocked_count, 1);
        assert_eq!(
            runtime_report.sidecar_runtime.requirements[0].source_ref,
            "hy2-in"
        );
        assert!(
            runtime_report
                .disabled_reasons
                .iter()
                .any(|reason| reason.contains("sidecar intent"))
        );

        panel_handle.abort();
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn tick_reports_synced_after_sidecar_executor_session_is_accepted() {
        let dir = temp_test_dir("tick-sidecar-session-accepted-sync");
        let cert = dir.join("hy2.crt");
        let key = dir.join("hy2.key");
        fs::write(&cert, "CERT").unwrap();
        fs::write(&key, "KEY").unwrap();
        let inbound = GeneratedInbound {
            tag: "hy2-in".to_string(),
            port: 2443,
            protocol: "hysteria2".to_string(),
            network: "udp".to_string(),
            tls_enabled: true,
        };
        let settings = serde_json::json!({
            "inbound": "hy2-in",
            "password": "secret",
            "tls_certificate_file": cert,
            "tls_key_file": key
        })
        .to_string();
        let mut panel_config = empty_generated_response("rev-sidecar-session-accepted");
        panel_config.generated_config.inbounds = vec![inbound.clone()];
        panel_config.generated_config.users = vec![GeneratedUserConfig {
            username: "alice".to_string(),
            status: "active".to_string(),
            data_limit_bytes: None,
            expire_at_unix: None,
            subscription_token: "token-a".to_string(),
            proxy_profiles: vec![GeneratedProxyProfile {
                id: "alice-hy2".to_string(),
                name: "Alice Hysteria2".to_string(),
                proxy_type: "hysteria2".to_string(),
                settings_json: settings,
            }],
            inbounds: vec![inbound],
            hosts: Vec::new(),
        }];
        let credential_bundle =
            route_credential_bundle("local-mtls", "CERT-A", "PRIVATE-KEY-A", "CA-A");
        let (panel_url, panel_handle, sync_reports, _, _) =
            spawn_test_panel(panel_config, credential_bundle).await;
        let xray_binary = fake_success_binary(&dir);
        let hysteria_binary = fake_version_binary(&dir, "hysteria", "hysteria version v2.6.0");
        let command = fake_success_binary(&dir);
        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir)
            .configured_xray_binary(xray_binary)
            .build();
        runtime.config.panel_url = panel_url;
        runtime.config.hysteria2_binary_path = Some(hysteria_binary.to_string_lossy().to_string());
        runtime.config.hysteria2_start_args = vec![command.to_string_lossy().to_string()];
        runtime.config.hysteria2_status_args = vec![command.to_string_lossy().to_string()];

        runtime.tick().await.unwrap();
        {
            let reports = sync_reports.lock().await;
            let report = reports.last().expect("initial sync report should be sent");
            assert_eq!(report.sync_status, NodeSyncStatus::Drifted);
            assert!(
                report
                    .detail
                    .as_deref()
                    .unwrap()
                    .contains("matching executor session has not been accepted"),
                "unexpected drift detail: {:?}",
                report.detail
            );
        }

        let session = runtime.sidecar_executor_session();
        assert!(session.executable);
        let results = session
            .envelopes
            .iter()
            .map(|envelope| LocalSidecarExecutorResultRequest {
                command_id: envelope.command_id.clone(),
                status: envelope.acceptance.expected_status,
                completed_checks: envelope.acceptance.required_checks.clone(),
                exit_code: Some(0),
                detail: Some("accepted by test executor".to_string()),
                completed_at_unix: Some(123),
            })
            .collect();
        let response = runtime
            .complete_sidecar_executor_session(SidecarExecutorSessionResultRequest {
                session_id: session.session_id,
                results,
            })
            .unwrap();
        assert!(response.accepted);

        runtime.tick().await.unwrap();

        let reports = sync_reports.lock().await;
        let report = reports.last().expect("second sync report should be sent");
        assert_eq!(report.sync_status, NodeSyncStatus::Synced);
        assert!(
            !report
                .detail
                .as_deref()
                .unwrap()
                .contains("xray render is fail-closed")
        );
        let runtime_report = runtime.snapshot().runtime_validation_report;
        assert!(runtime_report.sidecar_runtime.ready);
        assert!(
            runtime_report
                .required_protocols
                .iter()
                .all(|requirement| requirement.readiness == RuntimeProtocolReadiness::Ready)
        );

        panel_handle.abort();
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn tick_refreshes_sidecar_preflight_state() {
        let panel_config = empty_generated_response("rev-sidecar-preflight");
        let credential_bundle =
            route_credential_bundle("local-mtls", "CERT-A", "PRIVATE-KEY-A", "CA-A");
        let (panel_url, panel_handle, _, _, _) =
            spawn_test_panel(panel_config, credential_bundle).await;
        let dir = temp_test_dir("tick-sidecar-preflight");
        let binary = fake_version_binary(&dir, "hysteria", "hysteria version v2.6.0");
        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir).build();
        runtime.config.panel_url = panel_url;
        runtime.config.hysteria2_binary_path = Some(binary.to_string_lossy().to_string());

        runtime.tick().await.unwrap();

        let snapshot = runtime.snapshot();
        let sidecar = snapshot
            .sidecars
            .iter()
            .find(|sidecar| sidecar.sidecar == LocalSidecarKind::Hysteria2)
            .unwrap();
        assert_eq!(sidecar.status, LocalSidecarStatus::Ready);
        assert_eq!(
            sidecar.detected_version.as_deref(),
            Some("hysteria version v2.6.0")
        );
        assert_eq!(sidecar.last_action, Some(LocalSidecarAction::Status));
        let first_event_count = runtime
            .runtime_events
            .iter()
            .filter(|event| event.kind == "sidecar_preflight_state_changed")
            .count();
        assert_eq!(first_event_count, 2);

        runtime.tick().await.unwrap();

        let second_event_count = runtime
            .runtime_events
            .iter()
            .filter(|event| event.kind == "sidecar_preflight_state_changed")
            .count();
        assert_eq!(second_event_count, first_event_count);

        panel_handle.abort();
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn tick_forwards_observation_only_subscription_snapshot_without_capabilities() {
        let panel_config = empty_generated_response("rev-a");
        let credential_bundle =
            route_credential_bundle("local-mtls", "CERT-A", "PRIVATE-KEY-A", "CA-A");
        let (panel_url, panel_handle, _, session_reports, _) =
            spawn_test_panel(panel_config, credential_bundle).await;
        let dir = temp_test_dir("tick-subscription-session-observations");
        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir).build();
        runtime.config.panel_url = panel_url;
        runtime.config.subscription_session_adapter_token = Some("adapter-token".to_string());
        runtime
            .register_subscription_session_adapter(RegisterLocalSubscriptionSessionAdapterRequest {
                adapter_instance_id: "adapter-a".to_string(),
                runtime_capabilities: Vec::new(),
            })
            .await
            .unwrap();
        runtime
            .stage_subscription_session_observations(
                "adapter-a",
                ReportSubscriptionSessionsRequest {
                    observation_source:
                        SubscriptionSessionObservationSource::NodeManagedRuntimeTable,
                    runtime_capabilities: Vec::new(),
                    observations: vec![SubscriptionSessionObservation {
                        session_id: "session-a".to_string(),
                        runtime_username: "catalog/client-a".to_string(),
                        runtime_session_ref: None,
                        device_fingerprint: Some("device-a".to_string()),
                        source_ip: Some("203.0.113.7".to_string()),
                        connected_at_unix: Some(1),
                    }],
                },
            )
            .unwrap();

        runtime.tick().await.unwrap();

        let reports = session_reports.lock().await;
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].observations.len(), 1);
        assert!(reports[0].runtime_capabilities.is_empty());
        assert_eq!(
            runtime
                .snapshot()
                .subscription_session_adapter
                .last_reported_count,
            1
        );

        panel_handle.abort();
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn exact_adapter_queues_targeted_action_and_requires_absence_proof() {
        let panel_config = empty_generated_response("rev-a");
        let credential_bundle =
            route_credential_bundle("local-mtls", "CERT-A", "PRIVATE-KEY-A", "CA-A");
        let (panel_url, panel_handle, _, _, enforcement_results) =
            spawn_test_panel(panel_config, credential_bundle).await;
        let dir = temp_test_dir("tick-exact-session-enforcement");
        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir).build();
        runtime.config.panel_url = panel_url;
        runtime.config.subscription_session_adapter_token = Some("adapter-token".to_string());
        runtime
            .register_subscription_session_adapter(RegisterLocalSubscriptionSessionAdapterRequest {
                adapter_instance_id: "adapter-exact".to_string(),
                runtime_capabilities: vec![
                    SubscriptionSessionRuntimeCapability::OpaqueSessionReference,
                    SubscriptionSessionRuntimeCapability::ExactSessionTermination,
                    SubscriptionSessionRuntimeCapability::PostActionAbsenceVerification,
                ],
            })
            .await
            .unwrap();
        runtime
            .stage_subscription_session_observations(
                "adapter-exact",
                ReportSubscriptionSessionsRequest {
                    observation_source:
                        SubscriptionSessionObservationSource::NodeManagedRuntimeTable,
                    runtime_capabilities: vec![
                        SubscriptionSessionRuntimeCapability::OpaqueSessionReference,
                        SubscriptionSessionRuntimeCapability::ExactSessionTermination,
                        SubscriptionSessionRuntimeCapability::PostActionAbsenceVerification,
                    ],
                    observations: vec![SubscriptionSessionObservation {
                        session_id: "session-a".to_string(),
                        runtime_username: "catalog/client-a".to_string(),
                        runtime_session_ref: Some("opaque-runtime-session-a".to_string()),
                        device_fingerprint: None,
                        source_ip: Some("203.0.113.7".to_string()),
                        connected_at_unix: Some(1),
                    }],
                },
            )
            .unwrap();

        runtime.tick().await.unwrap();

        let pending = runtime
            .pending_subscription_session_enforcements("adapter-exact")
            .unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].action_id, "action-session-a");
        assert_eq!(pending[0].runtime_session_ref, "opaque-runtime-session-a");
        assert!(
            runtime
                .pending_subscription_session_enforcements("adapter-other")
                .is_err()
        );
        assert_eq!(
            runtime.snapshot().subscription_session_adapter.status,
            SubscriptionSessionAdapterStatus::ExactEnforcementReady
        );

        let invalid_proof = runtime
            .complete_subscription_session_enforcement(
                "adapter-exact",
                "action-session-a",
                CompleteLocalSubscriptionSessionEnforcementRequest {
                    status: SubscriptionSessionEnforcementStatus::Applied,
                    runtime_session_ref: Some("wrong-ref".to_string()),
                    session_absent_after_action: Some(true),
                    verified_at_unix: Some(2),
                    detail: None,
                },
            )
            .await;
        assert!(invalid_proof.is_err());
        assert!(enforcement_results.lock().await.is_empty());

        runtime
            .complete_subscription_session_enforcement(
                "adapter-exact",
                "action-session-a",
                CompleteLocalSubscriptionSessionEnforcementRequest {
                    status: SubscriptionSessionEnforcementStatus::Applied,
                    runtime_session_ref: Some("opaque-runtime-session-a".to_string()),
                    session_absent_after_action: Some(true),
                    verified_at_unix: Some(2),
                    detail: Some("target connection absent after termination".to_string()),
                },
            )
            .await
            .unwrap();

        assert!(
            runtime
                .pending_subscription_session_enforcements("adapter-exact")
                .unwrap()
                .is_empty()
        );
        let results = enforcement_results.lock().await;
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].adapter,
            Some(SubscriptionSessionRuntimeAdapter::NodeManagedExactSession)
        );
        assert_eq!(results[0].session_absent_after_action, Some(true));

        panel_handle.abort();
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn expired_adapter_lease_fails_pending_exact_action() {
        let panel_config = empty_generated_response("rev-a");
        let credential_bundle =
            route_credential_bundle("local-mtls", "CERT-A", "PRIVATE-KEY-A", "CA-A");
        let (panel_url, panel_handle, _, _, enforcement_results) =
            spawn_test_panel(panel_config, credential_bundle).await;
        let dir = temp_test_dir("expired-session-adapter-lease");
        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir).build();
        runtime.config.panel_url = panel_url;
        runtime.active_subscription_session_adapter = Some(ActiveSubscriptionSessionAdapterLease {
            adapter_instance_id: "adapter-expired".to_string(),
            runtime_capabilities: vec![
                SubscriptionSessionRuntimeCapability::OpaqueSessionReference,
                SubscriptionSessionRuntimeCapability::ExactSessionTermination,
                SubscriptionSessionRuntimeCapability::PostActionAbsenceVerification,
            ],
            registered_at_unix: 0,
            lease_expires_at_unix: 0,
        });
        runtime.pending_subscription_session_enforcements =
            vec![PendingSubscriptionSessionEnforcement {
                adapter_instance_id: "adapter-expired".to_string(),
                command: LocalSubscriptionSessionEnforcementCommand {
                    action_id: "action-expired".to_string(),
                    session_id: "session-expired".to_string(),
                    action: node_domain::SubscriptionSessionEnforcementAction::TerminateSession,
                    runtime_session_ref: "opaque-ref-expired".to_string(),
                    reason: "test policy action".to_string(),
                    requires_absence_verification: true,
                    issued_at_unix: 0,
                    expires_at_unix: u64::MAX,
                },
            }];

        runtime
            .expire_subscription_session_adapter_lease()
            .await
            .unwrap();

        assert!(runtime.active_subscription_session_adapter.is_none());
        assert!(runtime.pending_subscription_session_enforcements.is_empty());
        let results = enforcement_results.lock().await;
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].status,
            SubscriptionSessionEnforcementStatus::Failed
        );
        assert!(
            results[0]
                .detail
                .as_deref()
                .unwrap()
                .contains("lease expired")
        );

        panel_handle.abort();
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn expired_exact_action_reports_failed_even_while_adapter_lease_is_live() {
        let panel_config = empty_generated_response("rev-a");
        let credential_bundle =
            route_credential_bundle("local-mtls", "CERT-A", "PRIVATE-KEY-A", "CA-A");
        let (panel_url, panel_handle, _, _, enforcement_results) =
            spawn_test_panel(panel_config, credential_bundle).await;
        let dir = temp_test_dir("expired-session-enforcement-action");
        let (mut runtime, dir) = TestRuntimeBuilder::from_dir(dir).build();
        runtime.config.panel_url = panel_url;
        runtime.active_subscription_session_adapter = Some(ActiveSubscriptionSessionAdapterLease {
            adapter_instance_id: "adapter-live".to_string(),
            runtime_capabilities: vec![
                SubscriptionSessionRuntimeCapability::OpaqueSessionReference,
                SubscriptionSessionRuntimeCapability::ExactSessionTermination,
                SubscriptionSessionRuntimeCapability::PostActionAbsenceVerification,
            ],
            registered_at_unix: now_unix(),
            lease_expires_at_unix: now_unix().saturating_add(90),
        });
        runtime.pending_subscription_session_enforcements =
            vec![PendingSubscriptionSessionEnforcement {
                adapter_instance_id: "adapter-live".to_string(),
                command: LocalSubscriptionSessionEnforcementCommand {
                    action_id: "action-timeout".to_string(),
                    session_id: "session-timeout".to_string(),
                    action: node_domain::SubscriptionSessionEnforcementAction::TerminateSession,
                    runtime_session_ref: "opaque-ref-timeout".to_string(),
                    reason: "test policy action".to_string(),
                    requires_absence_verification: true,
                    issued_at_unix: 0,
                    expires_at_unix: 0,
                },
            }];

        assert!(
            runtime
                .pending_subscription_session_enforcements("adapter-live")
                .unwrap()
                .is_empty()
        );
        runtime
            .expire_subscription_session_enforcements()
            .await
            .unwrap();

        assert!(runtime.pending_subscription_session_enforcements.is_empty());
        let results = enforcement_results.lock().await;
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].status,
            SubscriptionSessionEnforcementStatus::Failed
        );
        assert!(
            results[0]
                .detail
                .as_deref()
                .unwrap()
                .contains("deadline expired")
        );

        panel_handle.abort();
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn route_credential_install_is_idempotent_and_secret_manifest_is_path_only() {
        let dir = temp_test_dir("route-credentials-idempotent");
        let credentials_dir = dir.join("materials");
        let manifest_path = dir.join("route-credentials.json");
        let bundle =
            route_credential_bundle("cluster/a/node/b/mtls", "CERT-A", "PRIVATE-KEY-A", "CA-A");

        let first = install_route_credentials(
            credentials_dir.to_str().unwrap(),
            manifest_path.to_str().unwrap(),
            &bundle,
        )
        .unwrap();
        let second = install_route_credentials(
            credentials_dir.to_str().unwrap(),
            manifest_path.to_str().unwrap(),
            &bundle,
        )
        .unwrap();

        assert!(first > 0);
        assert_eq!(second, 0);

        let manifest = fs::read_to_string(&manifest_path).unwrap();
        assert!(manifest.contains("certificate_file"));
        assert!(manifest.contains("private_key_file"));
        assert!(!manifest.contains("PRIVATE-KEY-A"));
        assert!(!manifest.contains("CERT-A"));
        assert!(!manifest.contains("CA-A"));

        let store = load_route_credentials(&manifest_path).unwrap();
        let credential = store.find("cluster/a/node/b/mtls").unwrap();
        assert!(credential.has_mtls_client_material());
        assert_eq!(credential.server_name.as_deref(), Some("relay.local"));
        assert_eq!(credential.certificate_pins, vec!["pin-a".to_string()]);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn route_credential_install_detects_rotated_material() {
        let dir = temp_test_dir("route-credentials-rotation");
        let credentials_dir = dir.join("materials");
        let manifest_path = dir.join("route-credentials.json");
        let original =
            route_credential_bundle("cluster/a/node/b/mtls", "CERT-A", "PRIVATE-KEY-A", "CA-A");
        let rotated =
            route_credential_bundle("cluster/a/node/b/mtls", "CERT-B", "PRIVATE-KEY-B", "CA-B");

        let first = install_route_credentials(
            credentials_dir.to_str().unwrap(),
            manifest_path.to_str().unwrap(),
            &original,
        )
        .unwrap();
        let second = install_route_credentials(
            credentials_dir.to_str().unwrap(),
            manifest_path.to_str().unwrap(),
            &rotated,
        )
        .unwrap();

        assert!(first > 0);
        assert!(second > 0);

        let store = load_route_credentials(&manifest_path).unwrap();
        let credential = store.find("cluster/a/node/b/mtls").unwrap();
        assert_eq!(
            fs::read_to_string(credential.private_key_file.as_ref().unwrap()).unwrap(),
            "PRIVATE-KEY-B"
        );
        assert_eq!(
            fs::read_to_string(credential.certificate_file.as_ref().unwrap()).unwrap(),
            "CERT-B"
        );

        let _ = fs::remove_dir_all(dir);
    }

    fn runtime_with_assignment(assignment: NodeRouteAssignment) -> NodeRuntimeConfigDocument {
        NodeRuntimeConfigDocument {
            schema_version: 1,
            node_id: Some("node-a".to_string()),
            source_revision: "rev-a".to_string(),
            source_generated_at_unix: 1,
            created_at_unix: 2,
            source_user_count: 0,
            source_node_count: 1,
            users: Vec::new(),
            inbounds: Vec::new(),
            hosts: Vec::new(),
            cluster_intents: Vec::new(),
            route_assignments: vec![assignment],
            required_protocols: Vec::new(),
        }
    }

    #[test]
    fn vless_hop_uses_same_opaque_identity_for_inbound_and_outbound_uuid() {
        let identity_ref = "opaque-cluster-peer-next";
        let inbound_settings = xray_inbound_settings(&route_listen(identity_ref));
        let outbound_settings = render_next_peer_vless_settings(&next_peer(identity_ref));

        let inbound_uuid = inbound_settings["clients"][0]["id"].as_str().unwrap();
        let outbound_uuid = outbound_settings["vnext"][0]["users"][0]["id"]
            .as_str()
            .unwrap();

        assert_eq!(inbound_uuid, outbound_uuid);
        assert_eq!(inbound_uuid.len(), 36);
    }

    #[test]
    fn renderer_fails_closed_when_required_mtls_material_is_missing() {
        let mut listen = route_listen("opaque-local");
        listen.security = Some(route_security("missing-ref"));
        let assignment = NodeRouteAssignment {
            node_id: "node-a".to_string(),
            route_id: "route-a".to_string(),
            cluster_id: "cluster-a".to_string(),
            cluster_revision: "cluster-rev-a".to_string(),
            role: ClusterNodeRole::Relay,
            listen: Some(listen),
            previous_peer: None,
            next_peer: None,
        };

        let plan = render_xray_config(
            &runtime_with_assignment(assignment),
            &RouteCredentialStore::default(),
            None,
        );

        assert!(
            plan.feature_flags
                .iter()
                .any(|flag| flag == "secure-route-material-pending-fail-closed")
        );
        assert!(
            plan.issues
                .iter()
                .any(|issue| issue.reason == "listen_mtls_material_missing")
        );
        assert_eq!(plan.config["inbounds"].as_array().unwrap().len(), 0);
        assert_eq!(plan.config["routing"]["rules"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn renderer_emits_vless_over_tcp_with_mtls_when_material_is_available() {
        let (credentials, temp_dir) = route_credential_store();
        let plan = render_xray_config(
            &runtime_with_assignment(secured_assignment()),
            &credentials,
            Some("v25.1.1".to_string()),
        );

        assert!(
            plan.feature_flags
                .iter()
                .any(|flag| flag == "secure-route-material-available")
        );
        let inbound = &plan.config["inbounds"][0];
        assert_eq!(inbound["protocol"], "vless");
        assert_eq!(inbound["settings"]["decryption"], "none");
        assert_eq!(
            inbound["settings"]["clients"][0]["id"]
                .as_str()
                .unwrap()
                .len(),
            36
        );
        assert_eq!(inbound["streamSettings"]["network"], "tcp");
        assert_eq!(inbound["streamSettings"]["security"], "tls");
        assert!(
            inbound["streamSettings"]["tlsSettings"]["allowInsecure"].is_null(),
            "allowInsecure was removed from Xray and must not be emitted"
        );
        // Pinning is configured by the client; the key must not appear on the listen side.
        assert!(
            inbound["streamSettings"]["tlsSettings"]["pinnedPeerCertSha256"].is_null(),
            "pinnedPeerCertSha256 is a client-side setting; listen does not emit it"
        );
        assert_eq!(
            inbound["streamSettings"]["tlsSettings"]["certificates"][0]["usage"],
            "encipherment"
        );
        assert_eq!(
            inbound["streamSettings"]["tlsSettings"]["certificates"][1]["usage"],
            "verify"
        );

        let outbound = plan.config["outbounds"]
            .as_array()
            .unwrap()
            .iter()
            .find(|outbound| outbound["tag"] == "route-route-a-next")
            .expect("route outbound exists");
        assert_eq!(outbound["protocol"], "vless");
        assert_eq!(outbound["settings"]["vnext"][0]["address"], "203.0.113.10");
        assert_eq!(outbound["settings"]["vnext"][0]["port"], 62050);
        assert_eq!(
            outbound["settings"]["vnext"][0]["users"][0]["encryption"],
            "none"
        );
        assert_eq!(outbound["streamSettings"]["network"], "tcp");
        assert_eq!(outbound["streamSettings"]["security"], "tls");
        assert_eq!(
            outbound["streamSettings"]["tlsSettings"]["serverName"],
            "relay.local"
        );
        assert!(
            outbound["streamSettings"]["tlsSettings"]["allowInsecure"].is_null(),
            "allowInsecure was removed from Xray and must not be emitted"
        );
        // The client-side replacement for allowInsecure: a specific certificate is
        // pinned. Xray expects a single string, not an array.
        assert_eq!(
            outbound["streamSettings"]["tlsSettings"]["pinnedPeerCertSha256"],
            "1a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f809"
        );
        assert_eq!(
            plan.config["routing"]["rules"][0]["outboundTag"],
            "route-route-a-next"
        );

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn render_summary_exposes_operator_diagnostics() {
        let (credentials, temp_dir) = route_credential_store();
        let plan = render_xray_config(
            &runtime_with_assignment(secured_assignment()),
            &credentials,
            Some("v25.1.1".to_string()),
        );

        let summary = summarize_xray_render_plan(&plan);

        assert_eq!(summary.renderer_version, 1);
        assert_eq!(summary.source_revision, "rev-a");
        assert_eq!(summary.xray_detected_version, Some("v25.1.1".to_string()));
        assert_eq!(summary.inbound_count, 1);
        assert_eq!(summary.outbound_count, 3);
        assert_eq!(summary.routing_rule_count, 1);
        assert!(!summary.fail_closed);
        assert_eq!(summary.issue_count, 0);
        assert!(summary.issues.is_empty());
        assert!(
            summary
                .feature_flags
                .iter()
                .any(|flag| flag == "secure-route-material-available")
        );
        let formatted = format_xray_render_summary(&summary);
        assert!(formatted.contains("fail_closed=false"));
        assert!(formatted.contains("issues=0"));

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn renderer_blocks_next_peer_without_endpoint() {
        let (credentials, temp_dir) = route_credential_store();
        let mut assignment = secured_assignment();
        if let Some(peer) = assignment.next_peer.as_mut() {
            peer.address = None;
        }
        let plan = render_xray_config(&runtime_with_assignment(assignment), &credentials, None);
        let summary = summarize_xray_render_plan(&plan);

        assert!(
            plan.feature_flags
                .iter()
                .any(|flag| flag == "secure-route-material-pending-fail-closed")
        );
        assert!(summary.fail_closed);
        assert_eq!(summary.issue_count, 1);
        assert_eq!(summary.issues[0].route_id, "route-a");
        assert_eq!(summary.issues[0].scope, "next_peer");
        assert_eq!(summary.issues[0].reason, "next_peer_endpoint_missing");
        assert_eq!(plan.config["routing"]["rules"][0]["outboundTag"], "blocked");
        assert!(
            !plan.config["outbounds"]
                .as_array()
                .unwrap()
                .iter()
                .any(|outbound| outbound["tag"] == "route-route-a-next")
        );

        let _ = fs::remove_dir_all(temp_dir);
    }
}

#[cfg(test)]
mod reality_render_tests {
    use super::*;

    fn runtime_with_inbound(inbound: GeneratedInbound) -> NodeRuntimeConfigDocument {
        NodeRuntimeConfigDocument {
            schema_version: 1,
            node_id: Some("node-a".to_string()),
            source_revision: "rev-a".to_string(),
            source_generated_at_unix: 1,
            created_at_unix: 2,
            source_user_count: 0,
            source_node_count: 1,
            users: Vec::new(),
            inbounds: vec![inbound],
            hosts: Vec::new(),
            cluster_intents: Vec::new(),
            route_assignments: Vec::new(),
            required_protocols: Vec::new(),
        }
    }

    fn reality_material(tag: &str) -> node_domain::NodeRealityMaterial {
        node_domain::NodeRealityMaterial {
            inbound_tag: tag.to_string(),
            private_key_b64: "NhUhziBHN2ePb3z3bp4eY1fzGDceZG_GVMav_avdul4".to_string(),
            short_ids: vec!["65e44873e150a969".to_string()],
            dest: "www.microsoft.com:443".to_string(),
            server_names: vec!["www.microsoft.com".to_string()],
        }
    }

    /// Reality is actually rendered rather than merely modelled.
    ///
    /// Before this, `grep realitySettings` across both repositories returned zero
    /// while `security: reality` was advertised in the capability matrix.
    #[test]
    fn reality_inbound_renders_full_settings_block() {
        let store = RouteCredentialStore {
            credentials: Vec::new(),
            reality_materials: vec![reality_material("vless-reality")],
        };
        let runtime = runtime_with_inbound(GeneratedInbound {
            tag: "vless-reality".to_string(),
            protocol: "vless".to_string(),
            network: "tcp".to_string(),
            port: 443,
            tls_enabled: false,
        });

        let rendered =
            xray_generated_inbound_stream_settings(&runtime, &runtime.inbounds[0], &store);

        assert_eq!(rendered["security"], "reality");
        let reality = &rendered["realitySettings"];
        assert_eq!(reality["dest"], "www.microsoft.com:443");
        assert_eq!(reality["xver"], 0);
        assert_eq!(reality["serverNames"][0], "www.microsoft.com");
        assert_eq!(
            reality["privateKey"],
            "NhUhziBHN2ePb3z3bp4eY1fzGDceZG_GVMav_avdul4"
        );
        assert_eq!(reality["shortIds"][0], "65e44873e150a969");
        // Reality displaces TLS: the two must not be mixed in one inbound.
        assert!(rendered["tlsSettings"].is_null());
    }

    /// An inbound without material does not silently become Reality.
    #[test]
    fn inbound_without_material_does_not_become_reality() {
        let store = RouteCredentialStore {
            credentials: Vec::new(),
            reality_materials: vec![reality_material("other-tag")],
        };
        let runtime = runtime_with_inbound(GeneratedInbound {
            tag: "plain".to_string(),
            protocol: "vless".to_string(),
            network: "tcp".to_string(),
            port: 8080,
            tls_enabled: false,
        });

        let rendered =
            xray_generated_inbound_stream_settings(&runtime, &runtime.inbounds[0], &store);
        assert_eq!(rendered["security"], "none");
        assert!(rendered["realitySettings"].is_null());
    }
}

#[cfg(test)]
mod persisted_file_failure_tests {
    use super::tests::temp_test_dir;
    use super::*;

    fn garbage(name: &str) -> String {
        let base = temp_test_dir(&format!("garbage-{name}"));
        let path = base.join(format!("{name}.json"));
        fs::write(&path, b"{ not json ][").expect("garbage written");
        path.to_string_lossy().to_string()
    }

    fn absent(name: &str) -> String {
        temp_test_dir(&format!("absent-{name}"))
            .join(format!("{name}.json"))
            .to_string_lossy()
            .to_string()
    }

    /// A corrupt state file does not become "the node has applied nothing".
    ///
    /// `unwrap_or_default` used to produce exactly that: the agent considered
    /// itself clean and re-applied configuration from scratch.
    #[test]
    fn malformed_persisted_files_are_refused_not_defaulted() {
        for (name, outcome) in [
            ("state", load_state(&garbage("state")).err()),
            (
                "apply-history",
                load_apply_history(&garbage("apply-history")).err(),
            ),
            (
                "runtime-events",
                load_runtime_events(&garbage("runtime-events")).err(),
            ),
        ] {
            let error =
                outcome.unwrap_or_else(|| panic!("{name}: a corrupt file must produce an error"));
            let message = format!("{error}");
            assert!(
                message.contains(name),
                "{name}: the message must name the file, got {message}"
            );
            assert!(
                message.contains("will not be replaced"),
                "{name}: the message must explain the file is not overwritten"
            );
        }
    }

    #[test]
    fn missing_persisted_files_fall_back_to_empty_state() {
        assert!(
            load_apply_history(&absent("apply-history"))
                .expect("a missing file is not an error")
                .is_empty()
        );
        assert!(
            load_runtime_events(&absent("runtime-events"))
                .expect("a missing file is not an error")
                .is_empty()
        );
        load_state(&absent("state")).expect("a missing file is not an error");
    }

    /// Secret material is created with mode 0600 up front.
    #[cfg(unix)]
    #[test]
    fn secret_temp_files_are_created_with_restricted_mode() {
        use std::os::unix::fs::PermissionsExt;

        let path = temp_test_dir("secret-mode").join("secret.tmp");
        write_secret_temp_file(&path, b"private-key").expect("file written");

        let mode = fs::metadata(&path).expect("metadata").permissions().mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "secret file created with mode {:o}, expected 600",
            mode & 0o777
        );
    }
}

#[cfg(test)]
mod test_stub_binary_tests {
    use super::tests::{fake_failure_binary, fake_version_binary, temp_test_dir};
    use std::fs;
    use std::process::Command;

    /// The stub is a real binary, not a script.
    ///
    /// A `#!/bin/sh` script pulls an interpreter into the exec path: a second
    /// process and a dependency on `/bin/sh`. A transient fork/exec failure under
    /// load then surfaced as "component is not ready" with no stated cause.
    #[test]
    fn stub_is_a_real_binary_without_an_interpreter() {
        let dir = temp_test_dir("stub-shape");
        let binary = fake_version_binary(&dir, "hysteria", "hysteria version v2.6.0");

        let head = fs::read(&binary).expect("stub is readable");
        // The assertion is negative and therefore portable: the built stub is ELF
        // on Linux and PE with an MZ signature on Windows. The only thing they
        // share is the absence of a shebang, which is exactly what is checked.
        assert_ne!(&head[..2], b"#!", "the stub became a shebang script again");
    }

    /// Output comes from a sidecar file rather than the environment: environment
    /// is per-process, so parallel tests would race for it.
    #[test]
    fn stub_prints_requested_output_and_exit_code() {
        let dir = temp_test_dir("stub-behaviour");

        let version = fake_version_binary(&dir, "hysteria", "hysteria version v2.6.0");
        let output = Command::new(&version)
            .arg("version")
            .output()
            .expect("spawn");
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            "hysteria version v2.6.0"
        );

        // Two stubs in one directory do not interfere with each other.
        let other = fake_version_binary(&dir, "wg", "wireguard-tools v1.0.20210914");
        let output = Command::new(&other)
            .arg("--version")
            .output()
            .expect("spawn");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            "wireguard-tools v1.0.20210914"
        );

        let failing = fake_failure_binary(&dir);
        let output = Command::new(&failing).output().expect("spawn");
        assert!(
            !output.status.success(),
            "the failure stub must exit non-zero"
        );
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(
            String::from_utf8_lossy(&output.stderr).trim(),
            "validation failed"
        );
        assert!(output.stdout.is_empty());
    }
}

use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeAgentIdentity {
    pub node_id: String,
    pub name: String,
    pub status: NodeStatus,
    pub sync_status: NodeSyncStatus,
    pub last_applied_revision: Option<String>,
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
    pub runtime_components: Vec<NodeReportedRuntimeComponentView>,
    #[serde(default)]
    pub external_xray_validation: Option<XrayExternalValidationReport>,
    #[serde(default)]
    pub runtime_alerts: Vec<RuntimeAlert>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolRuntimeOwner {
    Xray,
    Sidecar,
    NodeNative,
    Planned,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum XrayExternalValidationStatus {
    Passed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct XrayExternalValidationReport {
    pub status: XrayExternalValidationStatus,
    pub checked_at_unix: u64,
    pub binary_path: Option<String>,
    pub internal_validation_valid: bool,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub detail: String,
    pub config_retained: bool,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeMetricsRequest {
    pub memory_used_bytes: u64,
    pub memory_total_bytes: u64,
    pub disk_used_bytes: u64,
    pub disk_total_bytes: u64,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionSessionVerdict {
    Allow,
    Block,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionSessionEnforcementAction {
    TerminateSession,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionSessionRuntimeAdapter {
    NodeManagedExactSession,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionSessionObservationSource {
    NodeManagedRuntimeTable,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionSessionRuntimeCapability {
    OpaqueSessionReference,
    ExactSessionTermination,
    PostActionAbsenceVerification,
    PrincipalWideTerminationOnly,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionSessionEnforcementStatus {
    Pending,
    Applied,
    Failed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionSessionAdapterStatus {
    Unsupported,
    ObservationOnly,
    ExactEnforcementReady,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SubscriptionSessionAdapterView {
    pub status: SubscriptionSessionAdapterStatus,
    pub observation_source: Option<SubscriptionSessionObservationSource>,
    pub runtime_capabilities: Vec<SubscriptionSessionRuntimeCapability>,
    pub exact_session_termination_ready: bool,
    pub disabled_reason: Option<String>,
    pub buffered_observation_count: usize,
    pub last_observation_at_unix: Option<u64>,
    pub last_report_at_unix: Option<u64>,
    pub last_reported_count: usize,
    pub last_blocked_count: usize,
    pub pending_enforcement_count: usize,
    pub adapter_registered: bool,
    pub active_lease_expires_at_unix: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionSessionEnforcementView {
    pub action_id: String,
    pub session_id: String,
    pub action: SubscriptionSessionEnforcementAction,
    pub status: SubscriptionSessionEnforcementStatus,
    pub reason: String,
    pub required_adapter: SubscriptionSessionRuntimeAdapter,
    pub runtime_session_ref_present: bool,
    pub requires_absence_verification: bool,
    pub issued_at_unix: u64,
    pub updated_at_unix: u64,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionSessionObservation {
    pub session_id: String,
    pub runtime_username: String,
    pub runtime_session_ref: Option<String>,
    pub device_fingerprint: Option<String>,
    pub source_ip: Option<String>,
    pub connected_at_unix: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportSubscriptionSessionsRequest {
    pub observation_source: SubscriptionSessionObservationSource,
    #[serde(default)]
    pub runtime_capabilities: Vec<SubscriptionSessionRuntimeCapability>,
    pub observations: Vec<SubscriptionSessionObservation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionSessionVerdictView {
    pub session_id: String,
    pub verdict: SubscriptionSessionVerdict,
    pub reason: String,
    pub enforcement: Option<SubscriptionSessionEnforcementView>,
    pub enforcement_unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportSubscriptionSessionsResponse {
    pub node_id: String,
    pub reported_count: usize,
    pub allowed_count: usize,
    pub blocked_count: usize,
    pub verdicts: Vec<SubscriptionSessionVerdictView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportSubscriptionSessionEnforcementResultRequest {
    pub action_id: String,
    pub session_id: String,
    pub status: SubscriptionSessionEnforcementStatus,
    pub runtime_session_ref: Option<String>,
    pub adapter: Option<SubscriptionSessionRuntimeAdapter>,
    pub session_absent_after_action: Option<bool>,
    pub verified_at_unix: Option<u64>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalSubscriptionSessionEnforcementCommand {
    pub action_id: String,
    pub session_id: String,
    pub action: SubscriptionSessionEnforcementAction,
    pub runtime_session_ref: String,
    pub reason: String,
    pub requires_absence_verification: bool,
    pub issued_at_unix: u64,
    pub expires_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompleteLocalSubscriptionSessionEnforcementRequest {
    pub status: SubscriptionSessionEnforcementStatus,
    pub runtime_session_ref: Option<String>,
    pub session_absent_after_action: Option<bool>,
    pub verified_at_unix: Option<u64>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterLocalSubscriptionSessionAdapterRequest {
    pub adapter_instance_id: String,
    #[serde(default)]
    pub runtime_capabilities: Vec<SubscriptionSessionRuntimeCapability>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalSubscriptionSessionAdapterLeaseView {
    pub adapter_instance_id: String,
    pub runtime_capabilities: Vec<SubscriptionSessionRuntimeCapability>,
    pub registered_at_unix: u64,
    pub lease_expires_at_unix: u64,
}

pub const SUBSCRIPTION_SESSION_RUNTIME_DRIVER_PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionSessionRuntimeDriverOperation {
    Handshake,
    Observe,
    Terminate,
    Verify,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SubscriptionSessionRuntimeDriverRequest {
    pub protocol_version: u16,
    pub operation: SubscriptionSessionRuntimeDriverOperation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_session_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionSessionRuntimeDriverResponse {
    pub protocol_version: u16,
    pub success: bool,
    #[serde(default)]
    pub runtime_capabilities: Vec<SubscriptionSessionRuntimeCapability>,
    #[serde(default)]
    pub observations: Vec<SubscriptionSessionObservation>,
    #[serde(default)]
    pub session_absent: Option<bool>,
    #[serde(default)]
    pub verified_at_unix: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WireGuardSessionMappingDocument {
    pub schema_version: u16,
    pub source_revision: String,
    pub created_at_unix: u64,
    pub interfaces: Vec<WireGuardSessionInterfaceMapping>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WireGuardSessionInterfaceMapping {
    pub interface_name: String,
    pub peers: Vec<WireGuardSessionPeerMapping>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WireGuardSessionPeerMapping {
    pub runtime_username: String,
    pub public_key: String,
    pub device_fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedProxyProfile {
    pub id: String,
    pub name: String,
    pub proxy_type: String,
    pub settings_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedInbound {
    pub tag: String,
    pub port: u16,
    pub protocol: String,
    pub network: String,
    pub tls_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedHost {
    pub id: String,
    pub remark: String,
    pub address: String,
    pub port: u16,
    pub path: Option<String>,
    pub sni: Option<String>,
    pub security: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedUserConfig {
    pub username: String,
    pub status: String,
    pub data_limit_bytes: Option<u64>,
    pub expire_at_unix: Option<u64>,
    pub subscription_token: String,
    pub proxy_profiles: Vec<GeneratedProxyProfile>,
    pub inbounds: Vec<GeneratedInbound>,
    pub hosts: Vec<GeneratedHost>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClusterNodeRole {
    Entry,
    Relay,
    Exit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedClusterConfig {
    pub id: String,
    pub name: String,
    pub status: String,
    pub revision: String,
    pub routing_policy_name: String,
    pub controlled_egress: bool,
    pub failover_enabled: bool,
    pub paths: Vec<GeneratedClusterPath>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedClusterPath {
    pub hops: Vec<GeneratedClusterHop>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedClusterHop {
    pub cluster_node_id: String,
    pub node_id: String,
    pub role: ClusterNodeRole,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GeneratedClusterNodeTarget {
    pub node_id: String,
    pub cluster_id: String,
    pub cluster_name: String,
    pub cluster_revision: String,
    pub cluster_node_id: String,
    pub role: ClusterNodeRole,
    pub upstream_node_ids: Vec<String>,
    pub downstream_node_ids: Vec<String>,
    pub route_edge_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodePeerDirection {
    Previous,
    Next,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeRoutePeer {
    pub direction: NodePeerDirection,
    pub opaque_peer_id: String,
    pub address: Option<String>,
    pub port: Option<u16>,
    pub public_key: Option<String>,
    pub sni: Option<String>,
    pub transport: Option<String>,
    #[serde(default)]
    pub security: Option<NodeRouteTransportSecurity>,
    #[serde(default)]
    pub auth: Option<NodeRoutePeerAuth>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeRouteListen {
    pub tag: String,
    pub port: u16,
    pub protocol: String,
    pub network: String,
    pub tls_enabled: bool,
    #[serde(default)]
    pub security: Option<NodeRouteTransportSecurity>,
    #[serde(default)]
    pub auth: Option<NodeRouteListenAuth>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeRouteSecurityMode {
    None,
    MutualTls,
    Reality,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeRouteTransportSecurity {
    pub mode: NodeRouteSecurityMode,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub server_name: Option<String>,
    #[serde(default)]
    pub public_key: Option<String>,
    #[serde(default)]
    pub short_id: Option<String>,
    #[serde(default)]
    pub fingerprint: Option<String>,
    #[serde(default)]
    pub certificate_pins: Vec<String>,
    #[serde(default)]
    pub credential_ref: Option<String>,
    #[serde(default)]
    pub allow_insecure: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeRoutePeerAuth {
    pub method: String,
    #[serde(default)]
    pub identity_ref: Option<String>,
    #[serde(default)]
    pub credential_ref: Option<String>,
    #[serde(default)]
    pub public_key: Option<String>,
    #[serde(default)]
    pub certificate_pins: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeRouteListenAuth {
    pub method: String,
    #[serde(default)]
    pub identity_ref: Option<String>,
    #[serde(default)]
    pub credential_ref: Option<String>,
    #[serde(default)]
    pub allowed_public_keys: Vec<String>,
    #[serde(default)]
    pub certificate_pins: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeRouteAssignment {
    pub node_id: String,
    pub route_id: String,
    pub cluster_id: String,
    pub cluster_revision: String,
    pub role: ClusterNodeRole,
    pub listen: Option<NodeRouteListen>,
    pub previous_peer: Option<NodeRoutePeer>,
    pub next_peer: Option<NodeRoutePeer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedCoreConfig {
    pub generated_at_unix: u64,
    pub revision: String,
    pub users: Vec<GeneratedUserConfig>,
    pub inbounds: Vec<GeneratedInbound>,
    pub hosts: Vec<GeneratedHost>,
    pub nodes: Vec<PanelNode>,
    #[serde(default)]
    pub clusters: Vec<GeneratedClusterConfig>,
    #[serde(default)]
    pub cluster_node_targets: Vec<GeneratedClusterNodeTarget>,
    #[serde(default)]
    pub node_route_assignments: Vec<NodeRouteAssignment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelNode {
    pub id: String,
    pub name: String,
    pub address: String,
    pub port: u16,
    pub api_port: u16,
    pub usage_coefficient: f64,
    pub enabled: bool,
    pub xray_version: Option<String>,
    pub node_version: Option<String>,
    pub status: NodeStatus,
    pub sync_status: NodeSyncStatus,
    pub provisioning_status: String,
    pub last_applied_revision: Option<String>,
    pub last_registered_at_unix: Option<u64>,
    pub last_heartbeat_at_unix: Option<u64>,
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
pub struct NodeAgentConfigResponse {
    pub node_id: String,
    pub revision: String,
    pub generated_config: GeneratedCoreConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRouteCredentialBundle {
    pub node_id: String,
    pub revision: String,
    pub generated_at_unix: u64,
    pub credentials: Vec<NodeRouteCredentialMaterial>,
    /// Reality material for this node's inbounds.
    ///
    /// Delivered over the same channel as mTLS private keys: the route
    /// authenticates with the node token and is not part of the admin surface.
    #[serde(default)]
    pub reality_materials: Vec<NodeRealityMaterial>,
}

/// Reality material for one inbound. Carries a private key: never log it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRealityMaterial {
    pub inbound_tag: String,
    pub private_key_b64: String,
    pub short_ids: Vec<String>,
    pub dest: String,
    pub server_names: Vec<String>,
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

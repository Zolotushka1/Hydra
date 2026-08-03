use serde::{Deserialize, Serialize};

use crate::{configgen::GeneratedUserConfig, network::Inbound};

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionFormat {
    Json,
    PlainText,
    Base64,
    DiagnosticJson,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SubscriptionQuery {
    pub format: Option<SubscriptionFormat>,
    #[serde(default)]
    pub device_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderedSubscription {
    pub username: String,
    #[serde(default)]
    pub subject_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    pub format: SubscriptionFormat,
    pub content_type: String,
    pub body: String,
    pub generated_config: GeneratedUserConfig,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionCatalogClientStatus {
    Active,
    Disabled,
    Expired,
    Revoked,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionDeviceStatus {
    Active,
    Revoked,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionDeviceEnrollmentStatus {
    Active,
    Consumed,
    Revoked,
    Expired,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionUsageWindowPreset {
    Hours12,
    Day1,
    Days3,
    Week1,
    Month1,
    Months3,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionCatalogPlan {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub created_at_unix: u64,
    pub updated_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionCatalogClient {
    pub id: String,
    pub plan_id: String,
    pub name: String,
    pub status: SubscriptionCatalogClientStatus,
    pub max_simultaneous_devices: Option<u16>,
    #[serde(default)]
    pub max_simultaneous_ips: Option<u16>,
    pub data_limit_bytes: Option<u64>,
    pub used_traffic_bytes: u64,
    pub expire_at_unix: Option<u64>,
    pub note: Option<String>,
    pub access_policy: SubscriptionClientNodeAccessPolicy,
    pub subscription_token: String,
    pub revoked_at_unix: Option<u64>,
    pub created_at_unix: u64,
    pub updated_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionClientDevice {
    pub id: String,
    pub client_id: String,
    pub fingerprint_hmac_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscription_token_hmac_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wireguard_public_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wireguard_fingerprint_hmac_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wireguard_allowed_ips: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wireguard_key_registered_at_unix: Option<u64>,
    pub label: Option<String>,
    pub platform: Option<String>,
    pub status: SubscriptionDeviceStatus,
    pub first_seen_at_unix: u64,
    pub last_seen_at_unix: u64,
    pub revoked_at_unix: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionClientDeviceView {
    pub id: String,
    pub client_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wireguard_public_key: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wireguard_allowed_ips: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wireguard_key_registered_at_unix: Option<u64>,
    pub label: Option<String>,
    pub platform: Option<String>,
    pub status: SubscriptionDeviceStatus,
    pub first_seen_at_unix: u64,
    pub last_seen_at_unix: u64,
    pub revoked_at_unix: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionClientNodeAccessPolicy {
    #[serde(default = "default_true")]
    pub allow_all_nodes: bool,
    #[serde(default)]
    pub node_ids: Vec<String>,
    #[serde(default)]
    pub cluster_ids: Vec<String>,
    #[serde(default = "default_true")]
    pub allow_all_protocols: bool,
    #[serde(default)]
    pub protocols: Vec<String>,
}

impl Default for SubscriptionClientNodeAccessPolicy {
    fn default() -> Self {
        Self {
            allow_all_nodes: true,
            node_ids: Vec::new(),
            cluster_ids: Vec::new(),
            allow_all_protocols: true,
            protocols: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSubscriptionPlanRequest {
    pub name: String,
    pub description: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSubscriptionPlanRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSubscriptionClientRequest {
    pub name: String,
    pub status: Option<SubscriptionCatalogClientStatus>,
    pub max_simultaneous_devices: Option<u16>,
    pub max_simultaneous_ips: Option<u16>,
    pub data_limit_bytes: Option<u64>,
    pub expire_at_unix: Option<u64>,
    pub note: Option<String>,
    pub access_policy: Option<SubscriptionClientNodeAccessPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSubscriptionClientRequest {
    pub name: Option<String>,
    pub status: Option<SubscriptionCatalogClientStatus>,
    pub max_simultaneous_devices: Option<u16>,
    pub max_simultaneous_ips: Option<u16>,
    pub data_limit_bytes: Option<u64>,
    pub expire_at_unix: Option<u64>,
    pub note: Option<String>,
    pub access_policy: Option<SubscriptionClientNodeAccessPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSubscriptionClientAccessRequest {
    pub access_policy: SubscriptionClientNodeAccessPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterSubscriptionDeviceRequest {
    pub fingerprint: String,
    pub label: Option<String>,
    pub platform: Option<String>,
    #[serde(default)]
    pub wireguard_public_key: Option<String>,
    #[serde(default)]
    pub wireguard_allowed_ips: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterSubscriptionDeviceResponse {
    pub admitted: bool,
    pub active_device_count: usize,
    pub max_simultaneous_devices: Option<u16>,
    pub device: Option<SubscriptionClientDeviceView>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionDeviceEnrollmentGrant {
    pub id: String,
    pub client_id: String,
    pub token_hmac_sha256: String,
    pub status: SubscriptionDeviceEnrollmentStatus,
    pub created_at_unix: u64,
    pub expires_at_unix: u64,
    pub consumed_at_unix: Option<u64>,
    pub consumed_device_id: Option<String>,
    pub revoked_at_unix: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionDeviceEnrollmentGrantView {
    pub id: String,
    pub client_id: String,
    pub status: SubscriptionDeviceEnrollmentStatus,
    pub created_at_unix: u64,
    pub expires_at_unix: u64,
    pub consumed_at_unix: Option<u64>,
    pub consumed_device_id: Option<String>,
    pub revoked_at_unix: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateSubscriptionDeviceEnrollmentRequest {
    pub expires_in_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSubscriptionDeviceEnrollmentResponse {
    pub grant: SubscriptionDeviceEnrollmentGrantView,
    pub enrollment_token: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SubscriptionDeviceEnrollmentsQuery {
    pub status: Option<SubscriptionDeviceEnrollmentStatus>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExchangeSubscriptionDeviceEnrollmentRequest {
    pub enrollment_token: String,
    pub device: RegisterSubscriptionDeviceRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeSubscriptionDeviceEnrollmentResponse {
    pub device: SubscriptionClientDeviceView,
    pub device_credential: String,
    pub subscription_path: String,
    pub subscription_formats: Vec<SubscriptionFormat>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SubscriptionDevicesQuery {
    pub status: Option<SubscriptionDeviceStatus>,
    pub limit: Option<usize>,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
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

#[derive(Debug, Clone, Deserialize)]
pub struct SubscriptionSessionsQuery {
    pub verdict: Option<SubscriptionSessionVerdict>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionClientSessionView {
    pub session_id: String,
    pub client_id: String,
    pub runtime_username: String,
    pub node_id: String,
    pub device_id: Option<String>,
    pub source_ip_present: bool,
    pub verdict: SubscriptionSessionVerdict,
    pub reason: String,
    pub enforcement: Option<SubscriptionSessionEnforcementView>,
    pub enforcement_unavailable_reason: Option<String>,
    pub connected_at_unix: Option<u64>,
    pub last_observed_at_unix: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SubscriptionCatalogQuery {
    pub search: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SubscriptionUsageQuery {
    pub window: Option<SubscriptionUsageWindowPreset>,
    pub from_unix: Option<u64>,
    pub to_unix: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportSubscriptionUsageRequest {
    pub at_unix: Option<u64>,
    pub node_id: Option<String>,
    pub cluster_id: Option<String>,
    pub bytes_downlink: u64,
    pub bytes_uplink: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionUsagePoint {
    pub at_unix: u64,
    pub node_id: Option<String>,
    pub cluster_id: Option<String>,
    pub bytes_downlink: u64,
    pub bytes_uplink: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionUsageDetail {
    pub client_id: String,
    pub window: SubscriptionUsageWindowPreset,
    pub from_unix: u64,
    pub to_unix: u64,
    pub total_downlink_bytes: u64,
    pub total_uplink_bytes: u64,
    pub points: Vec<SubscriptionUsagePoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionClientAccessPreview {
    pub schema_version: u16,
    pub client_id: String,
    pub plan_id: String,
    pub access_policy: SubscriptionClientNodeAccessPolicy,
    pub renderable: bool,
    pub allowed_inbounds: Vec<SubscriptionAccessInboundPreview>,
    pub denied_inbounds: Vec<SubscriptionAccessInboundPreview>,
    pub allowed_hosts: Vec<SubscriptionAccessHostPreview>,
    pub denied_hosts: Vec<SubscriptionAccessHostPreview>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionAccessInboundPreview {
    pub tag: String,
    pub protocol: String,
    pub port: u16,
    pub node_id: Option<String>,
    pub cluster_id: Option<String>,
    pub reason: String,
    pub inbound: Inbound,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionAccessHostPreview {
    pub id: String,
    pub remark: String,
    pub address: String,
    pub port: u16,
    pub node_id: Option<String>,
    pub cluster_id: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSubscriptionCatalog {
    pub plans: Vec<SubscriptionCatalogPlan>,
    pub clients: Vec<SubscriptionCatalogClient>,
    #[serde(default)]
    pub devices: Vec<SubscriptionClientDevice>,
    #[serde(default)]
    pub enrollment_grants: Vec<SubscriptionDeviceEnrollmentGrant>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSubscriptionUsage {
    #[serde(default)]
    pub points: Vec<PersistedSubscriptionUsagePoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSubscriptionUsagePoint {
    pub client_id: String,
    pub point: SubscriptionUsagePoint,
}

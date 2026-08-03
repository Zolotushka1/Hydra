use serde::{Deserialize, Serialize};

use crate::network::XhttpMode;
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XrayDnsConfigDocument {
    pub strategy: String,
    pub hosts: Vec<XrayDnsHostOverride>,
    pub servers: Vec<XrayDnsServer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XrayDnsHostOverride {
    pub domain: String,
    pub address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XrayDnsServer {
    pub address: String,
    pub domains: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XrayPolicyConfigDocument {
    pub levels: Vec<XrayPolicyLevel>,
    pub system: XrayPolicySystem,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XrayPolicyLevel {
    pub level: u32,
    pub handshake_seconds: u64,
    pub conn_idle_seconds: u64,
    pub uplink_only_seconds: u64,
    pub downlink_only_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XrayPolicySystem {
    pub stats_inbound_uplink: bool,
    pub stats_inbound_downlink: bool,
    pub stats_outbound_uplink: bool,
    pub stats_outbound_downlink: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XrayRoutingConfigDocument {
    pub domain_strategy: String,
    pub rules: Vec<XrayRoutingRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XrayRoutingRule {
    pub name: String,
    pub rule_type: String,
    pub domains: Vec<String>,
    pub outbound_tag: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XrayHostDocument {
    pub id: String,
    pub remark: String,
    pub address: String,
    pub port: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sni: Option<String>,
    pub security: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XrayInboundClientDocument {
    pub username: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_limit_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expire_at_unix: Option<u64>,
    pub subscription_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_password: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_method: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XrayStreamSettingsDocument {
    pub network: String,
    pub security: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_name: Option<String>,
    pub allow_insecure: bool,
    /// XHTTP mode. Meaningful only when `network == "xhttp"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xhttp_mode: Option<XhttpMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XraySecuritySettingsDocument {
    pub mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub certificate_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reality_public_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reality_short_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XrayInboundDocument {
    pub tag: String,
    pub port: u16,
    pub protocol: String,
    pub network: String,
    pub tls_enabled: bool,
    pub stream_settings: XrayStreamSettingsDocument,
    pub security_settings: XraySecuritySettingsDocument,
    pub clients: Vec<XrayInboundClientDocument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XrayOutboundDocument {
    pub tag: String,
    pub protocol: String,
    pub transport: String,
    pub settings_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XrayUserBindingDocument {
    pub username: String,
    pub inbound_tags: Vec<String>,
    pub host_ids: Vec<String>,
    pub outbound_tags: Vec<String>,
    pub policy_level: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XrayNodeDocument {
    pub id: String,
    pub name: String,
    pub address: String,
    pub api_port: u16,
    pub enabled: bool,
    pub usage_coefficient: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XrayClusterDocument {
    pub id: String,
    pub name: String,
    pub status: String,
    pub revision: String,
    pub controlled_egress: bool,
    pub failover_enabled: bool,
    pub paths: Vec<XrayClusterPathDocument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XrayClusterPathDocument {
    pub hops: Vec<XrayClusterHopDocument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XrayClusterHopDocument {
    pub cluster_node_id: String,
    pub node_id: String,
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XrayClusterNodeTargetDocument {
    pub node_id: String,
    pub cluster_id: String,
    pub cluster_name: String,
    pub cluster_revision: String,
    pub cluster_node_id: String,
    pub role: String,
    pub upstream_node_ids: Vec<String>,
    pub downstream_node_ids: Vec<String>,
    pub route_edge_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XrayRawConfigValidationIssue {
    pub path: String,
    pub severity: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XrayRawConfigValidationReport {
    pub valid: bool,
    pub checked_at_unix: u64,
    pub issue_count: usize,
    pub issues: Vec<XrayRawConfigValidationIssue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum XrayExternalValidationStatus {
    Passed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XrayExternalValidationReport {
    pub status: XrayExternalValidationStatus,
    pub checked_at_unix: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary_path: Option<String>,
    pub internal_validation_valid: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub detail: String,
    pub config_retained: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XrayConfigDocument {
    pub generated_at_unix: u64,
    pub revision: String,
    pub raw_config_validation: XrayRawConfigValidationReport,
    pub raw_config: Value,
    pub dns: XrayDnsConfigDocument,
    pub routing: XrayRoutingConfigDocument,
    pub policy: XrayPolicyConfigDocument,
    pub inbounds: Vec<XrayInboundDocument>,
    pub outbounds: Vec<XrayOutboundDocument>,
    pub hosts: Vec<XrayHostDocument>,
    pub user_bindings: Vec<XrayUserBindingDocument>,
    pub nodes: Vec<XrayNodeDocument>,
    pub clusters: Vec<XrayClusterDocument>,
    pub cluster_node_targets: Vec<XrayClusterNodeTargetDocument>,
}

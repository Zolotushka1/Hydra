use serde::{Deserialize, Serialize};

use crate::{cluster::ClusterNodeRole, network::Inbound, node::Node, user::UserStatus};

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedProxyProfile {
    pub id: String,
    pub name: String,
    pub proxy_type: String,
    pub settings_json: String,
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
    /// Public half of the Reality material of the inbound this host serves. Not a
    /// secret: a client needs it to build its link. The private key never reaches
    /// this struct — it travels to the node over
    /// `/api/node-agent/route-credentials`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reality_public_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reality_short_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cluster_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedSubscriptionAccessPolicy {
    pub allow_all_nodes: bool,
    pub node_ids: Vec<String>,
    pub cluster_ids: Vec<String>,
    #[serde(default = "default_true")]
    pub allow_all_protocols: bool,
    #[serde(default)]
    pub protocols: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedUserConfig {
    pub username: String,
    pub status: UserStatus,
    pub data_limit_bytes: Option<u64>,
    pub expire_at_unix: Option<u64>,
    pub subscription_token: String,
    pub proxy_profiles: Vec<GeneratedProxyProfile>,
    pub inbounds: Vec<Inbound>,
    pub hosts: Vec<GeneratedHost>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_policy: Option<GeneratedSubscriptionAccessPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedCoreConfigPreview {
    pub generated_at_unix: u64,
    pub revision: String,
    pub users: Vec<GeneratedUserConfig>,
    pub inbounds: Vec<Inbound>,
    pub hosts: Vec<GeneratedHost>,
    pub nodes: Vec<Node>,
    pub clusters: Vec<GeneratedClusterConfig>,
    pub cluster_node_targets: Vec<GeneratedClusterNodeTarget>,
    pub node_route_assignments: Vec<NodeRouteAssignment>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

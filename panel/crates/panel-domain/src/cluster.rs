use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClusterStatus {
    Draft,
    Active,
    Disabled,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClusterNodeRole {
    Entry,
    Relay,
    Exit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterNode {
    pub id: String,
    pub node_id: String,
    pub role: ClusterNodeRole,
    pub position_x: f32,
    pub position_y: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterEdge {
    pub id: String,
    pub from_cluster_node_id: String,
    pub to_cluster_node_id: String,
    pub priority: u16,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterRoutingPolicy {
    pub name: String,
    pub description: Option<String>,
    pub prefer_domestic_entry: bool,
    pub controlled_egress: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterFailoverPolicy {
    pub enabled: bool,
    pub max_failover_hops: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cluster {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub status: ClusterStatus,
    pub nodes: Vec<ClusterNode>,
    pub edges: Vec<ClusterEdge>,
    pub routing_policy: ClusterRoutingPolicy,
    pub failover_policy: ClusterFailoverPolicy,
    pub revision: String,
    pub created_at_unix: u64,
    pub updated_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateClusterRequest {
    pub name: String,
    pub description: Option<String>,
    pub status: Option<ClusterStatus>,
    pub nodes: Vec<CreateClusterNodeRequest>,
    pub edges: Vec<CreateClusterEdgeRequest>,
    pub routing_policy: Option<ClusterRoutingPolicy>,
    pub failover_policy: Option<ClusterFailoverPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateClusterRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub status: Option<ClusterStatus>,
    pub nodes: Option<Vec<CreateClusterNodeRequest>>,
    pub edges: Option<Vec<CreateClusterEdgeRequest>>,
    pub routing_policy: Option<ClusterRoutingPolicy>,
    pub failover_policy: Option<ClusterFailoverPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateClusterNodeRequest {
    pub id: Option<String>,
    pub node_id: String,
    pub role: ClusterNodeRole,
    pub position_x: Option<f32>,
    pub position_y: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateClusterEdgeRequest {
    pub id: Option<String>,
    pub from_cluster_node_id: String,
    pub to_cluster_node_id: String,
    pub priority: Option<u16>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterPreview {
    pub cluster_id: String,
    pub revision: String,
    pub path_count: usize,
    pub paths: Vec<ClusterPathPreview>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterPathPreview {
    pub hops: Vec<ClusterPathHop>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterPathHop {
    pub cluster_node_id: String,
    pub node_id: String,
    pub role: ClusterNodeRole,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterValidationReport {
    pub cluster_id: String,
    pub valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedClusters {
    pub clusters: Vec<Cluster>,
}

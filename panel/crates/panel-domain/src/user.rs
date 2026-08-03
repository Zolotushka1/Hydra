use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UserStatus {
    Active,
    Disabled,
    Expired,
    OnHold,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub username: String,
    pub status: UserStatus,
    pub data_limit_bytes: Option<u64>,
    pub used_traffic_bytes: u64,
    pub expire_at_unix: Option<u64>,
    pub note: Option<String>,
    pub template_id: Option<String>,
    pub next_template_id: Option<String>,
    pub proxy_profile_ids: Vec<String>,
    pub excluded_inbound_tags: Vec<String>,
    pub subscription_token: String,
    pub sub_revoked_at_unix: Option<u64>,
    pub created_at_unix: u64,
    pub updated_at_unix: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UsersQuery {
    pub status: Option<UserStatus>,
    pub search: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UserActivityKind {
    Created,
    Updated,
    Deleted,
    UsageReset,
    UsageReported,
    SubscriptionRevoked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserActivityEntry {
    pub username: String,
    pub kind: UserActivityKind,
    pub actor_username: Option<String>,
    pub detail: String,
    pub traffic_delta_bytes: Option<u64>,
    pub total_used_traffic_bytes: Option<u64>,
    pub created_at_unix: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UserActivityQuery {
    pub kind: Option<UserActivityKind>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub template_id: Option<String>,
    pub next_template_id: Option<String>,
    pub status: Option<UserStatus>,
    pub data_limit_bytes: Option<u64>,
    pub expire_at_unix: Option<u64>,
    pub note: Option<String>,
    pub proxy_profile_ids: Option<Vec<String>>,
    pub excluded_inbound_tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateUserRequest {
    pub template_id: Option<String>,
    pub next_template_id: Option<String>,
    pub status: Option<UserStatus>,
    pub data_limit_bytes: Option<u64>,
    pub expire_at_unix: Option<u64>,
    pub note: Option<String>,
    pub proxy_profile_ids: Option<Vec<String>>,
    pub excluded_inbound_tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportUserUsageRequest {
    pub bytes_delta: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedUsers {
    pub users: Vec<User>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSubscriptionView {
    pub username: String,
    pub subscription_token: String,
    pub subscription_path: String,
    pub revoked_at_unix: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserConfigPreview {
    pub username: String,
    pub status: UserStatus,
    pub proxy_profiles: Vec<UserConfigProxyProfile>,
    pub available_inbounds: Vec<String>,
    pub excluded_inbound_tags: Vec<String>,
    pub hosts: Vec<UserConfigHost>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserConfigProxyProfile {
    pub id: String,
    pub name: String,
    pub proxy_type: String,
    pub excluded_inbound_tags: Vec<String>,
    pub settings_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserConfigHost {
    pub id: String,
    pub remark: String,
    pub address: String,
    pub port: u16,
    pub path: Option<String>,
    pub sni: Option<String>,
    pub security: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserTemplate {
    pub id: String,
    pub name: String,
    pub status: UserStatus,
    pub data_limit_bytes: Option<u64>,
    pub expire_duration_seconds: Option<u64>,
    pub note: Option<String>,
    pub proxy_profile_ids: Vec<String>,
    pub excluded_inbound_tags: Vec<String>,
    pub created_at_unix: u64,
    pub updated_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUserTemplateRequest {
    pub name: String,
    pub status: UserStatus,
    pub data_limit_bytes: Option<u64>,
    pub expire_duration_seconds: Option<u64>,
    pub note: Option<String>,
    pub proxy_profile_ids: Vec<String>,
    pub excluded_inbound_tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateUserTemplateRequest {
    pub name: Option<String>,
    pub status: Option<UserStatus>,
    pub data_limit_bytes: Option<u64>,
    pub expire_duration_seconds: Option<u64>,
    pub note: Option<String>,
    pub proxy_profile_ids: Option<Vec<String>>,
    pub excluded_inbound_tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedUserTemplates {
    pub templates: Vec<UserTemplate>,
}

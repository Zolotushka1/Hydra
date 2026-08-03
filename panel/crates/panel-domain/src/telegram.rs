use serde::{Deserialize, Serialize};

use crate::system::{AlertEventStatus, AlertKind, AlertSeverity};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramSettings {
    pub enabled: bool,
    pub bot_token_configured: bool,
    pub default_chat_id: Option<String>,
    pub notify_on_security_events: bool,
    pub notify_on_system_alerts: bool,
    pub notify_on_node_events: bool,
    #[serde(default = "default_true")]
    pub notify_on_node_health_alerts: bool,
    #[serde(default = "default_alert_notification_policy")]
    pub alert_policy: AlertNotificationPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateTelegramSettingsRequest {
    pub enabled: bool,
    pub bot_token: Option<String>,
    pub default_chat_id: Option<String>,
    pub notify_on_security_events: bool,
    pub notify_on_system_alerts: bool,
    pub notify_on_node_events: bool,
    #[serde(default = "default_true")]
    pub notify_on_node_health_alerts: bool,
    #[serde(default = "default_alert_notification_policy")]
    pub alert_policy: AlertNotificationPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedTelegramSettings {
    pub enabled: bool,
    #[serde(default)]
    pub bot_token: Option<String>,
    #[serde(default)]
    pub bot_token_ciphertext_b64: Option<String>,
    #[serde(default)]
    pub bot_token_nonce_b64: Option<String>,
    pub default_chat_id: Option<String>,
    pub notify_on_security_events: bool,
    pub notify_on_system_alerts: bool,
    pub notify_on_node_events: bool,
    #[serde(default = "default_true")]
    pub notify_on_node_health_alerts: bool,
    #[serde(default = "default_alert_notification_policy")]
    pub alert_policy: AlertNotificationPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AlertNotificationPolicy {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub notify_on_activation: bool,
    #[serde(default = "default_true")]
    pub notify_on_resolution: bool,
    #[serde(default = "default_alert_min_severity")]
    pub min_severity: AlertSeverity,
    #[serde(default = "default_included_alert_kinds")]
    pub included_alert_kinds: Vec<AlertKind>,
    #[serde(default = "default_alert_cooldown_seconds")]
    pub cooldown_seconds: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TelegramEventKind {
    TestMessage,
    Security,
    SystemAlert,
    Node,
    NodeHealthAlert,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TelegramEventStatus {
    Queued,
    Delivered,
    RetryScheduled,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramEvent {
    #[serde(default)]
    pub id: String,
    pub kind: TelegramEventKind,
    pub status: TelegramEventStatus,
    #[serde(default)]
    pub alert_kind: Option<AlertKind>,
    #[serde(default)]
    pub alert_severity: Option<AlertSeverity>,
    #[serde(default)]
    pub alert_status: Option<AlertEventStatus>,
    pub message: String,
    pub target_chat_id: Option<String>,
    #[serde(default)]
    pub attempt_count: u8,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub next_retry_at_unix: Option<u64>,
    #[serde(default)]
    pub delivered_at_unix: Option<u64>,
    pub created_at_unix: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TelegramEventsQuery {
    pub kind: Option<TelegramEventKind>,
    pub status: Option<TelegramEventStatus>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramTestRequest {
    pub message: Option<String>,
    pub chat_id: Option<String>,
}

fn default_true() -> bool {
    true
}

pub fn default_alert_notification_policy() -> AlertNotificationPolicy {
    AlertNotificationPolicy {
        enabled: true,
        notify_on_activation: true,
        notify_on_resolution: true,
        min_severity: default_alert_min_severity(),
        included_alert_kinds: default_included_alert_kinds(),
        cooldown_seconds: default_alert_cooldown_seconds(),
    }
}

fn default_alert_min_severity() -> AlertSeverity {
    AlertSeverity::Warning
}

fn default_included_alert_kinds() -> Vec<AlertKind> {
    vec![
        AlertKind::DiskUsage,
        AlertKind::MemoryUsage,
        AlertKind::PanelMemoryBudget,
        AlertKind::NodeOffline,
        AlertKind::NodeStaleHeartbeat,
        AlertKind::NodeConfigDrift,
        AlertKind::NodeProvisioningStale,
        AlertKind::NodeProvisioningFailed,
        AlertKind::NodeReportedApplyFailed,
        AlertKind::NodeRuntimeAlert,
    ]
}

fn default_alert_cooldown_seconds() -> u64 {
    300
}

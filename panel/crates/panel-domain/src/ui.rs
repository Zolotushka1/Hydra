use serde::{Deserialize, Serialize};

use crate::{
    cluster::ClusterStatus,
    installer::PanelAccessMode,
    network::{InboundTransport, ProtocolSecurityMode, ProtocolSupportStatus},
    node::{NodeHealthCenterSummary, NodeProvisioningStatus, NodeStatus, NodeSyncStatus},
    security::{AuthenticatedAdmin, SecuritySettings, TwoFactorState},
    system::{CoreRuntimeState, SystemOverview},
    telegram::TelegramSettings,
    user::UserStatus,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiBootstrapSnapshot {
    pub schema_version: u16,
    pub admin: AuthenticatedAdmin,
    pub security: UiSecuritySnapshot,
    pub system: SystemOverview,
    pub core: UiCoreSnapshot,
    pub users: UiUsersSummary,
    pub nodes: UiNodesSummary,
    pub clusters: UiClustersSummary,
    #[serde(default)]
    pub telegram: UiTelegramSummary,
    #[serde(default)]
    pub audit: UiAuditSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiOverviewSnapshot {
    pub schema_version: u16,
    pub checked_at_unix: u64,
    pub system: SystemOverview,
    pub core: UiCoreSnapshot,
    pub users: UiUsersSummary,
    pub nodes: UiNodesSummary,
    pub clusters: UiClustersSummary,
    pub node_health: NodeHealthCenterSummary,
    pub node_health_recommendations: Vec<String>,
    #[serde(default)]
    pub telegram: UiTelegramSummary,
    #[serde(default)]
    pub audit: UiAuditSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiContractsSnapshot {
    pub schema_version: u16,
    pub checked_at_unix: u64,
    pub api_version: String,
    pub schemas: Vec<UiSchemaVersion>,
    pub endpoint_groups: Vec<UiEndpointGroup>,
    pub enums: Vec<UiEnumValues>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiSchemaVersion {
    pub name: String,
    pub version: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiEndpointGroup {
    pub group: String,
    pub endpoints: Vec<UiEndpoint>,
}

/// One admin-surface route inside the contract document.
///
/// A projection of a `ROUTE_TABLE` row, built only from it and never written by
/// hand.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiEndpoint {
    pub method: String,
    pub path: String,
    /// Accepts `?limit=` and returns a truncated list, so the frontend can page
    /// rather than assume it received everything.
    pub paginated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiEnumValues {
    pub name: String,
    pub values: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiSecuritySnapshot {
    pub settings: SecuritySettings,
    pub two_factor: TwoFactorState,
    pub active_ban_count: usize,
    #[serde(default)]
    pub active_admin_session_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiCoreSnapshot {
    pub runtime: CoreRuntimeState,
    pub generated_revision: String,
    pub config_valid_json: bool,
    pub config_saved_at_unix: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UiUsersSummary {
    pub total: usize,
    pub active: usize,
    pub disabled: usize,
    pub expired: usize,
    pub on_hold: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UiNodesSummary {
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
    pub provisioning_failed: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UiClustersSummary {
    pub total: usize,
    pub active: usize,
    pub draft: usize,
    pub disabled: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UiTelegramSummary {
    pub settings: Option<TelegramSettings>,
    pub total_events_buffered: usize,
    pub queued: usize,
    pub delivered: usize,
    pub retry_scheduled: usize,
    pub skipped: usize,
    pub failed: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UiAuditSummary {
    pub total_events_buffered: usize,
    pub latest_event_at_unix: Option<u64>,
}

/// Subscription catalog summary.
///
/// Counters only: the catalog is the largest collection in the panel, so
/// `/api/ui/*` must not return lists from it. Lists are paged through
/// `/api/subscription-plans` and `/api/subscription-plans/{id}/clients`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UiSubscriptionsSummary {
    pub plans_total: usize,
    pub clients_total: usize,
    pub clients_active: usize,
    pub clients_disabled: usize,
    pub clients_expired: usize,
    pub clients_revoked: usize,
    /// Active clients expiring within the next 24 hours.
    pub clients_expiring_within_day: usize,
    /// Active clients that have reached their data limit.
    pub clients_data_limit_reached: usize,
    pub devices_total: usize,
    pub devices_active: usize,
    pub devices_revoked: usize,
    pub enrollment_grants_active: usize,
    pub enrollment_grants_consumed: usize,
    pub enrollment_grants_expired: usize,
    pub enrollment_grants_revoked: usize,
}

/// Compact projection of `protocol_capabilities` for gating forms.
///
/// The full capability matrix is heavy: every protocol carries modes, required
/// binaries and secret classes. Rendering a form only needs to know what is
/// available and over which transports.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UiProtocolsSummary {
    /// Version of the full matrix this projects, so the frontend can tell when
    /// `/api/protocol-capabilities` is worth re-reading.
    pub capabilities_schema_version: u16,
    /// Xray version on the panel side, as reported by the binary itself.
    ///
    /// Not a convenience: XHTTP is under active development and server and client
    /// versions must match. A mismatch fails without a useful error, so the
    /// operator needs this next to the transport choice. `None` means the binary
    /// is not configured or was not queried.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xray_version: Option<String>,
    pub protocols: Vec<UiProtocolOption>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiProtocolOption {
    pub protocol: String,
    pub display_name: String,
    pub status: ProtocolSupportStatus,
    pub recommended_default: bool,
    /// Production-ready and not disabled: the form may be opened.
    pub available: bool,
    pub disabled_reason: Option<String>,
    pub supported_transports: Vec<InboundTransport>,
    pub supported_security_modes: Vec<ProtocolSecurityMode>,
}

/// State of the first-run setup wizard.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UiInstallerSummary {
    /// TLS material is configured, so the panel serves HTTPS.
    pub tls_configured: bool,
    pub available_access_modes: Vec<PanelAccessMode>,
    pub recommended_access_mode: Option<PanelAccessMode>,
    pub jobs_total: usize,
    pub jobs_pending: usize,
    pub jobs_running: usize,
    pub jobs_succeeded: usize,
    pub jobs_failed: usize,
    pub jobs_rejected: usize,
    pub jobs_expired: usize,
    pub latest_job_at_unix: Option<u64>,
}

impl UiUsersSummary {
    pub fn count_status(&mut self, status: UserStatus) {
        self.total += 1;
        match status {
            UserStatus::Active => self.active += 1,
            UserStatus::Disabled => self.disabled += 1,
            UserStatus::Expired => self.expired += 1,
            UserStatus::OnHold => self.on_hold += 1,
        }
    }
}

impl UiNodesSummary {
    pub fn count_node(
        &mut self,
        enabled: bool,
        status: NodeStatus,
        sync_status: NodeSyncStatus,
        provisioning_status: NodeProvisioningStatus,
    ) {
        self.total += 1;
        if enabled {
            self.enabled += 1;
        } else {
            self.disabled += 1;
        }

        match status {
            NodeStatus::Healthy => self.healthy += 1,
            NodeStatus::Degraded => self.degraded += 1,
            NodeStatus::Offline => self.offline += 1,
            NodeStatus::Unknown => self.unknown += 1,
        }

        match sync_status {
            NodeSyncStatus::Synced => self.synced += 1,
            NodeSyncStatus::Drifted => self.drifted += 1,
            NodeSyncStatus::Pending => self.pending += 1,
            NodeSyncStatus::Unknown => {}
        }

        match provisioning_status {
            NodeProvisioningStatus::Running => self.provisioning_running += 1,
            NodeProvisioningStatus::Failed => self.provisioning_failed += 1,
            NodeProvisioningStatus::None
            | NodeProvisioningStatus::Pending
            | NodeProvisioningStatus::Completed => {}
        }
    }
}

impl UiClustersSummary {
    pub fn count_status(&mut self, status: ClusterStatus) {
        self.total += 1;
        match status {
            ClusterStatus::Active => self.active += 1,
            ClusterStatus::Draft => self.draft += 1,
            ClusterStatus::Disabled => self.disabled += 1,
        }
    }
}

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeLimits {
    pub memory_budget_mb: usize,
    pub max_audit_events_buffered: usize,
    pub max_alert_events_buffered: usize,
    pub max_core_apply_events_buffered: usize,
    pub max_node_sync_events_buffered: usize,
    pub max_node_bootstrap_events_buffered: usize,
    pub max_node_provisioning_events_buffered: usize,
    pub max_panel_installer_jobs_buffered: usize,
    pub max_telegram_events_buffered: usize,
    pub max_user_activity_events_buffered: usize,
    pub max_subscription_usage_points_buffered: usize,
    pub max_subscription_devices_buffered: usize,
    pub max_subscription_enrollment_grants_buffered: usize,
    pub max_subscription_sessions_buffered: usize,
    pub max_operational_log_lines_buffered: usize,
    pub max_node_log_lines_per_request: usize,
    pub max_tracked_login_ips: usize,
    pub max_active_sessions: usize,
    /// Maximum page size for list APIs.
    ///
    /// Applies to collections without a buffer cap of their own: users, plans,
    /// clients, devices, grants, sessions. `?limit=` can only narrow a result;
    /// asking for more is refused, otherwise the response stops being bounded
    /// under the 512 MB budget.
    pub max_list_page_size: usize,
}

impl Default for RuntimeLimits {
    fn default() -> Self {
        Self {
            memory_budget_mb: 512,
            max_audit_events_buffered: 750,
            max_alert_events_buffered: 300,
            max_core_apply_events_buffered: 300,
            max_node_sync_events_buffered: 300,
            max_node_bootstrap_events_buffered: 300,
            max_node_provisioning_events_buffered: 300,
            max_panel_installer_jobs_buffered: 100,
            max_telegram_events_buffered: 300,
            max_user_activity_events_buffered: 1_000,
            max_subscription_usage_points_buffered: 1_000,
            max_subscription_devices_buffered: 2_000,
            max_subscription_enrollment_grants_buffered: 1_000,
            max_subscription_sessions_buffered: 2_000,
            max_operational_log_lines_buffered: 500,
            max_node_log_lines_per_request: 250,
            max_tracked_login_ips: 5_000,
            max_active_sessions: 128,
            max_list_page_size: 200,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapAdminConfig {
    pub username: Option<String>,
    pub password: Option<String>,
    pub password_hash: Option<String>,
    pub two_factor_secret_base32: Option<String>,
    pub two_factor_enabled: bool,
    pub two_factor_two_step_enabled: bool,
    pub two_factor_confirmed_at_unix: Option<u64>,
}

impl Default for BootstrapAdminConfig {
    fn default() -> Self {
        Self {
            username: std::env::var("HYDRA_BOOTSTRAP_ADMIN_USERNAME").ok(),
            password: std::env::var("HYDRA_BOOTSTRAP_ADMIN_PASSWORD").ok(),
            password_hash: std::env::var("HYDRA_BOOTSTRAP_ADMIN_PASSWORD_HASH").ok(),
            two_factor_secret_base32: None,
            two_factor_enabled: false,
            two_factor_two_step_enabled: false,
            two_factor_confirmed_at_unix: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyTrustConfig {
    pub trust_x_forwarded_for: bool,
    pub trusted_proxy_ips: Vec<String>,
    pub trusted_proxy_cidrs: Vec<String>,
}

/// Deny-by-default proxy trust.
///
/// Written out rather than derived, even though `derive(Default)` produces the
/// same values. `trust_x_forwarded_for: false` is a security decision, and a
/// reader checking what an unconfigured panel trusts should find the answer here
/// instead of inferring it from `bool`'s default.
#[allow(clippy::derivable_impls)]
impl Default for ProxyTrustConfig {
    fn default() -> Self {
        Self {
            trust_x_forwarded_for: false,
            trusted_proxy_ips: Vec::new(),
            trusted_proxy_cidrs: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    pub settings_path: String,
    pub admin_path: String,
    pub admin_secrets_key_path: String,
    pub audit_log_path: String,
    pub bootstrap_admin: BootstrapAdminConfig,
    pub proxy_trust: ProxyTrustConfig,
    pub login_protection_enabled: bool,
    pub smart_ban_enabled: bool,
    pub max_failed_attempts: usize,
    pub attempt_window_seconds: u64,
    pub block_for_seconds: u64,
    pub session_ttl_seconds: u64,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            settings_path: std::env::var("HYDRA_SECURITY_SETTINGS_PATH")
                .unwrap_or_else(|_| "data/security-settings.json".to_string()),
            admin_path: std::env::var("HYDRA_ADMIN_PATH")
                .unwrap_or_else(|_| "data/admin.json".to_string()),
            admin_secrets_key_path: std::env::var("HYDRA_ADMIN_SECRETS_KEY_PATH")
                .unwrap_or_else(|_| "data/admin-secrets.key".to_string()),
            audit_log_path: std::env::var("HYDRA_AUDIT_LOG_PATH")
                .unwrap_or_else(|_| "data/security-audit.ndjson".to_string()),
            bootstrap_admin: BootstrapAdminConfig::default(),
            proxy_trust: ProxyTrustConfig::default(),
            login_protection_enabled: true,
            smart_ban_enabled: true,
            max_failed_attempts: 3,
            attempt_window_seconds: 300,
            block_for_seconds: 30,
            session_ttl_seconds: 86_400,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoringConfig {
    pub thresholds_path: String,
    pub alert_history_path: String,
    pub disk_warning_percent: u8,
    pub disk_critical_percent: u8,
    pub memory_warning_percent: u8,
    pub memory_critical_percent: u8,
}

impl Default for MonitoringConfig {
    fn default() -> Self {
        Self {
            thresholds_path: std::env::var("HYDRA_MONITORING_THRESHOLDS_PATH")
                .unwrap_or_else(|_| "data/monitoring-thresholds.json".to_string()),
            alert_history_path: std::env::var("HYDRA_ALERT_HISTORY_PATH")
                .unwrap_or_else(|_| "data/system-alerts.ndjson".to_string()),
            disk_warning_percent: 80,
            disk_critical_percent: 90,
            memory_warning_percent: 80,
            memory_critical_percent: 90,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub bind_addr: String,
    pub tls_certificate_path: Option<String>,
    pub tls_private_key_path: Option<String>,
    pub core_config_path: String,
    pub xray_binary_path: Option<String>,
    pub xray_validation_temp_dir: String,
    pub xray_stats_poll_interval_seconds: u64,
    pub xray_activity_window_seconds: u64,
    pub xray_release_api_url: String,
    pub xray_update_work_dir: String,
    pub xray_update_max_download_bytes: u64,
    pub operational_log_path: String,
    pub core_apply_history_path: String,
    pub node_sync_history_path: String,
    pub node_apply_results_path: String,
    pub node_bootstrap_history_path: String,
    pub node_provisioning_tasks_path: String,
    pub node_provisioning_events_path: String,
    pub panel_installer_jobs_path: String,
    pub telegram_settings_path: String,
    pub telegram_secrets_key_path: String,
    pub telegram_events_path: String,
    pub user_activity_log_path: String,
    pub users_path: String,
    pub user_templates_path: String,
    pub subscription_catalog_path: String,
    pub subscription_usage_path: String,
    pub subscription_devices_key_path: String,
    pub network_resources_path: String,
    pub clusters_path: String,
    pub route_materials_path: String,
    pub route_materials_key_path: String,
    /// Per-inbound Reality material: x25519 key pairs and short ids. Private keys
    /// are stored encrypted only.
    pub reality_materials_path: String,
    pub reality_materials_key_path: String,
    pub node_secrets_key_path: String,
    pub nodes_path: String,
    pub runtime_limits: RuntimeLimits,
    pub security: SecurityConfig,
    pub monitoring: MonitoringConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            bind_addr: std::env::var("HYDRA_BIND_ADDR")
                .unwrap_or_else(|_| "0.0.0.0:8080".to_string()),
            tls_certificate_path: std::env::var("HYDRA_TLS_CERT_PATH")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            tls_private_key_path: std::env::var("HYDRA_TLS_KEY_PATH")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            core_config_path: std::env::var("HYDRA_CORE_CONFIG_PATH")
                .unwrap_or_else(|_| "data/core-config.json".to_string()),
            xray_binary_path: std::env::var("HYDRA_XRAY_BINARY_PATH")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            xray_validation_temp_dir: std::env::var("HYDRA_XRAY_VALIDATION_TEMP_DIR")
                .unwrap_or_else(|_| "data/xray-validation".to_string()),
            xray_stats_poll_interval_seconds: std::env::var(
                "HYDRA_XRAY_STATS_POLL_INTERVAL_SECONDS",
            )
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(15)
            .clamp(5, 300),
            xray_activity_window_seconds: std::env::var("HYDRA_XRAY_ACTIVITY_WINDOW_SECONDS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(120)
                .clamp(15, 3_600),
            xray_release_api_url: std::env::var("HYDRA_XRAY_RELEASE_API_URL").unwrap_or_else(
                |_| "https://api.github.com/repos/XTLS/Xray-core/releases/latest".to_string(),
            ),
            xray_update_work_dir: std::env::var("HYDRA_XRAY_UPDATE_WORK_DIR")
                .unwrap_or_else(|_| "data/xray-updates".to_string()),
            xray_update_max_download_bytes: std::env::var("HYDRA_XRAY_UPDATE_MAX_DOWNLOAD_BYTES")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(128 * 1024 * 1024),
            operational_log_path: std::env::var("HYDRA_OPERATIONAL_LOG_PATH")
                .unwrap_or_else(|_| "data/operational.ndjson".to_string()),
            core_apply_history_path: std::env::var("HYDRA_CORE_APPLY_HISTORY_PATH")
                .unwrap_or_else(|_| "data/core-apply-history.ndjson".to_string()),
            node_sync_history_path: std::env::var("HYDRA_NODE_SYNC_HISTORY_PATH")
                .unwrap_or_else(|_| "data/node-sync-history.ndjson".to_string()),
            node_apply_results_path: std::env::var("HYDRA_NODE_APPLY_RESULTS_PATH")
                .unwrap_or_else(|_| "data/node-apply-results.ndjson".to_string()),
            node_bootstrap_history_path: std::env::var("HYDRA_NODE_BOOTSTRAP_HISTORY_PATH")
                .unwrap_or_else(|_| "data/node-bootstrap-history.ndjson".to_string()),
            node_provisioning_tasks_path: std::env::var("HYDRA_NODE_PROVISIONING_TASKS_PATH")
                .unwrap_or_else(|_| "data/node-provisioning-tasks.json".to_string()),
            node_provisioning_events_path: std::env::var("HYDRA_NODE_PROVISIONING_EVENTS_PATH")
                .unwrap_or_else(|_| "data/node-provisioning-events.ndjson".to_string()),
            panel_installer_jobs_path: std::env::var("HYDRA_PANEL_INSTALLER_JOBS_PATH")
                .unwrap_or_else(|_| "data/panel-installer-jobs.json".to_string()),
            telegram_settings_path: std::env::var("HYDRA_TELEGRAM_SETTINGS_PATH")
                .unwrap_or_else(|_| "data/telegram-settings.json".to_string()),
            telegram_secrets_key_path: std::env::var("HYDRA_TELEGRAM_SECRETS_KEY_PATH")
                .unwrap_or_else(|_| "data/telegram-secrets.key".to_string()),
            telegram_events_path: std::env::var("HYDRA_TELEGRAM_EVENTS_PATH")
                .unwrap_or_else(|_| "data/telegram-events.ndjson".to_string()),
            user_activity_log_path: std::env::var("HYDRA_USER_ACTIVITY_LOG_PATH")
                .unwrap_or_else(|_| "data/user-activity.ndjson".to_string()),
            users_path: std::env::var("HYDRA_USERS_PATH")
                .unwrap_or_else(|_| "data/users.json".to_string()),
            user_templates_path: std::env::var("HYDRA_USER_TEMPLATES_PATH")
                .unwrap_or_else(|_| "data/user-templates.json".to_string()),
            subscription_catalog_path: std::env::var("HYDRA_SUBSCRIPTION_CATALOG_PATH")
                .unwrap_or_else(|_| "data/subscription-catalog.json".to_string()),
            subscription_usage_path: std::env::var("HYDRA_SUBSCRIPTION_USAGE_PATH")
                .unwrap_or_else(|_| "data/subscription-usage.json".to_string()),
            subscription_devices_key_path: std::env::var("HYDRA_SUBSCRIPTION_DEVICES_KEY_PATH")
                .unwrap_or_else(|_| "data/subscription-devices.key".to_string()),
            network_resources_path: std::env::var("HYDRA_NETWORK_RESOURCES_PATH")
                .unwrap_or_else(|_| "data/network-resources.json".to_string()),
            clusters_path: std::env::var("HYDRA_CLUSTERS_PATH")
                .unwrap_or_else(|_| "data/clusters.json".to_string()),
            route_materials_path: std::env::var("HYDRA_ROUTE_MATERIALS_PATH")
                .unwrap_or_else(|_| "data/route-materials.json".to_string()),
            route_materials_key_path: std::env::var("HYDRA_ROUTE_MATERIALS_KEY_PATH")
                .unwrap_or_else(|_| "data/route-materials.key".to_string()),
            reality_materials_path: std::env::var("HYDRA_REALITY_MATERIALS_PATH")
                .unwrap_or_else(|_| "data/reality-materials.json".to_string()),
            reality_materials_key_path: std::env::var("HYDRA_REALITY_MATERIALS_KEY_PATH")
                .unwrap_or_else(|_| "data/reality-materials.key".to_string()),
            node_secrets_key_path: std::env::var("HYDRA_NODE_SECRETS_KEY_PATH")
                .unwrap_or_else(|_| "data/node-secrets.key".to_string()),
            nodes_path: std::env::var("HYDRA_NODES_PATH")
                .unwrap_or_else(|_| "data/nodes.json".to_string()),
            runtime_limits: RuntimeLimits::default(),
            security: SecurityConfig::default(),
            monitoring: MonitoringConfig::default(),
        }
    }
}

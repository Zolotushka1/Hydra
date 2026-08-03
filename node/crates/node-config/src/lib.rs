use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    pub panel_url: String,
    pub node_token: String,
    pub poll_interval_seconds: u64,
    pub node_version: String,
    pub xray_version: Option<String>,
    pub local_state_path: String,
    pub local_config_path: String,
    pub local_runtime_config_path: String,
    pub local_sidecar_runtime_config_path: String,
    pub local_xray_config_path: String,
    pub route_credentials_path: String,
    pub route_credentials_dir: String,
    pub apply_history_path: String,
    pub runtime_event_history_path: String,
    pub local_api_bind: String,
    pub local_api_token: Option<String>,
    pub subscription_session_adapter_token: Option<String>,
    pub max_subscription_session_observations: usize,
    pub max_pending_subscription_session_enforcements: usize,
    pub subscription_session_observation_stale_after_seconds: u64,
    pub subscription_session_adapter_lease_seconds: u64,
    pub subscription_session_action_timeout_seconds: u64,
    pub xray_binary_path: Option<String>,
    pub xray_validate_args: Vec<String>,
    pub xray_run_args: Vec<String>,
    pub xray_stats_api_address: String,
    pub runtime_stats_timeout_seconds: u64,
    pub runtime_activity_window_seconds: u64,
    pub hysteria2_binary_path: Option<String>,
    pub hysteria2_traffic_stats_base_port: u16,
    pub wireguard_binary_path: Option<String>,
    pub wg_quick_binary_path: Option<String>,
    pub sidecar_recipe_mode: String,
    pub hysteria2_service_name: String,
    pub wireguard_interface_name: String,
    pub hysteria2_install_args: Vec<String>,
    pub hysteria2_update_args: Vec<String>,
    pub hysteria2_start_args: Vec<String>,
    pub hysteria2_stop_args: Vec<String>,
    pub hysteria2_restart_args: Vec<String>,
    pub hysteria2_status_args: Vec<String>,
    pub hysteria2_logs_args: Vec<String>,
    pub wireguard_install_args: Vec<String>,
    pub wireguard_update_args: Vec<String>,
    pub wireguard_start_args: Vec<String>,
    pub wireguard_stop_args: Vec<String>,
    pub wireguard_restart_args: Vec<String>,
    pub wireguard_status_args: Vec<String>,
    pub wireguard_logs_args: Vec<String>,
    pub max_log_lines_per_upload: usize,
    pub max_buffered_log_lines: usize,
    pub max_apply_history_entries: usize,
    pub max_runtime_event_entries: usize,
    pub tick_failure_backoff_base_seconds: u64,
    pub tick_failure_backoff_max_seconds: u64,
    pub xray_restart_backoff_base_seconds: u64,
    pub xray_restart_backoff_max_seconds: u64,
    pub xray_apply_mode: String,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            panel_url: std::env::var("HYDRA_PANEL_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:8080".to_string()),
            node_token: std::env::var("HYDRA_NODE_TOKEN")
                .or_else(|_| std::env::var("HYDRA_NODE_AUTH_TOKEN"))
                .unwrap_or_default(),
            poll_interval_seconds: std::env::var("HYDRA_NODE_POLL_INTERVAL_SECONDS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(15),
            node_version: env!("CARGO_PKG_VERSION").to_string(),
            xray_version: std::env::var("HYDRA_NODE_XRAY_VERSION").ok(),
            local_state_path: std::env::var("HYDRA_NODE_STATE_PATH")
                .unwrap_or_else(|_| "data/node-state.json".to_string()),
            local_config_path: std::env::var("HYDRA_NODE_CONFIG_PATH")
                .unwrap_or_else(|_| "data/generated-config.json".to_string()),
            local_runtime_config_path: std::env::var("HYDRA_NODE_RUNTIME_CONFIG_PATH")
                .unwrap_or_else(|_| "data/node-runtime-config.json".to_string()),
            local_sidecar_runtime_config_path: std::env::var(
                "HYDRA_NODE_SIDECAR_RUNTIME_CONFIG_PATH",
            )
            .unwrap_or_else(|_| "data/sidecar-runtime-config.json".to_string()),
            local_xray_config_path: std::env::var("HYDRA_NODE_XRAY_CONFIG_PATH")
                .unwrap_or_else(|_| "data/xray.json".to_string()),
            route_credentials_path: std::env::var("HYDRA_NODE_ROUTE_CREDENTIALS_PATH")
                .unwrap_or_else(|_| "data/route-credentials.json".to_string()),
            route_credentials_dir: std::env::var("HYDRA_NODE_ROUTE_CREDENTIALS_DIR")
                .unwrap_or_else(|_| "data/route-credentials".to_string()),
            apply_history_path: std::env::var("HYDRA_NODE_APPLY_HISTORY_PATH")
                .unwrap_or_else(|_| "data/apply-history.json".to_string()),
            runtime_event_history_path: std::env::var("HYDRA_NODE_RUNTIME_EVENTS_PATH")
                .unwrap_or_else(|_| "data/runtime-events.json".to_string()),
            local_api_bind: std::env::var("HYDRA_NODE_LOCAL_API_BIND")
                .unwrap_or_else(|_| "127.0.0.1:8081".to_string()),
            local_api_token: std::env::var("HYDRA_NODE_LOCAL_API_TOKEN")
                .ok()
                .filter(|value| !value.trim().is_empty()),
            subscription_session_adapter_token: std::env::var("HYDRA_NODE_SESSION_ADAPTER_TOKEN")
                .ok()
                .filter(|value| !value.trim().is_empty()),
            max_subscription_session_observations: std::env::var(
                "HYDRA_NODE_MAX_SESSION_OBSERVATIONS",
            )
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(2_048),
            max_pending_subscription_session_enforcements: std::env::var(
                "HYDRA_NODE_MAX_PENDING_SESSION_ENFORCEMENTS",
            )
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(256),
            subscription_session_observation_stale_after_seconds: std::env::var(
                "HYDRA_NODE_SESSION_OBSERVATION_STALE_AFTER_SECONDS",
            )
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(120),
            subscription_session_adapter_lease_seconds: std::env::var(
                "HYDRA_NODE_SESSION_ADAPTER_LEASE_SECONDS",
            )
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(90),
            subscription_session_action_timeout_seconds: std::env::var(
                "HYDRA_NODE_SESSION_ACTION_TIMEOUT_SECONDS",
            )
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(30),
            xray_binary_path: std::env::var("HYDRA_NODE_XRAY_BINARY_PATH").ok(),
            xray_validate_args: parse_string_list_env("HYDRA_NODE_XRAY_VALIDATE_ARGS_JSON"),
            xray_run_args: parse_string_list_env("HYDRA_NODE_XRAY_RUN_ARGS_JSON"),
            xray_stats_api_address: std::env::var("HYDRA_NODE_XRAY_STATS_API_ADDRESS")
                .unwrap_or_else(|_| "127.0.0.1:10085".to_string()),
            runtime_stats_timeout_seconds: std::env::var(
                "HYDRA_NODE_RUNTIME_STATS_TIMEOUT_SECONDS",
            )
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(5),
            runtime_activity_window_seconds: std::env::var(
                "HYDRA_NODE_RUNTIME_ACTIVITY_WINDOW_SECONDS",
            )
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(120),
            hysteria2_binary_path: std::env::var("HYDRA_NODE_HYSTERIA2_BINARY_PATH")
                .ok()
                .filter(|value| !value.trim().is_empty()),
            hysteria2_traffic_stats_base_port: std::env::var(
                "HYDRA_NODE_HYSTERIA2_TRAFFIC_STATS_BASE_PORT",
            )
            .ok()
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(19_090),
            wireguard_binary_path: std::env::var("HYDRA_NODE_WIREGUARD_BINARY_PATH")
                .ok()
                .filter(|value| !value.trim().is_empty()),
            wg_quick_binary_path: std::env::var("HYDRA_NODE_WG_QUICK_BINARY_PATH")
                .ok()
                .filter(|value| !value.trim().is_empty()),
            sidecar_recipe_mode: std::env::var("HYDRA_NODE_SIDECAR_RECIPE_MODE")
                .unwrap_or_else(|_| "explicit_argv".to_string()),
            hysteria2_service_name: std::env::var("HYDRA_NODE_HYSTERIA2_SERVICE_NAME")
                .unwrap_or_else(|_| "hysteria-server.service".to_string()),
            wireguard_interface_name: std::env::var("HYDRA_NODE_WIREGUARD_INTERFACE_NAME")
                .unwrap_or_else(|_| "hydra-wg0".to_string()),
            hysteria2_install_args: parse_string_list_env("HYDRA_NODE_HYSTERIA2_INSTALL_ARGS_JSON"),
            hysteria2_update_args: parse_string_list_env("HYDRA_NODE_HYSTERIA2_UPDATE_ARGS_JSON"),
            hysteria2_start_args: parse_string_list_env("HYDRA_NODE_HYSTERIA2_START_ARGS_JSON"),
            hysteria2_stop_args: parse_string_list_env("HYDRA_NODE_HYSTERIA2_STOP_ARGS_JSON"),
            hysteria2_restart_args: parse_string_list_env("HYDRA_NODE_HYSTERIA2_RESTART_ARGS_JSON"),
            hysteria2_status_args: parse_string_list_env("HYDRA_NODE_HYSTERIA2_STATUS_ARGS_JSON"),
            hysteria2_logs_args: parse_string_list_env("HYDRA_NODE_HYSTERIA2_LOGS_ARGS_JSON"),
            wireguard_install_args: parse_string_list_env("HYDRA_NODE_WIREGUARD_INSTALL_ARGS_JSON"),
            wireguard_update_args: parse_string_list_env("HYDRA_NODE_WIREGUARD_UPDATE_ARGS_JSON"),
            wireguard_start_args: parse_string_list_env("HYDRA_NODE_WIREGUARD_START_ARGS_JSON"),
            wireguard_stop_args: parse_string_list_env("HYDRA_NODE_WIREGUARD_STOP_ARGS_JSON"),
            wireguard_restart_args: parse_string_list_env("HYDRA_NODE_WIREGUARD_RESTART_ARGS_JSON"),
            wireguard_status_args: parse_string_list_env("HYDRA_NODE_WIREGUARD_STATUS_ARGS_JSON"),
            wireguard_logs_args: parse_string_list_env("HYDRA_NODE_WIREGUARD_LOGS_ARGS_JSON"),
            max_log_lines_per_upload: std::env::var("HYDRA_NODE_MAX_LOG_LINES")
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(100),
            max_buffered_log_lines: std::env::var("HYDRA_NODE_MAX_BUFFERED_LOG_LINES")
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(400),
            max_apply_history_entries: std::env::var("HYDRA_NODE_MAX_APPLY_HISTORY_ENTRIES")
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(128),
            max_runtime_event_entries: std::env::var("HYDRA_NODE_MAX_RUNTIME_EVENT_ENTRIES")
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(256),
            tick_failure_backoff_base_seconds: std::env::var(
                "HYDRA_NODE_TICK_BACKOFF_BASE_SECONDS",
            )
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(5),
            tick_failure_backoff_max_seconds: std::env::var("HYDRA_NODE_TICK_BACKOFF_MAX_SECONDS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(300),
            xray_restart_backoff_base_seconds: std::env::var(
                "HYDRA_NODE_XRAY_RESTART_BACKOFF_BASE_SECONDS",
            )
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(5),
            xray_restart_backoff_max_seconds: std::env::var(
                "HYDRA_NODE_XRAY_RESTART_BACKOFF_MAX_SECONDS",
            )
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(300),
            xray_apply_mode: std::env::var("HYDRA_NODE_XRAY_APPLY_MODE")
                .unwrap_or_else(|_| "validate_json".to_string()),
        }
    }
}

fn parse_string_list_env(key: &str) -> Vec<String> {
    std::env::var(key)
        .ok()
        .and_then(|value| serde_json::from_str::<Vec<String>>(&value).ok())
        .unwrap_or_default()
}

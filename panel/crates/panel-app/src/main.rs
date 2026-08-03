use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;

use anyhow::Context;
use axum::{
    Json, Router,
    extract::{ConnectInfo, DefaultBodyLimit, Query, State},
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{AUTHORIZATION, REFERRER_POLICY, X_CONTENT_TYPE_OPTIONS, X_FRAME_OPTIONS},
    },
    middleware::{self, Next},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{MethodRouter, delete, get, post, put},
};
use panel_config::AppConfig;
use panel_core::AppState;
use panel_core::routes::{ROUTE_TABLE, RouteId, RouteMethod, RouteSpec};
use panel_domain::cluster::{CreateClusterRequest, UpdateClusterRequest};
use panel_domain::installer::{
    PanelInstallPlanRequest, PanelInstallerBootstrapRequest, PanelInstallerCreateJobRequest,
    PanelInstallerJobAccessRequest, PanelInstallerJobHeartbeatRequest,
    PanelInstallerJobResultRequest, PanelInstallerSessionRequest,
    PanelInstallerSessionResultRequest,
};
use panel_domain::network::{
    CreateHostRequest, CreateInboundRequest, CreateProxyProfileRequest, UpdateHostRequest,
    UpdateInboundRequest, UpdateProxyProfileRequest,
};
use panel_domain::node::{
    CreateNodeRequest, NodeApplyRequest, NodeApplyResultRequest, NodeApplyRetryRequest,
    NodeBootstrapReadinessView, NodeHeartbeatRequest, NodeLocalActionResponse,
    NodeLogUploadRequest, NodeMetricsRequest, NodeRollbackRequest, NodeSyncRequest,
    RouteCredentialActionRequest, UpdateNodeRequest,
};
use panel_domain::provisioning::{
    FinishNodeProvisioningRequest, NodeProvisioningEventsQuery,
    NodeProvisioningExecutorHandshakeRequest, NodeProvisioningExecutorRecoveryHint,
    NodeProvisioningExecutorRejectionCode, NodeProvisioningExecutorRejectionView,
    NodeProvisioningExecutorResultQuery, NodeProvisioningExecutorSessionQuery,
    NodeProvisioningExecutorSubmissionsQuery, ReportNodeProvisioningCommandRequest,
    ReportNodeProvisioningHandoffRequest, ReprovisionNodeRequest, RetryNodeProvisioningRequest,
    StartNodeProvisioningRequest, TouchNodeProvisioningTaskRequest,
    UpdateNodeProvisioningExecutorTrustRequest, UpdateNodeProvisioningStepRequest,
};
use panel_domain::security::{
    ApplySecurityPresetRequest, AuditEventsQuery, AuthenticatedAdmin, CreateBanRequest,
    DisableTwoFactorRequest, EnableTwoFactorRequest, LoginFailure, LoginRequest, SecurityError,
    UpdateSecuritySettingsRequest, UpdateTwoFactorTwoStepRequest,
};
use panel_domain::subscription::{
    CreateSubscriptionClientRequest, CreateSubscriptionDeviceEnrollmentRequest,
    CreateSubscriptionPlanRequest, ExchangeSubscriptionDeviceEnrollmentRequest,
    RegisterSubscriptionDeviceRequest, ReportSubscriptionSessionEnforcementResultRequest,
    ReportSubscriptionSessionsRequest, ReportSubscriptionUsageRequest, SubscriptionCatalogQuery,
    SubscriptionDeviceEnrollmentsQuery, SubscriptionDevicesQuery, SubscriptionFormat,
    SubscriptionQuery, SubscriptionSessionsQuery, SubscriptionUsageQuery,
    UpdateSubscriptionClientAccessRequest, UpdateSubscriptionClientRequest,
    UpdateSubscriptionPlanRequest,
};
use panel_domain::system::{
    AlertHistoryQuery, CoreActionRequest, CoreApplyHistoryQuery, CoreApplyRequest,
    OperationalLogsQuery, SaveCoreConfigRequest, UpdateSystemThresholdsRequest,
    XrayCoreUpdateRequest,
};
use panel_domain::telegram::{
    TelegramEventsQuery, TelegramTestRequest, UpdateTelegramSettingsRequest,
};
use panel_domain::user::{
    CreateUserRequest, CreateUserTemplateRequest, ReportUserUsageRequest, UpdateUserRequest,
    UpdateUserTemplateRequest, UserActivityQuery, UsersQuery,
};
use std::path::Path;
use std::sync::OnceLock;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing::{info, info_span, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Where the built frontend lives at run time.
///
/// Resolved from the running executable, never from `CARGO_MANIFEST_DIR`. That
/// variable holds the path of the machine the binary was *built* on, so a release
/// binary looked for the bundle in a directory that exists only on the build
/// host — which is why `/assets` returned 404 in every installed deployment while
/// working in development.
///
/// Order: the explicit override, then `web/` beside the executable, which is the
/// layout `scripts/package-release.*` produce.
///
/// The source-tree fallback exists only in debug builds. A release binary that
/// silently read the build host's checkout would make every packaging check
/// meaningless — it would find the bundle on the build machine whether or not the
/// package contained one, and fail only for the operator. This was not
/// hypothetical: the first version of the packaging check passed against a
/// package with no frontend in it at all.
fn web_dist_dir() -> &'static Path {
    static RESOLVED: OnceLock<PathBuf> = OnceLock::new();
    RESOLVED.get_or_init(|| {
        if let Ok(configured) = std::env::var("HYDRA_WEB_DIST_DIR") {
            let path = PathBuf::from(configured);
            if !path.join("index.html").is_file() {
                warn!(path = %path.display(), "HYDRA_WEB_DIST_DIR has no index.html");
            }
            return path;
        }

        let beside_executable = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|dir| dir.join("web")));
        if let Some(path) = beside_executable.as_ref()
            && path.join("index.html").is_file()
        {
            return path.clone();
        }

        #[cfg(debug_assertions)]
        let fallback = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../web/dist");
        #[cfg(not(debug_assertions))]
        let fallback = beside_executable.unwrap_or_else(|| PathBuf::from("web"));

        fallback
    })
}

/// The built frontend entry point, read at run time rather than embedded.
///
/// `web/dist/` is a build output and is not in the repository, so an
/// `include_str!` of it made the panel impossible to compile from a clean
/// checkout. Reading at run time also matches how `/assets` is served, so the
/// bundle has to be present beside the binary either way.
///
/// A missing file is reported in the response instead of being fatal: the API is
/// fully usable without the frontend, and an operator who has not built or
/// packaged it should be told which directory was searched.
fn dashboard_html() -> &'static str {
    static CACHED: OnceLock<String> = OnceLock::new();
    CACHED.get_or_init(|| {
        let path = web_dist_dir().join("index.html");
        std::fs::read_to_string(&path).unwrap_or_else(|error| {
            warn!(%error, path = %path.display(), "frontend bundle is missing");
            format!(
                "<!doctype html><meta charset=\"utf-8\"><title>Hydra</title>\
                 <p>The frontend bundle was not found in {}. Build it with \
                 <code>npm ci &amp;&amp; npm run build</code> in <code>panel/web</code>, or set \
                 <code>HYDRA_WEB_DIST_DIR</code>. The HTTP API is unaffected.",
                path.display()
            )
        })
    })
}

const MAX_REQUEST_BODY_BYTES: usize = 4 * 1024 * 1024;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let config = AppConfig::default();
    let bind_addr = config.bind_addr.clone();
    let tls_certificate_path = config.tls_certificate_path.clone();
    let tls_private_key_path = config.tls_private_key_path.clone();
    // Refuse to start rather than come up empty. Two causes: a corrupt or
    // incompatible persisted file, and permissions wider than allowed on a secret
    // file or data directory. Both must be surfaced here, naming the path.
    let state =
        AppState::new(config).map_err(|error| anyhow::anyhow!("panel cannot start: {error}"))?;
    let standalone_activity_state = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(
            standalone_activity_state
                .standalone_xray_stats_poll_interval()
                .await,
        );
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            if let Err(error) = standalone_activity_state
                .collect_standalone_xray_activity()
                .await
            {
                warn!(%error, "standalone Xray activity collection failed");
            }
        }
    });
    let dist_assets_dir = web_dist_dir().join("assets");

    // The router is built solely by walking ROUTE_TABLE. There is no second route
    // list in the project: both the served surface and the GET /api/ui/contracts
    // document are derived from that one constant.
    let mut app = Router::new().nest_service("/assets", ServeDir::new(dist_assets_dir));
    for spec in ROUTE_TABLE {
        app = app.route(spec.path, method_router(spec));
    }
    let app = app
        .layer(middleware::from_fn(set_security_headers))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY_BYTES))
        .with_state(state)
        .layer(
            TraceLayer::new_for_http().make_span_with(|request: &http::Request<_>| {
                let path = request.uri().path();
                let route = request_trace_route(path);
                info_span!(
                    "http_request",
                    method = %request.method(),
                    route = route
                )
            }),
        );

    match (tls_certificate_path, tls_private_key_path) {
        (Some(certificate_path), Some(private_key_path)) => {
            rustls::crypto::ring::default_provider()
                .install_default()
                .map_err(|_| {
                    anyhow::anyhow!("failed to install the Rustls ring crypto provider")
                })?;
            let address = bind_addr.parse::<SocketAddr>().with_context(|| {
                format!("TLS bind address must be a socket address: {bind_addr}")
            })?;
            let tls_config = axum_server::tls_rustls::RustlsConfig::from_pem_file(
                certificate_path,
                private_key_path,
            )
            .await
            .context("failed to load panel TLS certificate/private key")?;
            info!(%address, transport = "https", "hydra-panel listening");
            axum_server::bind_rustls(address, tls_config)
                .serve(app.into_make_service_with_connect_info::<SocketAddr>())
                .await
                .context("TLS panel server failed")?;
        }
        (None, None) => {
            let listener = tokio::net::TcpListener::bind(&bind_addr)
                .await
                .with_context(|| format!("failed to bind {bind_addr}"))?;
            let local_addr: SocketAddr = listener.local_addr().context("missing local addr")?;
            info!(%local_addr, transport = "http", "hydra-panel listening");
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .context("panel server failed")?;
        }
        _ => {
            anyhow::bail!("HYDRA_TLS_CERT_PATH and HYDRA_TLS_KEY_PATH must be configured together")
        }
    }

    Ok(())
}

/// Binds a handler to the method from the table.
///
/// The method comes only from `spec.method` and never appears in the arm, so
/// "declared GET but registered POST" is inexpressible.
macro_rules! bind {
    ($spec:expr, $handler:expr) => {
        match $spec.method {
            RouteMethod::Get => get($handler),
            RouteMethod::Post => post($handler),
            RouteMethod::Put => put($handler),
            RouteMethod::Delete => delete($handler),
        }
    };
}

/// The only place a route is bound to a handler.
///
/// The `match` is exhaustive: a new `RouteId` without an arm is a compile error
/// rather than a route that silently goes unserved.
fn method_router(spec: &RouteSpec) -> MethodRouter<AppState> {
    match spec.id {
        RouteId::Root => bind!(spec, root),
        RouteId::DashboardGet => bind!(spec, dashboard),
        RouteId::DashboardSlash => bind!(spec, dashboard),
        RouteId::Health => bind!(spec, health),
        RouteId::PublicSubscription => bind!(spec, public_subscription),
        RouteId::PublicDeviceSubscription => bind!(spec, public_device_subscription),
        RouteId::ExchangeSubscriptionDeviceEnrollment => {
            bind!(spec, exchange_subscription_device_enrollment)
                .layer(DefaultBodyLimit::max(16 * 1024))
        }
        RouteId::Login => bind!(spec, login),
        RouteId::Logout => bind!(spec, logout),
        RouteId::Me => bind!(spec, me),
        RouteId::GetAdminSessions => bind!(spec, get_admin_sessions),
        RouteId::RevokeAdminSession => bind!(spec, revoke_admin_session),
        RouteId::GetUiBootstrap => bind!(spec, get_ui_bootstrap),
        RouteId::GetUiOverview => bind!(spec, get_ui_overview),
        RouteId::GetUiContracts => bind!(spec, get_ui_contracts),
        RouteId::GetUiSecurity => bind!(spec, get_ui_security),
        RouteId::GetUiCore => bind!(spec, get_ui_core),
        RouteId::GetUiUsersSummary => bind!(spec, get_ui_users_summary),
        RouteId::GetUiNodesSummary => bind!(spec, get_ui_nodes_summary),
        RouteId::GetUiClustersSummary => bind!(spec, get_ui_clusters_summary),
        RouteId::GetUiTelegramSummary => bind!(spec, get_ui_telegram_summary),
        RouteId::GetUiAuditSummary => bind!(spec, get_ui_audit_summary),
        RouteId::GetUiSubscriptionsSummary => bind!(spec, get_ui_subscriptions_summary),
        RouteId::GetUiProtocolsSummary => bind!(spec, get_ui_protocols_summary),
        RouteId::GetUiInstallerSummary => bind!(spec, get_ui_installer_summary),
        RouteId::GetSecuritySettings => bind!(spec, get_security_settings),
        RouteId::UpdateSecuritySettings => bind!(spec, update_security_settings),
        RouteId::ApplySecurityPreset => bind!(spec, apply_security_preset),
        RouteId::GetTwoFactorState => bind!(spec, get_two_factor_state),
        RouteId::SetupTwoFactor => bind!(spec, setup_two_factor),
        RouteId::EnableTwoFactor => bind!(spec, enable_two_factor),
        RouteId::DisableTwoFactor => bind!(spec, disable_two_factor),
        RouteId::UpdateTwoFactorTwoStep => bind!(spec, update_two_factor_two_step),
        RouteId::GetActiveBans => bind!(spec, get_active_bans),
        RouteId::CreateBan => bind!(spec, create_ban),
        RouteId::RemoveBan => bind!(spec, remove_ban),
        RouteId::GetSecurityAudit => bind!(spec, get_security_audit),
        RouteId::GetSecurityStatus => bind!(spec, get_security_status),
        RouteId::GetTelegramSettings => bind!(spec, get_telegram_settings),
        RouteId::UpdateTelegramSettings => bind!(spec, update_telegram_settings),
        RouteId::GetTelegramEvents => bind!(spec, get_telegram_events),
        RouteId::RetryDueTelegramEvents => bind!(spec, retry_due_telegram_events),
        RouteId::SendTelegramTest => bind!(spec, send_telegram_test),
        RouteId::GetPanelAccessModes => bind!(spec, get_panel_access_modes),
        RouteId::PlanPanelInstall => bind!(spec, plan_panel_install),
        RouteId::CreatePanelInstallerBootstrap => bind!(spec, create_panel_installer_bootstrap),
        RouteId::CreatePanelInstallerSession => bind!(spec, create_panel_installer_session),
        RouteId::ReportPanelInstallerResult => bind!(spec, report_panel_installer_result),
        RouteId::ListPanelInstallerJobs => bind!(spec, list_panel_installer_jobs),
        RouteId::CreatePanelInstallerJob => bind!(spec, create_panel_installer_job),
        RouteId::ReportPanelInstallerJobHeartbeat => {
            bind!(spec, report_panel_installer_job_heartbeat)
        }
        RouteId::GetPanelInstallerJobForExecutor => {
            bind!(spec, get_panel_installer_job_for_executor)
        }
        RouteId::ReportPanelInstallerJobResult => bind!(spec, report_panel_installer_job_result),
        RouteId::GetSystemOverview => bind!(spec, get_system_overview),
        RouteId::GetResourceBudget => bind!(spec, get_resource_budget),
        RouteId::GetSystemThresholds => bind!(spec, get_system_thresholds),
        RouteId::UpdateSystemThresholds => bind!(spec, update_system_thresholds),
        RouteId::GetSecretKeyReadiness => bind!(spec, get_secret_key_readiness),
        RouteId::GetSystemAlerts => bind!(spec, get_system_alerts),
        RouteId::GetSystemAlertHistory => bind!(spec, get_system_alert_history),
        RouteId::GetOperationalLogs => bind!(spec, get_operational_logs),
        RouteId::GetCoreConfig => bind!(spec, get_core_config),
        RouteId::SaveCoreConfig => bind!(spec, save_core_config),
        RouteId::GetGeneratedCoreConfig => bind!(spec, get_generated_core_config),
        RouteId::GetGeneratedXrayConfig => bind!(spec, get_generated_xray_config),
        RouteId::GetGeneratedXrayConfigValidation => {
            bind!(spec, get_generated_xray_config_validation)
        }
        RouteId::GetGeneratedXrayConfigExternalValidation => {
            bind!(spec, get_generated_xray_config_external_validation)
        }
        RouteId::GetCoreState => bind!(spec, get_core_state),
        RouteId::GetRouteMaterials => bind!(spec, get_route_materials),
        RouteId::RotateRouteCredential => bind!(spec, rotate_route_credential),
        RouteId::RevokeRouteCredential => bind!(spec, revoke_route_credential),
        RouteId::RotateRouteCa => bind!(spec, rotate_route_ca),
        RouteId::ApplyGeneratedCoreConfig => bind!(spec, apply_generated_core_config),
        RouteId::GetCoreApplyHistory => bind!(spec, get_core_apply_history),
        RouteId::ExecuteCoreAction => bind!(spec, execute_core_action),
        RouteId::RestartCore => bind!(spec, restart_core),
        RouteId::UpdateXrayCore => bind!(spec, update_xray_core),
        RouteId::ListUsers => bind!(spec, list_users),
        RouteId::CreateUser => bind!(spec, create_user),
        RouteId::GetUsersActivity => bind!(spec, get_users_activity),
        RouteId::GetUser => bind!(spec, get_user),
        RouteId::UpdateUser => bind!(spec, update_user),
        RouteId::DeleteUser => bind!(spec, delete_user),
        RouteId::ResetUserUsage => bind!(spec, reset_user_usage),
        RouteId::ReportUserUsage => bind!(spec, report_user_usage),
        RouteId::GetUserActivity => bind!(spec, get_user_activity),
        RouteId::RevokeUserSubscription => bind!(spec, revoke_user_subscription),
        RouteId::GetUserSubscription => bind!(spec, get_user_subscription),
        RouteId::RenderUserSubscription => bind!(spec, render_user_subscription),
        RouteId::GetUserConfigPreview => bind!(spec, get_user_config_preview),
        RouteId::GetGeneratedUserConfig => bind!(spec, get_generated_user_config),
        RouteId::ListUserTemplates => bind!(spec, list_user_templates),
        RouteId::CreateUserTemplate => bind!(spec, create_user_template),
        RouteId::UpdateUserTemplate => bind!(spec, update_user_template),
        RouteId::DeleteUserTemplate => bind!(spec, delete_user_template),
        RouteId::ListSubscriptionPlans => bind!(spec, list_subscription_plans),
        RouteId::CreateSubscriptionPlan => bind!(spec, create_subscription_plan),
        RouteId::GetSubscriptionPlan => bind!(spec, get_subscription_plan),
        RouteId::UpdateSubscriptionPlan => bind!(spec, update_subscription_plan),
        RouteId::DeleteSubscriptionPlan => bind!(spec, delete_subscription_plan),
        RouteId::ListSubscriptionPlanClients => bind!(spec, list_subscription_plan_clients),
        RouteId::CreateSubscriptionClient => bind!(spec, create_subscription_client),
        RouteId::GetSubscriptionClient => bind!(spec, get_subscription_client),
        RouteId::UpdateSubscriptionClient => bind!(spec, update_subscription_client),
        RouteId::DeleteSubscriptionClient => bind!(spec, delete_subscription_client),
        RouteId::RenderSubscriptionClient => bind!(spec, render_subscription_client),
        RouteId::GetSubscriptionClientAccessPreview => {
            bind!(spec, get_subscription_client_access_preview)
        }
        RouteId::UpdateSubscriptionClientAccess => bind!(spec, update_subscription_client_access),
        RouteId::ListSubscriptionClientDevices => bind!(spec, list_subscription_client_devices),
        RouteId::ListSubscriptionDeviceEnrollments => {
            bind!(spec, list_subscription_device_enrollments)
        }
        RouteId::CreateSubscriptionDeviceEnrollment => {
            bind!(spec, create_subscription_device_enrollment)
        }
        RouteId::RevokeSubscriptionDeviceEnrollment => {
            bind!(spec, revoke_subscription_device_enrollment)
        }
        RouteId::RegisterSubscriptionClientDevice => {
            bind!(spec, register_subscription_client_device)
        }
        RouteId::RevokeSubscriptionClientDevice => bind!(spec, revoke_subscription_client_device),
        RouteId::ListSubscriptionClientSessions => bind!(spec, list_subscription_client_sessions),
        RouteId::GetSubscriptionClientUsage => bind!(spec, get_subscription_client_usage),
        RouteId::ReportSubscriptionClientUsage => bind!(spec, report_subscription_client_usage),
        RouteId::ResetSubscriptionClientUsage => bind!(spec, reset_subscription_client_usage),
        RouteId::RevokeSubscriptionClient => bind!(spec, revoke_subscription_client),
        RouteId::ListInbounds => bind!(spec, list_inbounds),
        RouteId::CreateInbound => bind!(spec, create_inbound),
        RouteId::GetProtocolCapabilities => bind!(spec, get_protocol_capabilities),
        RouteId::UpdateInbound => bind!(spec, update_inbound),
        RouteId::DeleteInbound => bind!(spec, delete_inbound),
        RouteId::ListHosts => bind!(spec, list_hosts),
        RouteId::CreateHost => bind!(spec, create_host),
        RouteId::UpdateHost => bind!(spec, update_host),
        RouteId::DeleteHost => bind!(spec, delete_host),
        RouteId::ListProxyProfiles => bind!(spec, list_proxy_profiles),
        RouteId::CreateProxyProfile => bind!(spec, create_proxy_profile),
        RouteId::UpdateProxyProfile => bind!(spec, update_proxy_profile),
        RouteId::DeleteProxyProfile => bind!(spec, delete_proxy_profile),
        RouteId::ListClusters => bind!(spec, list_clusters),
        RouteId::CreateCluster => bind!(spec, create_cluster),
        RouteId::UpdateCluster => bind!(spec, update_cluster),
        RouteId::DeleteCluster => bind!(spec, delete_cluster),
        RouteId::ValidateCluster => bind!(spec, validate_cluster),
        RouteId::PreviewCluster => bind!(spec, preview_cluster),
        RouteId::ListNodes => bind!(spec, list_nodes),
        RouteId::CreateNode => bind!(spec, create_node),
        RouteId::GetNodeHealthCenter => bind!(spec, get_node_health_center),
        RouteId::UpdateNode => bind!(spec, update_node),
        RouteId::DeleteNode => bind!(spec, delete_node),
        RouteId::GetNodeDiagnostics => bind!(spec, get_node_diagnostics),
        RouteId::GetNodeApplyStatus => bind!(spec, get_node_apply_status),
        RouteId::ListNodeProvisioningTasks => bind!(spec, list_node_provisioning_tasks),
        RouteId::GetNodeProvisioningStatus => bind!(spec, get_node_provisioning_status),
        RouteId::GetNodeProvisioningEvents => bind!(spec, get_node_provisioning_events),
        RouteId::GetNodeProvisioningPreflight => bind!(spec, get_node_provisioning_preflight),
        RouteId::GetNodeProvisioningSshPreflightProbe => {
            bind!(spec, get_node_provisioning_ssh_preflight_probe)
        }
        RouteId::GetNodeProvisioningSshInstallPlan => {
            bind!(spec, get_node_provisioning_ssh_install_plan)
        }
        RouteId::NodeProvisioningExecutorHandshake => {
            bind!(spec, node_provisioning_executor_handshake)
        }
        RouteId::ListNodeProvisioningExecutors => bind!(spec, list_node_provisioning_executors),
        RouteId::ListNodeProvisioningExecutorSubmissions => {
            bind!(spec, list_node_provisioning_executor_submissions)
        }
        RouteId::UpdateNodeProvisioningExecutorTrust => {
            bind!(spec, update_node_provisioning_executor_trust)
        }
        RouteId::RotateNodeProvisioningExecutorToken => {
            bind!(spec, rotate_node_provisioning_executor_token)
        }
        RouteId::GetNodeProvisioningExecutorSession => {
            bind!(spec, get_node_provisioning_executor_session)
        }
        RouteId::StartNodeProvisioning => bind!(spec, start_node_provisioning),
        RouteId::ReprovisionNode => bind!(spec, reprovision_node),
        RouteId::UpdateNodeProvisioningStep => bind!(spec, update_node_provisioning_step),
        RouteId::ReportNodeProvisioningCommand => bind!(spec, report_node_provisioning_command),
        RouteId::ReportNodeProvisioningHandoff => bind!(spec, report_node_provisioning_handoff),
        RouteId::TouchNodeProvisioningTask => bind!(spec, touch_node_provisioning_task),
        RouteId::FinishNodeProvisioning => bind!(spec, finish_node_provisioning),
        RouteId::RetryNodeProvisioning => bind!(spec, retry_node_provisioning),
        RouteId::GetNodeBootstrapReadiness => bind!(spec, get_node_bootstrap_readiness),
        RouteId::RunNodeBootstrapProbe => bind!(spec, run_node_bootstrap_probe),
        RouteId::GetNodeBootstrapHistory => bind!(spec, get_node_bootstrap_history),
        RouteId::RotateNodeAuthToken => bind!(spec, rotate_node_auth_token),
        RouteId::RequestNodeApply => bind!(spec, request_node_apply),
        RouteId::RetryNodeApply => bind!(spec, retry_node_apply),
        RouteId::RequestNodeRollback => bind!(spec, request_node_rollback),
        RouteId::GetNodeClusterTargets => bind!(spec, get_node_cluster_targets),
        RouteId::GetNodeLocalHealth => bind!(spec, get_node_local_health),
        RouteId::GetNodeLocalState => bind!(spec, get_node_local_state),
        RouteId::ExecuteNodeLocalRuntimeAction => bind!(spec, execute_node_local_runtime_action),
        RouteId::ExecuteNodeLocalRuntimeComponentAction => {
            bind!(spec, execute_node_local_runtime_component_action)
        }
        RouteId::UpdateNodeLocalXray => bind!(spec, update_node_local_xray),
        RouteId::RecordNodeHeartbeat => bind!(spec, record_node_heartbeat),
        RouteId::UpdateNodeSyncStatus => bind!(spec, update_node_sync_status),
        RouteId::GetNodeSyncHistory => bind!(spec, get_node_sync_history),
        RouteId::GetNodeApplyResults => bind!(spec, get_node_apply_results),
        RouteId::NodeAgentMe => bind!(spec, node_agent_me),
        RouteId::NodeAgentConfig => bind!(spec, node_agent_config),
        RouteId::NodeAgentXrayConfig => bind!(spec, node_agent_xray_config),
        RouteId::NodeAgentRouteCredentials => bind!(spec, node_agent_route_credentials),
        RouteId::NodeAgentClusterTargets => bind!(spec, node_agent_cluster_targets),
        RouteId::NodeAgentHeartbeat => bind!(spec, node_agent_heartbeat),
        RouteId::NodeAgentSync => bind!(spec, node_agent_sync),
        RouteId::NodeAgentApplyResult => bind!(spec, node_agent_apply_result),
        RouteId::NodeAgentLogs => bind!(spec, node_agent_logs),
        RouteId::NodeAgentMetrics => bind!(spec, node_agent_metrics),
        RouteId::NodeAgentSubscriptionClientUsageReport => {
            bind!(spec, node_agent_subscription_client_usage_report)
        }
        RouteId::NodeAgentSubscriptionSessionsReport => {
            bind!(spec, node_agent_subscription_sessions_report)
        }
        RouteId::NodeAgentSubscriptionSessionEnforcementResult => {
            bind!(spec, node_agent_subscription_session_enforcement_result)
        }
    }
}

fn request_trace_route(path: &str) -> &str {
    if path.starts_with("/sub/") {
        "/sub/{redacted}"
    } else {
        path
    }
}

async fn root() -> Redirect {
    Redirect::temporary("/dashboard")
}

async fn dashboard() -> Html<&'static str> {
    Html(dashboard_html())
}

async fn public_subscription(
    State(state): State<AppState>,
    axum::extract::Path(subscription_token): axum::extract::Path<String>,
    Query(query): Query<SubscriptionQuery>,
) -> Response {
    let format = query.format.unwrap_or(SubscriptionFormat::Base64);
    match state
        .render_subscription_by_token(&subscription_token, format, query.device_id.as_deref())
        .await
    {
        Ok(rendered) => (
            StatusCode::OK,
            [
                (http::header::CONTENT_TYPE, rendered.content_type),
                (http::header::CACHE_CONTROL, "private, no-store".to_string()),
                (http::header::PRAGMA, "no-cache".to_string()),
                (
                    http::header::HeaderName::from_static("x-content-type-options"),
                    "nosniff".to_string(),
                ),
            ],
            rendered.body,
        )
            .into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn public_device_subscription(
    State(state): State<AppState>,
    axum::extract::Path(device_credential): axum::extract::Path<String>,
    Query(query): Query<SubscriptionQuery>,
) -> Response {
    let format = query.format.unwrap_or(SubscriptionFormat::Base64);
    match state
        .render_subscription_by_device_credential(&device_credential, format)
        .await
    {
        Ok(rendered) => (
            StatusCode::OK,
            [
                (http::header::CONTENT_TYPE, rendered.content_type),
                (http::header::CACHE_CONTROL, "private, no-store".to_string()),
                (http::header::PRAGMA, "no-cache".to_string()),
                (
                    http::header::HeaderName::from_static("x-content-type-options"),
                    "nosniff".to_string(),
                ),
            ],
            rendered.body,
        )
            .into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn exchange_subscription_device_enrollment(
    State(state): State<AppState>,
    Json(payload): Json<ExchangeSubscriptionDeviceEnrollmentRequest>,
) -> Response {
    match state.exchange_subscription_device_enrollment(payload).await {
        Ok(result) => (
            StatusCode::OK,
            [
                (http::header::CACHE_CONTROL, "private, no-store".to_string()),
                (http::header::PRAGMA, "no-cache".to_string()),
                (
                    http::header::HeaderName::from_static("x-content-type-options"),
                    "nosniff".to_string(),
                ),
            ],
            Json(result),
        )
            .into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn health(State(state): State<AppState>) -> Json<serde_json::Value> {
    let status = state.health();
    let config = state.config_snapshot().await;

    Json(serde_json::json!({
        "service": status.service,
        "status": status.status,
        "memory_budget_mb": config.runtime_limits.memory_budget_mb,
    }))
}

async fn login(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(payload): Json<LoginRequest>,
) -> Response {
    let config = state.config_snapshot().await;
    let client_ip = resolve_client_ip(&config, addr.ip().to_string(), &headers);

    match state
        .login(
            &client_ip,
            &payload.username,
            &payload.password,
            payload.two_factor_code.as_deref(),
            payload.challenge_token.as_deref(),
        )
        .await
    {
        Ok(result) => (StatusCode::OK, Json(serde_json::json!(result))).into_response(),
        Err(LoginFailure {
            reason,
            blocked_until_unix,
            ban_kind,
            challenge_token,
            wait_seconds,
        }) => {
            let status = if reason == "ip_banned" {
                StatusCode::TOO_MANY_REQUESTS
            } else {
                StatusCode::UNAUTHORIZED
            };

            (
                status,
                Json(serde_json::json!({
                    "reason": reason,
                    "blocked_until_unix": blocked_until_unix,
                    "ban_kind": ban_kind,
                    "challenge_token": challenge_token,
                    "wait_seconds": wait_seconds,
                })),
            )
                .into_response()
        }
    }
}

async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.logout(&auth.token).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn me(State(state): State<AppState>, headers: HeaderMap) -> Response {
    match authorize(&state, &headers).await {
        Ok(auth) => (StatusCode::OK, Json(serde_json::json!(auth.admin))).into_response(),
        Err(response) => response,
    }
}

async fn get_admin_sessions(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.active_admin_sessions(&auth.token).await {
        Ok(sessions) => (StatusCode::OK, Json(serde_json::json!(sessions))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn revoke_admin_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(session_id): axum::extract::Path<String>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.revoke_admin_session(&auth.token, &session_id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_ui_bootstrap(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.ui_bootstrap_snapshot(&auth.token).await {
        Ok(snapshot) => (StatusCode::OK, Json(serde_json::json!(snapshot))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_ui_overview(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.ui_overview_snapshot(&auth.token).await {
        Ok(snapshot) => (StatusCode::OK, Json(serde_json::json!(snapshot))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_ui_contracts(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.ui_contracts_snapshot(&auth.token).await {
        Ok(snapshot) => (StatusCode::OK, Json(serde_json::json!(snapshot))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_ui_security(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.ui_security_snapshot(&auth.token).await {
        Ok(snapshot) => (StatusCode::OK, Json(serde_json::json!(snapshot))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_ui_core(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.ui_core_snapshot(&auth.token).await {
        Ok(snapshot) => (StatusCode::OK, Json(serde_json::json!(snapshot))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_ui_users_summary(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.ui_users_summary_snapshot(&auth.token).await {
        Ok(snapshot) => (StatusCode::OK, Json(serde_json::json!(snapshot))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_ui_nodes_summary(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.ui_nodes_summary_snapshot(&auth.token).await {
        Ok(snapshot) => (StatusCode::OK, Json(serde_json::json!(snapshot))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_ui_clusters_summary(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.ui_clusters_summary_snapshot(&auth.token).await {
        Ok(snapshot) => (StatusCode::OK, Json(serde_json::json!(snapshot))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_ui_telegram_summary(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.ui_telegram_summary_snapshot(&auth.token).await {
        Ok(snapshot) => (StatusCode::OK, Json(serde_json::json!(snapshot))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_ui_audit_summary(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.ui_audit_summary_snapshot(&auth.token).await {
        Ok(snapshot) => (StatusCode::OK, Json(serde_json::json!(snapshot))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_ui_subscriptions_summary(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.ui_subscriptions_summary_snapshot(&auth.token).await {
        Ok(summary) => (StatusCode::OK, Json(serde_json::json!(summary))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_ui_protocols_summary(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.ui_protocols_summary_snapshot(&auth.token).await {
        Ok(summary) => (StatusCode::OK, Json(serde_json::json!(summary))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_ui_installer_summary(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.ui_installer_summary_snapshot(&auth.token).await {
        Ok(summary) => (StatusCode::OK, Json(serde_json::json!(summary))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_security_settings(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.get_security_settings(&auth.token).await {
        Ok(settings) => (StatusCode::OK, Json(serde_json::json!(settings))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn update_security_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<UpdateSecuritySettingsRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.update_security_settings(&auth.token, payload).await {
        Ok(settings) => (StatusCode::OK, Json(serde_json::json!(settings))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn apply_security_preset(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<ApplySecurityPresetRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.apply_security_preset(&auth.token, payload).await {
        Ok(settings) => (StatusCode::OK, Json(serde_json::json!(settings))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_security_status(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    let config = state.config_snapshot().await;
    let client_ip = resolve_client_ip(&config, addr.ip().to_string(), &headers);

    match state.get_login_status(&auth.token, &client_ip).await {
        Ok(status) => (StatusCode::OK, Json(serde_json::json!(status))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_telegram_settings(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.telegram_settings(&auth.token).await {
        Ok(settings) => (StatusCode::OK, Json(serde_json::json!(settings))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn update_telegram_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<UpdateTelegramSettingsRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.update_telegram_settings(&auth.token, payload).await {
        Ok(settings) => (StatusCode::OK, Json(serde_json::json!(settings))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_telegram_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<TelegramEventsQuery>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.telegram_events(&auth.token, query).await {
        Ok(events) => (StatusCode::OK, Json(serde_json::json!(events))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn retry_due_telegram_events(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.retry_due_telegram_events(&auth.token).await {
        Ok(events) => (StatusCode::OK, Json(serde_json::json!(events))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn send_telegram_test(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<TelegramTestRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.send_telegram_test(&auth.token, payload).await {
        Ok(event) => (StatusCode::OK, Json(serde_json::json!(event))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_security_audit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AuditEventsQuery>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.recent_audit_events(&auth.token, query).await {
        Ok(events) => (StatusCode::OK, Json(serde_json::json!(events))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_active_bans(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.active_bans(&auth.token).await {
        Ok(bans) => (StatusCode::OK, Json(serde_json::json!(bans))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn create_ban(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateBanRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.create_ban(&auth.token, payload).await {
        Ok(ban) => (StatusCode::OK, Json(serde_json::json!(ban))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn remove_ban(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(client_ip): axum::extract::Path<String>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.remove_ban(&auth.token, &client_ip).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_two_factor_state(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.two_factor_state(&auth.token).await {
        Ok(two_factor) => (StatusCode::OK, Json(serde_json::json!(two_factor))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn setup_two_factor(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.setup_two_factor(&auth.token).await {
        Ok(response) => (StatusCode::OK, Json(serde_json::json!(response))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn enable_two_factor(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<EnableTwoFactorRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.enable_two_factor(&auth.token, payload).await {
        Ok(response) => (StatusCode::OK, Json(serde_json::json!(response))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn disable_two_factor(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<DisableTwoFactorRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.disable_two_factor(&auth.token, payload).await {
        Ok(response) => (StatusCode::OK, Json(serde_json::json!(response))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn update_two_factor_two_step(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<UpdateTwoFactorTwoStepRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.update_two_factor_two_step(&auth.token, payload).await {
        Ok(response) => (StatusCode::OK, Json(serde_json::json!(response))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_panel_access_modes(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.panel_access_modes(&auth.token).await {
        Ok(view) => (StatusCode::OK, Json(serde_json::json!(view))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn plan_panel_install(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<PanelInstallPlanRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.panel_install_plan(&auth.token, payload).await {
        Ok(view) => (StatusCode::OK, Json(serde_json::json!(view))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn create_panel_installer_bootstrap(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<PanelInstallerBootstrapRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.panel_installer_bootstrap(&auth.token, payload).await {
        Ok(view) => (StatusCode::OK, Json(serde_json::json!(view))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn create_panel_installer_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<PanelInstallerSessionRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.panel_installer_session(&auth.token, payload).await {
        Ok(view) => (StatusCode::OK, Json(serde_json::json!(view))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn report_panel_installer_result(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<PanelInstallerSessionResultRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.panel_installer_result(&auth.token, payload).await {
        Ok(view) => (StatusCode::OK, Json(serde_json::json!(view))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn create_panel_installer_job(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<PanelInstallerCreateJobRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.create_panel_installer_job(&auth.token, payload).await {
        Ok(view) => (StatusCode::OK, Json(serde_json::json!(view))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn list_panel_installer_jobs(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.list_panel_installer_jobs(&auth.token).await {
        Ok(view) => (StatusCode::OK, Json(serde_json::json!(view))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn report_panel_installer_job_heartbeat(
    State(state): State<AppState>,
    Json(payload): Json<PanelInstallerJobHeartbeatRequest>,
) -> Response {
    match state.panel_installer_job_heartbeat(payload).await {
        Ok(view) => (StatusCode::OK, Json(serde_json::json!(view))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_panel_installer_job_for_executor(
    State(state): State<AppState>,
    Json(payload): Json<PanelInstallerJobAccessRequest>,
) -> Response {
    match state.panel_installer_job_for_executor(payload).await {
        Ok(view) => (StatusCode::OK, Json(serde_json::json!(view))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn report_panel_installer_job_result(
    State(state): State<AppState>,
    Json(payload): Json<PanelInstallerJobResultRequest>,
) -> Response {
    match state.panel_installer_job_result(payload).await {
        Ok(view) => (StatusCode::OK, Json(serde_json::json!(view))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_system_overview(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.system_overview(&auth.token).await {
        Ok(overview) => (StatusCode::OK, Json(serde_json::json!(overview))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_resource_budget(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.resource_budget_report(&auth.token).await {
        Ok(report) => (StatusCode::OK, Json(serde_json::json!(report))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_system_thresholds(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.system_thresholds(&auth.token).await {
        Ok(thresholds) => (StatusCode::OK, Json(serde_json::json!(thresholds))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_secret_key_readiness(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };
    match state.secret_key_readiness(&auth.token).await {
        Ok(readiness) => (StatusCode::OK, Json(serde_json::json!(readiness))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn update_system_thresholds(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<UpdateSystemThresholdsRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.update_system_thresholds(&auth.token, payload).await {
        Ok(thresholds) => (StatusCode::OK, Json(serde_json::json!(thresholds))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_system_alerts(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.active_system_alerts(&auth.token).await {
        Ok(alerts) => (StatusCode::OK, Json(serde_json::json!(alerts))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_system_alert_history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AlertHistoryQuery>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.filtered_alert_history(&auth.token, query).await {
        Ok(events) => (StatusCode::OK, Json(serde_json::json!(events))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_operational_logs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<OperationalLogsQuery>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.operational_logs(&auth.token, query).await {
        Ok(logs) => (StatusCode::OK, Json(serde_json::json!(logs))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_core_config(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.core_config_state(&auth.token).await {
        Ok(config) => (StatusCode::OK, Json(serde_json::json!(config))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_generated_core_config(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.generated_core_config(&auth.token).await {
        Ok(config) => (StatusCode::OK, Json(serde_json::json!(config))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_generated_xray_config(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.generated_xray_config(&auth.token).await {
        Ok(config) => (StatusCode::OK, Json(serde_json::json!(config))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn save_core_config(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<SaveCoreConfigRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.save_core_config(&auth.token, payload).await {
        Ok(config) => (StatusCode::OK, Json(serde_json::json!(config))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_generated_xray_config_validation(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.generated_xray_config_validation(&auth.token).await {
        Ok(report) => (StatusCode::OK, Json(serde_json::json!(report))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_generated_xray_config_external_validation(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .generated_xray_config_external_validation(&auth.token)
        .await
    {
        Ok(report) => (StatusCode::OK, Json(serde_json::json!(report))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_route_materials(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.route_materials_view(&auth.token).await {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn rotate_route_credential(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<RouteCredentialActionRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .rotate_route_credential(&auth.token, &payload.credential_ref)
        .await
    {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn revoke_route_credential(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<RouteCredentialActionRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .revoke_route_credential(&auth.token, &payload.credential_ref)
        .await
    {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn rotate_route_ca(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.rotate_route_ca(&auth.token).await {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_core_state(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.core_runtime_state(&auth.token).await {
        Ok(core_state) => (StatusCode::OK, Json(serde_json::json!(core_state))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn apply_generated_core_config(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CoreApplyRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.core_apply_generated(&auth.token, payload).await {
        Ok(record) => (StatusCode::OK, Json(serde_json::json!(record))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_core_apply_history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<CoreApplyHistoryQuery>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.core_apply_history(&auth.token, query).await {
        Ok(records) => (StatusCode::OK, Json(serde_json::json!(records))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn execute_core_action(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CoreActionRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.execute_core_action(&auth.token, payload.action).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn restart_core(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.restart_core(&auth.token).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn update_xray_core(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<XrayCoreUpdateRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.xray_core_update(&auth.token, payload).await {
        Ok(report) => (StatusCode::OK, Json(serde_json::json!(report))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn list_users(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<UsersQuery>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.list_users(&auth.token, query).await {
        Ok(users) => (StatusCode::OK, Json(serde_json::json!(users))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_users_activity(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<UserActivityQuery>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.user_activity(&auth.token, None, query).await {
        Ok(items) => (StatusCode::OK, Json(serde_json::json!(items))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn create_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateUserRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.create_user(&auth.token, payload).await {
        Ok(user) => (StatusCode::OK, Json(serde_json::json!(user))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(username): axum::extract::Path<String>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.get_user(&auth.token, &username).await {
        Ok(user) => (StatusCode::OK, Json(serde_json::json!(user))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn update_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(username): axum::extract::Path<String>,
    Json(payload): Json<UpdateUserRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.update_user(&auth.token, &username, payload).await {
        Ok(user) => (StatusCode::OK, Json(serde_json::json!(user))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn delete_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(username): axum::extract::Path<String>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.delete_user(&auth.token, &username).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn reset_user_usage(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(username): axum::extract::Path<String>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.reset_user_usage(&auth.token, &username).await {
        Ok(user) => (StatusCode::OK, Json(serde_json::json!(user))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn report_user_usage(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(username): axum::extract::Path<String>,
    Json(payload): Json<ReportUserUsageRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .report_user_usage(&auth.token, &username, payload)
        .await
    {
        Ok(user) => (StatusCode::OK, Json(serde_json::json!(user))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_user_activity(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(username): axum::extract::Path<String>,
    Query(query): Query<UserActivityQuery>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .user_activity(&auth.token, Some(&username), query)
        .await
    {
        Ok(items) => (StatusCode::OK, Json(serde_json::json!(items))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn revoke_user_subscription(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(username): axum::extract::Path<String>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.revoke_user_subscription(&auth.token, &username).await {
        Ok(user) => (StatusCode::OK, Json(serde_json::json!(user))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_user_subscription(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(username): axum::extract::Path<String>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.user_subscription(&auth.token, &username).await {
        Ok(subscription) => (StatusCode::OK, Json(serde_json::json!(subscription))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn render_user_subscription(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(username): axum::extract::Path<String>,
    Query(query): Query<SubscriptionQuery>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };
    let format = query.format.unwrap_or(SubscriptionFormat::Json);

    match state
        .render_user_subscription(&auth.token, &username, format)
        .await
    {
        Ok(subscription) => (StatusCode::OK, Json(serde_json::json!(subscription))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_user_config_preview(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(username): axum::extract::Path<String>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.user_config_preview(&auth.token, &username).await {
        Ok(preview) => (StatusCode::OK, Json(serde_json::json!(preview))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_generated_user_config(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(username): axum::extract::Path<String>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.generated_user_config(&auth.token, &username).await {
        Ok(generated) => (StatusCode::OK, Json(serde_json::json!(generated))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn list_user_templates(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.list_user_templates(&auth.token).await {
        Ok(items) => (StatusCode::OK, Json(serde_json::json!(items))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn create_user_template(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateUserTemplateRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.create_user_template(&auth.token, payload).await {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn update_user_template(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(template_id): axum::extract::Path<String>,
    Json(payload): Json<UpdateUserTemplateRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .update_user_template(&auth.token, &template_id, payload)
        .await
    {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn delete_user_template(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(template_id): axum::extract::Path<String>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.delete_user_template(&auth.token, &template_id).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn list_subscription_plans(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<SubscriptionCatalogQuery>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.list_subscription_plans(&auth.token, query).await {
        Ok(items) => (StatusCode::OK, Json(serde_json::json!(items))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn create_subscription_plan(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateSubscriptionPlanRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.create_subscription_plan(&auth.token, payload).await {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_subscription_plan(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(plan_id): axum::extract::Path<String>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.get_subscription_plan(&auth.token, &plan_id).await {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn update_subscription_plan(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(plan_id): axum::extract::Path<String>,
    Json(payload): Json<UpdateSubscriptionPlanRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .update_subscription_plan(&auth.token, &plan_id, payload)
        .await
    {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn delete_subscription_plan(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(plan_id): axum::extract::Path<String>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.delete_subscription_plan(&auth.token, &plan_id).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn list_subscription_plan_clients(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(plan_id): axum::extract::Path<String>,
    Query(query): Query<SubscriptionCatalogQuery>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .list_subscription_clients(&auth.token, Some(&plan_id), query)
        .await
    {
        Ok(items) => (StatusCode::OK, Json(serde_json::json!(items))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn create_subscription_client(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(plan_id): axum::extract::Path<String>,
    Json(payload): Json<CreateSubscriptionClientRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .create_subscription_client(&auth.token, &plan_id, payload)
        .await
    {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_subscription_client(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(client_id): axum::extract::Path<String>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.get_subscription_client(&auth.token, &client_id).await {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn update_subscription_client(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(client_id): axum::extract::Path<String>,
    Json(payload): Json<UpdateSubscriptionClientRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .update_subscription_client(&auth.token, &client_id, payload)
        .await
    {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn render_subscription_client(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(client_id): axum::extract::Path<String>,
    Query(query): Query<SubscriptionQuery>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };
    let format = query.format.unwrap_or(SubscriptionFormat::Json);

    match state
        .render_subscription_client(&auth.token, &client_id, format, query.device_id.as_deref())
        .await
    {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_subscription_client_access_preview(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(client_id): axum::extract::Path<String>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .subscription_client_access_preview(&auth.token, &client_id)
        .await
    {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn update_subscription_client_access(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(client_id): axum::extract::Path<String>,
    Json(payload): Json<UpdateSubscriptionClientAccessRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .update_subscription_client_access(&auth.token, &client_id, payload)
        .await
    {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn list_subscription_client_devices(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(client_id): axum::extract::Path<String>,
    Query(query): Query<SubscriptionDevicesQuery>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .list_subscription_client_devices(&auth.token, &client_id, query)
        .await
    {
        Ok(items) => (StatusCode::OK, Json(serde_json::json!(items))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn list_subscription_device_enrollments(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(client_id): axum::extract::Path<String>,
    Query(query): Query<SubscriptionDeviceEnrollmentsQuery>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .list_subscription_device_enrollments(&auth.token, &client_id, query)
        .await
    {
        Ok(items) => (StatusCode::OK, Json(serde_json::json!(items))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn create_subscription_device_enrollment(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(client_id): axum::extract::Path<String>,
    Json(payload): Json<CreateSubscriptionDeviceEnrollmentRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .create_subscription_device_enrollment(&auth.token, &client_id, payload)
        .await
    {
        Ok(item) => (
            StatusCode::CREATED,
            [(http::header::CACHE_CONTROL, "private, no-store")],
            Json(item),
        )
            .into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn revoke_subscription_device_enrollment(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path((client_id, grant_id)): axum::extract::Path<(String, String)>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .revoke_subscription_device_enrollment(&auth.token, &client_id, &grant_id)
        .await
    {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn register_subscription_client_device(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(client_id): axum::extract::Path<String>,
    Json(payload): Json<RegisterSubscriptionDeviceRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .register_subscription_client_device(&auth.token, &client_id, payload)
        .await
    {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn revoke_subscription_client_device(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path((client_id, device_id)): axum::extract::Path<(String, String)>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .revoke_subscription_client_device(&auth.token, &client_id, &device_id)
        .await
    {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn list_subscription_client_sessions(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(client_id): axum::extract::Path<String>,
    Query(query): Query<SubscriptionSessionsQuery>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .list_subscription_client_sessions(&auth.token, &client_id, query)
        .await
    {
        Ok(items) => (StatusCode::OK, Json(serde_json::json!(items))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_subscription_client_usage(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(client_id): axum::extract::Path<String>,
    Query(query): Query<SubscriptionUsageQuery>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .subscription_client_usage_detail(&auth.token, &client_id, query)
        .await
    {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn reset_subscription_client_usage(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(client_id): axum::extract::Path<String>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .reset_subscription_client_usage(&auth.token, &client_id)
        .await
    {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn report_subscription_client_usage(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(client_id): axum::extract::Path<String>,
    Json(payload): Json<ReportSubscriptionUsageRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .report_subscription_client_usage(&auth.token, &client_id, payload)
        .await
    {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn revoke_subscription_client(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(client_id): axum::extract::Path<String>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .revoke_subscription_client(&auth.token, &client_id)
        .await
    {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn delete_subscription_client(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(client_id): axum::extract::Path<String>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .delete_subscription_client(&auth.token, &client_id)
        .await
    {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn list_inbounds(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.list_inbounds(&auth.token).await {
        Ok(items) => (StatusCode::OK, Json(serde_json::json!(items))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_protocol_capabilities(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };
    match state.protocol_capabilities(&auth.token).await {
        Ok(items) => (StatusCode::OK, Json(serde_json::json!(items))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn create_inbound(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateInboundRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.create_inbound(&auth.token, payload).await {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn update_inbound(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(tag): axum::extract::Path<String>,
    Json(payload): Json<UpdateInboundRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.update_inbound(&auth.token, &tag, payload).await {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn delete_inbound(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(tag): axum::extract::Path<String>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.delete_inbound(&auth.token, &tag).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn list_hosts(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.list_hosts(&auth.token).await {
        Ok(items) => (StatusCode::OK, Json(serde_json::json!(items))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn create_host(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateHostRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.create_host(&auth.token, payload).await {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn update_host(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(host_id): axum::extract::Path<String>,
    Json(payload): Json<UpdateHostRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.update_host(&auth.token, &host_id, payload).await {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn delete_host(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(host_id): axum::extract::Path<String>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.delete_host(&auth.token, &host_id).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn list_proxy_profiles(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.list_proxy_profiles(&auth.token).await {
        Ok(items) => (StatusCode::OK, Json(serde_json::json!(items))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn create_proxy_profile(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateProxyProfileRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.create_proxy_profile(&auth.token, payload).await {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn update_proxy_profile(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(profile_id): axum::extract::Path<String>,
    Json(payload): Json<UpdateProxyProfileRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .update_proxy_profile(&auth.token, &profile_id, payload)
        .await
    {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn delete_proxy_profile(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(profile_id): axum::extract::Path<String>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.delete_proxy_profile(&auth.token, &profile_id).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn list_clusters(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.list_clusters(&auth.token).await {
        Ok(items) => (StatusCode::OK, Json(serde_json::json!(items))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn create_cluster(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateClusterRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.create_cluster(&auth.token, payload).await {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn update_cluster(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(cluster_id): axum::extract::Path<String>,
    Json(payload): Json<UpdateClusterRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .update_cluster(&auth.token, &cluster_id, payload)
        .await
    {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn delete_cluster(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(cluster_id): axum::extract::Path<String>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.delete_cluster(&auth.token, &cluster_id).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn validate_cluster(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(cluster_id): axum::extract::Path<String>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.cluster_validation(&auth.token, &cluster_id).await {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn preview_cluster(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(cluster_id): axum::extract::Path<String>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.cluster_preview(&auth.token, &cluster_id).await {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn list_nodes(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.list_nodes(&auth.token).await {
        Ok(items) => (StatusCode::OK, Json(serde_json::json!(items))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_node_health_center(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.node_health_center(&auth.token).await {
        Ok(view) => (StatusCode::OK, Json(serde_json::json!(view))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn create_node(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateNodeRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.create_node(&auth.token, payload).await {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn update_node(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(node_id): axum::extract::Path<String>,
    Json(payload): Json<UpdateNodeRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.update_node(&auth.token, &node_id, payload).await {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn rotate_node_auth_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(node_id): axum::extract::Path<String>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.rotate_node_auth_token(&auth.token, &node_id).await {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_node_cluster_targets(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(node_id): axum::extract::Path<String>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.node_cluster_targets(&auth.token, &node_id).await {
        Ok(items) => (StatusCode::OK, Json(serde_json::json!(items))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn request_node_apply(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(node_id): axum::extract::Path<String>,
    Json(payload): Json<NodeApplyRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .request_node_apply(&auth.token, &node_id, payload)
        .await
    {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn retry_node_apply(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(node_id): axum::extract::Path<String>,
    Json(payload): Json<NodeApplyRetryRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.retry_node_apply(&auth.token, &node_id, payload).await {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn request_node_rollback(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(node_id): axum::extract::Path<String>,
    Json(payload): Json<NodeRollbackRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .request_node_rollback(&auth.token, &node_id, payload)
        .await
    {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_node_diagnostics(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(node_id): axum::extract::Path<String>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.node_diagnostics(&auth.token, &node_id).await {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_node_apply_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(node_id): axum::extract::Path<String>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.node_apply_status(&auth.token, &node_id).await {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn list_node_provisioning_tasks(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(node_id): axum::extract::Path<String>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .list_node_provisioning_tasks(&auth.token, &node_id)
        .await
    {
        Ok(items) => (StatusCode::OK, Json(serde_json::json!(items))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_node_provisioning_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(node_id): axum::extract::Path<String>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.node_provisioning_status(&auth.token, &node_id).await {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_node_provisioning_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(node_id): axum::extract::Path<String>,
    Query(query): Query<NodeProvisioningEventsQuery>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .node_provisioning_events(&auth.token, &node_id, query)
        .await
    {
        Ok(items) => (StatusCode::OK, Json(serde_json::json!(items))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_node_provisioning_preflight(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(node_id): axum::extract::Path<String>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .node_provisioning_preflight(&auth.token, &node_id)
        .await
    {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_node_provisioning_ssh_preflight_probe(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(node_id): axum::extract::Path<String>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .node_provisioning_ssh_preflight_probe(&auth.token, &node_id)
        .await
    {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_node_provisioning_ssh_install_plan(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(node_id): axum::extract::Path<String>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .node_provisioning_ssh_install_plan(&auth.token, &node_id)
        .await
    {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_node_provisioning_executor_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path((node_id, task_id)): axum::extract::Path<(String, String)>,
    Query(query): Query<NodeProvisioningExecutorSessionQuery>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .node_provisioning_executor_session(
            &auth.token,
            &node_id,
            &task_id,
            query.executor_contract_version,
        )
        .await
    {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn node_provisioning_executor_handshake(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<NodeProvisioningExecutorHandshakeRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .node_provisioning_executor_handshake(&auth.token, payload)
        .await
    {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn list_node_provisioning_executors(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.node_provisioning_executors(&auth.token).await {
        Ok(items) => (StatusCode::OK, Json(serde_json::json!(items))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn list_node_provisioning_executor_submissions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<NodeProvisioningExecutorSubmissionsQuery>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .node_provisioning_executor_submissions(&auth.token, query)
        .await
    {
        Ok(items) => (StatusCode::OK, Json(serde_json::json!(items))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn update_node_provisioning_executor_trust(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(executor_id): axum::extract::Path<String>,
    Json(payload): Json<UpdateNodeProvisioningExecutorTrustRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .update_node_provisioning_executor_trust(&auth.token, &executor_id, payload)
        .await
    {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn rotate_node_provisioning_executor_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(executor_id): axum::extract::Path<String>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .rotate_node_provisioning_executor_token(&auth.token, &executor_id)
        .await
    {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn start_node_provisioning(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(node_id): axum::extract::Path<String>,
    Json(payload): Json<StartNodeProvisioningRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .start_node_provisioning(&auth.token, &node_id, payload)
        .await
    {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn reprovision_node(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(node_id): axum::extract::Path<String>,
    Json(payload): Json<ReprovisionNodeRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.reprovision_node(&auth.token, &node_id, payload).await {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn update_node_provisioning_step(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path((node_id, task_id)): axum::extract::Path<(String, String)>,
    Query(query): Query<NodeProvisioningExecutorResultQuery>,
    Json(payload): Json<UpdateNodeProvisioningStepRequest>,
) -> Response {
    if let Err(response) = authorize_executor(&state, &headers, &query).await {
        return response;
    }

    let accepted_node_id = Some(format!("step_{}", provisioning_step_label(payload.step)));
    match state
        .update_node_provisioning_step(
            &node_id,
            &task_id,
            query.executor_id.as_deref(),
            query.executor_contract_version,
            payload,
        )
        .await
    {
        Ok(item) => match state
            .node_provisioning_executor_accepted_result(&node_id, item, accepted_node_id)
            .await
        {
            Ok(result) => (StatusCode::OK, Json(serde_json::json!(result))).into_response(),
            Err(error) => map_node_provisioning_executor_error(error),
        },
        Err(error) => map_node_provisioning_executor_error(error),
    }
}

async fn authorize_executor(
    state: &AppState,
    headers: &HeaderMap,
    query: &NodeProvisioningExecutorResultQuery,
) -> Result<(), Response> {
    let executor_id = query
        .executor_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            map_node_provisioning_executor_error(SecurityError::InvalidSecuritySettings(
                "executor id is required",
            ))
        })?;
    let auth_header = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| StatusCode::UNAUTHORIZED.into_response())?;
    let token = auth_header
        .strip_prefix("Bearer ")
        .ok_or_else(|| StatusCode::UNAUTHORIZED.into_response())?;
    state
        .authenticate_node_provisioning_executor(executor_id, token)
        .await
        .map(|_| ())
        .map_err(|error| match error {
            SecurityError::InvalidSecuritySettings(_) => {
                map_node_provisioning_executor_error(error)
            }
            other => map_security_error(other),
        })
}

async fn touch_node_provisioning_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path((node_id, task_id)): axum::extract::Path<(String, String)>,
    Json(payload): Json<TouchNodeProvisioningTaskRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .touch_node_provisioning_task(&auth.token, &node_id, &task_id, payload)
        .await
    {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn report_node_provisioning_command(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path((node_id, task_id)): axum::extract::Path<(String, String)>,
    Query(query): Query<NodeProvisioningExecutorResultQuery>,
    Json(payload): Json<ReportNodeProvisioningCommandRequest>,
) -> Response {
    if let Err(response) = authorize_executor(&state, &headers, &query).await {
        return response;
    }

    let accepted_node_id = Some(format!("step_{}", provisioning_step_label(payload.step)));
    match state
        .report_node_provisioning_command(
            &node_id,
            &task_id,
            query.executor_id.as_deref(),
            query.executor_contract_version,
            payload,
        )
        .await
    {
        Ok(item) => match state
            .node_provisioning_executor_accepted_result(&node_id, item, accepted_node_id)
            .await
        {
            Ok(result) => (StatusCode::OK, Json(serde_json::json!(result))).into_response(),
            Err(error) => map_node_provisioning_executor_error(error),
        },
        Err(error) => map_node_provisioning_executor_error(error),
    }
}

async fn report_node_provisioning_handoff(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path((node_id, task_id)): axum::extract::Path<(String, String)>,
    Query(query): Query<NodeProvisioningExecutorResultQuery>,
    Json(payload): Json<ReportNodeProvisioningHandoffRequest>,
) -> Response {
    if let Err(response) = authorize_executor(&state, &headers, &query).await {
        return response;
    }

    let accepted_node_id = Some(format!(
        "handoff_{}",
        provisioning_handoff_label(payload.kind)
    ));
    match state
        .report_node_provisioning_handoff(
            &node_id,
            &task_id,
            query.executor_id.as_deref(),
            query.executor_contract_version,
            payload,
        )
        .await
    {
        Ok(item) => match state
            .node_provisioning_executor_accepted_result(&node_id, item, accepted_node_id)
            .await
        {
            Ok(result) => (StatusCode::OK, Json(serde_json::json!(result))).into_response(),
            Err(error) => map_node_provisioning_executor_error(error),
        },
        Err(error) => map_node_provisioning_executor_error(error),
    }
}

async fn finish_node_provisioning(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path((node_id, task_id)): axum::extract::Path<(String, String)>,
    Json(payload): Json<FinishNodeProvisioningRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .finish_node_provisioning(&auth.token, &node_id, &task_id, payload)
        .await
    {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(SecurityError::InvalidSecuritySettings(
            "provisioning cannot finish before required executor steps succeed",
        ))
        | Err(SecurityError::InvalidSecuritySettings(
            "ssh provisioning cannot finish before required handoff reports succeed",
        )) => match state
            .node_provisioning_finish_rejection(&auth.token, &node_id, &task_id)
            .await
        {
            Ok(rejection) => {
                (StatusCode::BAD_REQUEST, Json(serde_json::json!(rejection))).into_response()
            }
            Err(error) => map_security_error(error),
        },
        Err(error) => map_security_error(error),
    }
}

async fn retry_node_provisioning(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path((node_id, task_id)): axum::extract::Path<(String, String)>,
    Json(payload): Json<RetryNodeProvisioningRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .retry_node_provisioning(&auth.token, &node_id, &task_id, payload)
        .await
    {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn run_node_bootstrap_probe(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(node_id): axum::extract::Path<String>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.run_node_bootstrap_probe(&auth.token, &node_id).await {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_node_bootstrap_readiness(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(node_id): axum::extract::Path<String>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.node_bootstrap_readiness(&auth.token, &node_id).await {
        Ok(NodeBootstrapReadinessView {
            node,
            ready,
            checked_at_unix,
            failed_steps,
            recommendations,
        }) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "node": node,
                "ready": ready,
                "checked_at_unix": checked_at_unix,
                "failed_steps": failed_steps,
                "recommendations": recommendations,
            })),
        )
            .into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_node_bootstrap_history(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(node_id): axum::extract::Path<String>,
    Query(query): Query<LimitQuery>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .node_bootstrap_history(&auth.token, &node_id, query.limit)
        .await
    {
        Ok(items) => (StatusCode::OK, Json(serde_json::json!(items))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_node_local_health(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(node_id): axum::extract::Path<String>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.node_local_health(&auth.token, &node_id).await {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_node_local_state(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(node_id): axum::extract::Path<String>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.node_local_state(&auth.token, &node_id).await {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn execute_node_local_runtime_action(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path((node_id, action)): axum::extract::Path<(String, String)>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .node_local_runtime_action(&auth.token, &node_id, &action)
        .await
    {
        Ok(NodeLocalActionResponse { detail }) => (
            StatusCode::OK,
            Json(serde_json::json!({ "detail": detail })),
        )
            .into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn execute_node_local_runtime_component_action(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path((node_id, component, action)): axum::extract::Path<(
        String,
        String,
        String,
    )>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .node_local_runtime_component_action(&auth.token, &node_id, &component, &action)
        .await
    {
        Ok(NodeLocalActionResponse { detail }) => (
            StatusCode::OK,
            Json(serde_json::json!({ "detail": detail })),
        )
            .into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn update_node_local_xray(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(node_id): axum::extract::Path<String>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.node_local_xray_update(&auth.token, &node_id).await {
        Ok(NodeLocalActionResponse { detail }) => (
            StatusCode::OK,
            Json(serde_json::json!({ "detail": detail })),
        )
            .into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn delete_node(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(node_id): axum::extract::Path<String>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state.delete_node(&auth.token, &node_id).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn record_node_heartbeat(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(node_id): axum::extract::Path<String>,
    Json(payload): Json<NodeHeartbeatRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .record_node_heartbeat(&auth.token, &node_id, payload)
        .await
    {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn update_node_sync_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(node_id): axum::extract::Path<String>,
    Json(payload): Json<NodeSyncRequest>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .update_node_sync_status(&auth.token, &node_id, payload)
        .await
    {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_node_sync_history(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(node_id): axum::extract::Path<String>,
    Query(query): Query<LimitQuery>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .node_sync_history(&auth.token, &node_id, query.limit)
        .await
    {
        Ok(items) => (StatusCode::OK, Json(serde_json::json!(items))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn get_node_apply_results(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(node_id): axum::extract::Path<String>,
    Query(query): Query<LimitQuery>,
) -> Response {
    let auth = match authorize(&state, &headers).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };

    match state
        .node_apply_results(&auth.token, &node_id, query.limit)
        .await
    {
        Ok(items) => (StatusCode::OK, Json(serde_json::json!(items))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn node_agent_me(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth_token = match authorize_node(&headers) {
        Ok(token) => token,
        Err(response) => return response,
    };

    match state.node_agent_me(&auth_token).await {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn node_agent_config(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth_token = match authorize_node(&headers) {
        Ok(token) => token,
        Err(response) => return response,
    };

    match state.node_agent_config(&auth_token).await {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn node_agent_xray_config(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth_token = match authorize_node(&headers) {
        Ok(token) => token,
        Err(response) => return response,
    };

    match state.node_agent_xray_config(&auth_token).await {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn node_agent_route_credentials(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    let auth_token = match authorize_node(&headers) {
        Ok(token) => token,
        Err(response) => return response,
    };

    match state.node_agent_route_credentials(&auth_token).await {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn node_agent_cluster_targets(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth_token = match authorize_node(&headers) {
        Ok(token) => token,
        Err(response) => return response,
    };

    match state.node_agent_cluster_targets(&auth_token).await {
        Ok(items) => (StatusCode::OK, Json(serde_json::json!(items))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn node_agent_heartbeat(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<NodeHeartbeatRequest>,
) -> Response {
    let auth_token = match authorize_node(&headers) {
        Ok(token) => token,
        Err(response) => return response,
    };

    match state.node_agent_heartbeat(&auth_token, payload).await {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn node_agent_sync(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<NodeSyncRequest>,
) -> Response {
    let auth_token = match authorize_node(&headers) {
        Ok(token) => token,
        Err(response) => return response,
    };

    match state.node_agent_sync(&auth_token, payload).await {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn node_agent_apply_result(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<NodeApplyResultRequest>,
) -> Response {
    let auth_token = match authorize_node(&headers) {
        Ok(token) => token,
        Err(response) => return response,
    };

    match state.node_agent_apply_result(&auth_token, payload).await {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn node_agent_logs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<NodeLogUploadRequest>,
) -> Response {
    let auth_token = match authorize_node(&headers) {
        Ok(token) => token,
        Err(response) => return response,
    };

    match state.node_agent_upload_logs(&auth_token, payload).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn node_agent_metrics(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<NodeMetricsRequest>,
) -> Response {
    let auth_token = match authorize_node(&headers) {
        Ok(token) => token,
        Err(response) => return response,
    };

    match state.node_agent_upload_metrics(&auth_token, payload).await {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn node_agent_subscription_sessions_report(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<ReportSubscriptionSessionsRequest>,
) -> Response {
    let auth_token = match authorize_node(&headers) {
        Ok(token) => token,
        Err(response) => return response,
    };

    match state
        .node_agent_report_subscription_sessions(&auth_token, payload)
        .await
    {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn node_agent_subscription_client_usage_report(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(client_id): axum::extract::Path<String>,
    Json(payload): Json<ReportSubscriptionUsageRequest>,
) -> Response {
    let auth_token = match authorize_node(&headers) {
        Ok(token) => token,
        Err(response) => return response,
    };

    match state
        .node_agent_report_subscription_usage(&auth_token, &client_id, payload)
        .await
    {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn node_agent_subscription_session_enforcement_result(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<ReportSubscriptionSessionEnforcementResultRequest>,
) -> Response {
    let auth_token = match authorize_node(&headers) {
        Ok(token) => token,
        Err(response) => return response,
    };

    match state
        .node_agent_report_subscription_session_enforcement_result(&auth_token, payload)
        .await
    {
        Ok(item) => (StatusCode::OK, Json(serde_json::json!(item))).into_response(),
        Err(error) => map_security_error(error),
    }
}

async fn set_security_headers(request: axum::extract::Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    headers.insert(
        http::header::CACHE_CONTROL,
        HeaderValue::from_static("no-store"),
    );
    response
}

#[derive(Debug, serde::Deserialize)]
struct LimitQuery {
    limit: Option<usize>,
}

struct AdminSession {
    token: String,
    admin: AuthenticatedAdmin,
}

/// `Response` as the error type is the axum idiom: an authorization failure is
/// already the reply that goes on the wire, so boxing it would only add an
/// allocation on the rejection path and an unwrap at each of the callers.
#[allow(clippy::result_large_err)]
fn authorize_node(headers: &HeaderMap) -> Result<String, Response> {
    headers
        .get("x-hydra-node-token")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| StatusCode::UNAUTHORIZED.into_response())
}

async fn authorize(state: &AppState, headers: &HeaderMap) -> Result<AdminSession, Response> {
    let auth_header = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| StatusCode::UNAUTHORIZED.into_response())?;
    let token = auth_header
        .strip_prefix("Bearer ")
        .map(str::to_string)
        .ok_or_else(|| StatusCode::UNAUTHORIZED.into_response())?;

    let admin = state.me(&token).await.map_err(map_security_error)?;

    Ok(AdminSession { token, admin })
}

fn map_security_error(error: SecurityError) -> Response {
    let status = match error {
        SecurityError::Unauthorized => StatusCode::UNAUTHORIZED,
        SecurityError::IpBanned => StatusCode::TOO_MANY_REQUESTS,
        SecurityError::AdminNotConfigured => StatusCode::SERVICE_UNAVAILABLE,
        SecurityError::InvalidCredentials => StatusCode::UNAUTHORIZED,
        SecurityError::SessionLimitReached => StatusCode::TOO_MANY_REQUESTS,
        SecurityError::InvalidSecuritySettings(_) => StatusCode::BAD_REQUEST,
        SecurityError::PersistenceFailure => StatusCode::INTERNAL_SERVER_ERROR,
        SecurityError::InvalidTwoFactorCode => StatusCode::UNAUTHORIZED,
        SecurityError::TwoFactorNotConfigured => StatusCode::BAD_REQUEST,
        SecurityError::TwoFactorNotEnabled => StatusCode::BAD_REQUEST,
        SecurityError::MissingPersistencePolicy => StatusCode::INTERNAL_SERVER_ERROR,
    };

    (
        status,
        Json(serde_json::json!({
            "error": error.to_string(),
        })),
    )
        .into_response()
}

fn map_node_provisioning_executor_error(error: SecurityError) -> Response {
    let SecurityError::InvalidSecuritySettings(message) = error else {
        return map_security_error(error);
    };
    let (code, recovery_hint, retryable) = match message {
        "executor contract version is missing or incompatible" => (
            NodeProvisioningExecutorRejectionCode::IncompatibleExecutorContract,
            NodeProvisioningExecutorRecoveryHint::RefreshSession,
            false,
        ),
        "executor id is required" => (
            NodeProvisioningExecutorRejectionCode::ExecutorIdentityRequired,
            NodeProvisioningExecutorRecoveryHint::RefreshSession,
            false,
        ),
        "executor is not registered" | "executor registration is not compatible" => (
            NodeProvisioningExecutorRejectionCode::ExecutorNotRegistered,
            NodeProvisioningExecutorRecoveryHint::RefreshSession,
            false,
        ),
        "executor is disabled" => (
            NodeProvisioningExecutorRejectionCode::ExecutorDisabled,
            NodeProvisioningExecutorRecoveryHint::Stop,
            false,
        ),
        "successful preflight step requires ssh preflight output or remote prerequisites" => (
            NodeProvisioningExecutorRejectionCode::InvalidPreflightEvidence,
            NodeProvisioningExecutorRecoveryHint::RetryWithCorrectPayload,
            true,
        ),
        "successful command step requires command-report evidence" => (
            NodeProvisioningExecutorRejectionCode::WrongResultChannel,
            NodeProvisioningExecutorRecoveryHint::UseCommandReport,
            true,
        ),
        "successful node_env_written handoff requires valid node env attestation" => (
            NodeProvisioningExecutorRejectionCode::MissingNodeEnvAttestation,
            NodeProvisioningExecutorRecoveryHint::ReattestRemoteState,
            true,
        ),
        "successful service_started handoff requires valid service attestation" => (
            NodeProvisioningExecutorRejectionCode::MissingServiceAttestation,
            NodeProvisioningExecutorRecoveryHint::ReattestRemoteState,
            true,
        ),
        "only active provisioning tasks accept step updates"
        | "only active provisioning tasks accept handoff reports" => (
            NodeProvisioningExecutorRejectionCode::TaskNotActive,
            NodeProvisioningExecutorRecoveryHint::RefreshSession,
            false,
        ),
        "provisioning task does not exist" => (
            NodeProvisioningExecutorRejectionCode::TaskNotFound,
            NodeProvisioningExecutorRecoveryHint::Stop,
            false,
        ),
        _ => (
            NodeProvisioningExecutorRejectionCode::InvalidResult,
            NodeProvisioningExecutorRecoveryHint::RetryWithCorrectPayload,
            true,
        ),
    };
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!(NodeProvisioningExecutorRejectionView {
            error: message.to_string(),
            code,
            recovery_hint,
            retryable,
        })),
    )
        .into_response()
}

fn provisioning_step_label(
    step: panel_domain::provisioning::NodeProvisioningStepKind,
) -> &'static str {
    use panel_domain::provisioning::NodeProvisioningStepKind;
    match step {
        NodeProvisioningStepKind::Preflight => "preflight",
        NodeProvisioningStepKind::AgentReachability => "agent_reachability",
        NodeProvisioningStepKind::RuntimeHealth => "runtime_health",
        NodeProvisioningStepKind::ConfigApply => "config_apply",
        NodeProvisioningStepKind::BootstrapVerify => "bootstrap_verify",
        NodeProvisioningStepKind::SshConnect => "ssh_connect",
        NodeProvisioningStepKind::SudoCheck => "sudo_check",
        NodeProvisioningStepKind::XrayInstall => "xray_install",
        NodeProvisioningStepKind::SidecarRuntimeInstall => "sidecar_runtime_install",
        NodeProvisioningStepKind::NodeInstall => "node_install",
        NodeProvisioningStepKind::ServiceInstall => "service_install",
    }
}

fn provisioning_handoff_label(
    handoff: panel_domain::provisioning::NodeProvisioningHandoffKind,
) -> &'static str {
    use panel_domain::provisioning::NodeProvisioningHandoffKind;
    match handoff {
        NodeProvisioningHandoffKind::TokenIssued => "token_issued",
        NodeProvisioningHandoffKind::NodeEnvWritten => "node_env_written",
        NodeProvisioningHandoffKind::ServiceStarted => "service_started",
        NodeProvisioningHandoffKind::AgentReturned => "agent_returned",
    }
}

fn resolve_client_ip(config: &AppConfig, direct_ip: String, headers: &HeaderMap) -> String {
    if !config.security.proxy_trust.trust_x_forwarded_for {
        return direct_ip;
    }

    let direct_ip_addr = match direct_ip.parse::<IpAddr>() {
        Ok(ip) => ip,
        Err(_) => return direct_ip,
    };

    if !is_trusted_proxy_ip(config, direct_ip_addr) {
        return direct_ip;
    }

    let Some(mut chain) = parse_x_forwarded_for_chain(headers) else {
        return direct_ip;
    };

    chain.push(direct_ip_addr);
    chain
        .into_iter()
        .rev()
        .find(|ip| !is_trusted_proxy_ip(config, *ip))
        .map(|ip| ip.to_string())
        .unwrap_or(direct_ip)
}

fn parse_x_forwarded_for_chain(headers: &HeaderMap) -> Option<Vec<IpAddr>> {
    let mut chain = Vec::new();
    for value in headers.get_all("x-forwarded-for") {
        let value = value.to_str().ok()?;
        for candidate in value.split(',').map(str::trim) {
            if candidate.is_empty() {
                return None;
            }
            chain.push(candidate.parse::<IpAddr>().ok()?);
        }
    }
    (!chain.is_empty()).then_some(chain)
}

fn is_trusted_proxy_ip(config: &AppConfig, ip: IpAddr) -> bool {
    let ip_text = ip.to_string();
    config
        .security
        .proxy_trust
        .trusted_proxy_ips
        .iter()
        .any(|trusted| trusted == &ip_text)
        || config
            .security
            .proxy_trust
            .trusted_proxy_cidrs
            .iter()
            .filter_map(|cidr| cidr.parse::<ipnet::IpNet>().ok())
            .any(|cidr: ipnet::IpNet| cidr.contains(&ip))
}

fn init_tracing() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "panel_app=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proxy_config() -> AppConfig {
        let mut config = AppConfig::default();
        config.security.proxy_trust.trust_x_forwarded_for = true;
        config.security.proxy_trust.trusted_proxy_ips = vec!["127.0.0.1".to_string()];
        config.security.proxy_trust.trusted_proxy_cidrs = vec!["10.0.0.0/8".to_string()];
        config
    }

    fn headers_with_xff(value: &'static str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static(value));
        headers
    }

    #[test]
    fn every_route_table_entry_registers_without_conflict() {
        // axum panics on a duplicate method+path pair and on conflicting wildcard
        // segments. Catch that here rather than on a live panel start.
        let mut router: Router<AppState> = Router::new();
        for spec in ROUTE_TABLE {
            router = router.route(spec.path, method_router(spec));
        }
        let _ = router;
    }

    #[test]
    fn subscription_request_trace_routes_never_include_bearer_material() {
        assert_eq!(request_trace_route("/sub/client-secret"), "/sub/{redacted}");
        assert_eq!(
            request_trace_route("/sub/device/device-secret"),
            "/sub/{redacted}"
        );
        assert_eq!(request_trace_route("/api/admin/me"), "/api/admin/me");
    }

    #[test]
    fn forwarded_for_is_ignored_when_trust_is_disabled() {
        let mut config = proxy_config();
        config.security.proxy_trust.trust_x_forwarded_for = false;
        let headers = headers_with_xff("198.51.100.10");

        assert_eq!(
            resolve_client_ip(&config, "127.0.0.1".to_string(), &headers),
            "127.0.0.1"
        );
    }

    #[test]
    fn forwarded_for_is_ignored_from_untrusted_direct_peer() {
        let config = proxy_config();
        let headers = headers_with_xff("198.51.100.10");

        assert_eq!(
            resolve_client_ip(&config, "203.0.113.44".to_string(), &headers),
            "203.0.113.44"
        );
    }

    #[test]
    fn forwarded_for_uses_nearest_untrusted_hop_from_right() {
        let config = proxy_config();
        let headers = headers_with_xff("198.51.100.200, 198.51.100.10, 10.1.2.3");

        assert_eq!(
            resolve_client_ip(&config, "127.0.0.1".to_string(), &headers),
            "198.51.100.10"
        );
    }

    #[test]
    fn malformed_forwarded_for_falls_back_to_direct_peer() {
        let config = proxy_config();
        let headers = headers_with_xff("198.51.100.10, not-an-ip");

        assert_eq!(
            resolve_client_ip(&config, "127.0.0.1".to_string(), &headers),
            "127.0.0.1"
        );
    }
}

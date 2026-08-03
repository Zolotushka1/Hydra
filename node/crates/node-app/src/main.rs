use std::{net::SocketAddr, sync::Arc};

use anyhow::{Context, Result, bail};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, header::AUTHORIZATION},
    routing::{get, post},
};
use node_config::NodeConfig;
use node_core::{
    ApplyHistoryEntry, LocalNodeSnapshot, LocalRuntimeAction, LocalSidecarAction,
    LocalSidecarExecutorResultRequest, LocalSidecarExecutorResultResponse, LocalSidecarKind,
    LocalSidecarLifecycleResponse, LocalSidecarStateView, NodeRuntime, RuntimeAlert,
    RuntimeArtifactView, RuntimeEventEntry, RuntimeValidationReport, SidecarExecutorSession,
    SidecarExecutorSessionResultRequest, SidecarExecutorSessionResultResponse, XrayUpdateStatus,
};
use node_domain::{
    CompleteLocalSubscriptionSessionEnforcementRequest, LocalSubscriptionSessionAdapterLeaseView,
    LocalSubscriptionSessionEnforcementCommand, RegisterLocalSubscriptionSessionAdapterRequest,
    ReportSubscriptionSessionsRequest, SubscriptionSessionAdapterView,
};
use tokio::sync::Mutex;
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

type SharedRuntime = Arc<Mutex<NodeRuntime>>;
const MAX_LOCAL_API_BODY_BYTES: usize = 1024 * 1024;

#[derive(Clone)]
struct AppState {
    runtime: SharedRuntime,
    local_api_token: Option<String>,
    subscription_session_adapter_token: Option<String>,
}

#[derive(serde::Serialize)]
struct HealthResponse {
    status: String,
    node_id: Option<String>,
    applied_revision: Option<String>,
    consecutive_tick_failures: u32,
}

#[derive(serde::Serialize)]
struct RuntimeActionResponse {
    detail: String,
}

#[derive(serde::Serialize)]
struct XrayUpdateResponse {
    detail: String,
    status: Option<XrayUpdateStatus>,
    phase: Option<String>,
    target_version: Option<String>,
    source_release: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let config = NodeConfig::default();
    let bind = config.local_api_bind.clone();
    let addr: SocketAddr = bind
        .parse()
        .with_context(|| format!("invalid HYDRA_NODE_LOCAL_API_BIND: {bind}"))?;
    validate_local_api_exposure(addr, config.local_api_token.as_deref())?;
    let local_api_token = config.local_api_token.clone();
    let subscription_session_adapter_token = config.subscription_session_adapter_token.clone();
    let runtime = Arc::new(Mutex::new(NodeRuntime::new(config)?));
    let app_state = AppState {
        runtime: Arc::clone(&runtime),
        local_api_token,
        subscription_session_adapter_token,
    };

    let poll_runtime = Arc::clone(&runtime);
    tokio::spawn(async move {
        run_poll_loop(poll_runtime).await;
    });

    let app = Router::new()
        .route("/health", get(health))
        .route("/state", get(state))
        .route("/runtime/artifacts", get(runtime_artifacts))
        .route("/runtime/validation-report", get(runtime_validation_report))
        .route("/runtime/sidecars", get(runtime_sidecars))
        .route("/runtime/alerts", get(runtime_alerts))
        .route(
            "/runtime/sidecar-executor-session",
            get(sidecar_executor_session),
        )
        .route(
            "/runtime/sidecar-executor-session/result",
            post(sidecar_executor_session_result),
        )
        .route("/runtime/events", get(runtime_events))
        .route("/runtime/apply-history", get(runtime_apply_history))
        .route("/runtime/{action}", post(runtime_action))
        .route("/runtime/sidecars/{sidecar}/{action}", post(sidecar_action))
        .route(
            "/runtime/sidecars/{sidecar}/{action}/result",
            post(sidecar_action_result),
        )
        .route("/runtime/rollback", post(rollback_runtime_config))
        .route(
            "/runtime/subscription-sessions/adapter/register",
            post(register_subscription_session_adapter),
        )
        .route(
            "/runtime/subscription-sessions/observations",
            post(stage_subscription_session_observations),
        )
        .route(
            "/runtime/subscription-sessions/actions",
            get(list_subscription_session_actions),
        )
        .route(
            "/runtime/subscription-sessions/actions/{action_id}/result",
            post(complete_subscription_session_action),
        )
        .route("/xray/update", post(update_xray))
        .with_state(app_state)
        .layer(DefaultBodyLimit::max(MAX_LOCAL_API_BODY_BYTES));

    info!(bind = %addr, "hydra-node rust agent started");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .context("failed to bind local node api")?;
    axum::serve(listener, app)
        .await
        .context("local node api server failed")
}

fn validate_local_api_exposure(addr: SocketAddr, token: Option<&str>) -> Result<()> {
    if addr.ip().is_loopback() {
        return Ok(());
    }
    if token.is_some_and(|value| !value.trim().is_empty()) {
        return Ok(());
    }
    bail!(
        "refusing to bind unauthenticated local API on non-loopback address {addr}; set HYDRA_NODE_LOCAL_API_TOKEN or bind HYDRA_NODE_LOCAL_API_BIND to 127.0.0.1"
    )
}

async fn run_poll_loop(runtime: SharedRuntime) {
    loop {
        let interval = {
            let mut runtime = runtime.lock().await;
            if let Err(error) = runtime.tick().await {
                error!(error = %error, "node tick failed");
            }
            runtime.next_poll_delay()
        };
        tokio::time::sleep(interval).await;
    }
}

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    let snapshot = state.runtime.lock().await.snapshot();
    let status = match snapshot.status {
        node_domain::NodeStatus::Unknown => "unknown",
        node_domain::NodeStatus::Healthy => "healthy",
        node_domain::NodeStatus::Degraded => "degraded",
        node_domain::NodeStatus::Offline => "offline",
    };

    Json(HealthResponse {
        status: status.to_string(),
        node_id: snapshot.node_id,
        applied_revision: snapshot.applied_revision,
        consecutive_tick_failures: snapshot.consecutive_tick_failures,
    })
}

async fn state(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<LocalNodeSnapshot>, axum::http::StatusCode> {
    authorize_local_api(&state, &headers)?;
    Ok(Json(state.runtime.lock().await.snapshot()))
}

async fn runtime_artifacts(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<RuntimeArtifactView>>, axum::http::StatusCode> {
    authorize_local_api(&state, &headers)?;
    Ok(Json(
        state.runtime.lock().await.snapshot().runtime_artifacts,
    ))
}

async fn runtime_validation_report(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<RuntimeValidationReport>, axum::http::StatusCode> {
    authorize_local_api(&state, &headers)?;
    Ok(Json(
        state
            .runtime
            .lock()
            .await
            .snapshot()
            .runtime_validation_report,
    ))
}

async fn runtime_sidecars(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<LocalSidecarStateView>>, axum::http::StatusCode> {
    authorize_local_api(&state, &headers)?;
    Ok(Json(state.runtime.lock().await.snapshot().sidecars))
}

async fn runtime_alerts(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<RuntimeAlert>>, axum::http::StatusCode> {
    authorize_local_api(&state, &headers)?;
    Ok(Json(state.runtime.lock().await.runtime_alerts()))
}

async fn sidecar_executor_session(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SidecarExecutorSession>, axum::http::StatusCode> {
    authorize_local_api(&state, &headers)?;
    Ok(Json(state.runtime.lock().await.sidecar_executor_session()))
}

async fn sidecar_executor_session_result(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<SidecarExecutorSessionResultRequest>,
) -> Result<Json<SidecarExecutorSessionResultResponse>, axum::http::StatusCode> {
    authorize_local_api(&state, &headers)?;
    state
        .runtime
        .lock()
        .await
        .complete_sidecar_executor_session(payload)
        .map(Json)
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)
}

async fn runtime_events(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<RuntimeEventEntry>>, axum::http::StatusCode> {
    authorize_local_api(&state, &headers)?;
    Ok(Json(state.runtime.lock().await.snapshot().runtime_events))
}

async fn runtime_apply_history(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<ApplyHistoryEntry>>, axum::http::StatusCode> {
    authorize_local_api(&state, &headers)?;
    Ok(Json(state.runtime.lock().await.snapshot().apply_history))
}

async fn runtime_action(
    State(state): State<AppState>,
    Path(action): Path<String>,
    headers: HeaderMap,
) -> Result<Json<RuntimeActionResponse>, axum::http::StatusCode> {
    authorize_local_api(&state, &headers)?;
    let parsed = match action.as_str() {
        "validate" => LocalRuntimeAction::Validate,
        "start" => LocalRuntimeAction::Start,
        "stop" => LocalRuntimeAction::Stop,
        "restart" => LocalRuntimeAction::Restart,
        _ => return Err(axum::http::StatusCode::BAD_REQUEST),
    };

    let mut runtime = state.runtime.lock().await;
    let detail = runtime
        .execute_local_runtime_action(parsed)
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(RuntimeActionResponse { detail }))
}

async fn sidecar_action(
    State(state): State<AppState>,
    Path((sidecar, action)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<LocalSidecarLifecycleResponse>, axum::http::StatusCode> {
    authorize_local_api(&state, &headers)?;
    let sidecar = parse_sidecar_kind(&sidecar)?;
    let action = parse_sidecar_action(&action)?;
    state
        .runtime
        .lock()
        .await
        .execute_local_sidecar_action(sidecar, action)
        .map(Json)
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)
}

async fn sidecar_action_result(
    State(state): State<AppState>,
    Path((sidecar, action)): Path<(String, String)>,
    headers: HeaderMap,
    Json(payload): Json<LocalSidecarExecutorResultRequest>,
) -> Result<Json<LocalSidecarExecutorResultResponse>, axum::http::StatusCode> {
    authorize_local_api(&state, &headers)?;
    let sidecar = parse_sidecar_kind(&sidecar)?;
    let action = parse_sidecar_action(&action)?;
    state
        .runtime
        .lock()
        .await
        .complete_local_sidecar_action(sidecar, action, payload)
        .map(Json)
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)
}

async fn update_xray(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<XrayUpdateResponse>, axum::http::StatusCode> {
    authorize_local_api(&state, &headers)?;
    let mut runtime = state.runtime.lock().await;
    let detail = runtime
        .update_xray_core()
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let snapshot = runtime.snapshot();
    Ok(Json(XrayUpdateResponse {
        detail,
        status: snapshot.last_xray_update_status,
        phase: snapshot.last_xray_update_phase,
        target_version: snapshot.last_xray_update_target_version,
        source_release: snapshot.last_xray_update_source_release,
    }))
}

async fn rollback_runtime_config(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<RuntimeActionResponse>, axum::http::StatusCode> {
    authorize_local_api(&state, &headers)?;
    let mut runtime = state.runtime.lock().await;
    let detail = runtime
        .rollback_last_config()
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(RuntimeActionResponse { detail }))
}

async fn stage_subscription_session_observations(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<ReportSubscriptionSessionsRequest>,
) -> Result<Json<SubscriptionSessionAdapterView>, axum::http::StatusCode> {
    authorize_subscription_session_adapter(&state, &headers)?;
    let adapter_instance_id = session_adapter_instance_id(&headers)?;
    let mut runtime = state.runtime.lock().await;
    runtime
        .stage_subscription_session_observations(adapter_instance_id, payload)
        .map(Json)
        .map_err(|_| axum::http::StatusCode::BAD_REQUEST)
}

async fn register_subscription_session_adapter(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<RegisterLocalSubscriptionSessionAdapterRequest>,
) -> Result<Json<LocalSubscriptionSessionAdapterLeaseView>, axum::http::StatusCode> {
    authorize_subscription_session_adapter(&state, &headers)?;
    state
        .runtime
        .lock()
        .await
        .register_subscription_session_adapter(payload)
        .await
        .map(Json)
        .map_err(|_| axum::http::StatusCode::BAD_REQUEST)
}

async fn list_subscription_session_actions(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<LocalSubscriptionSessionEnforcementCommand>>, axum::http::StatusCode> {
    authorize_subscription_session_adapter(&state, &headers)?;
    let adapter_instance_id = session_adapter_instance_id(&headers)?;
    state
        .runtime
        .lock()
        .await
        .pending_subscription_session_enforcements(adapter_instance_id)
        .map(Json)
        .map_err(|_| axum::http::StatusCode::BAD_REQUEST)
}

async fn complete_subscription_session_action(
    State(state): State<AppState>,
    Path(action_id): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<CompleteLocalSubscriptionSessionEnforcementRequest>,
) -> Result<Json<RuntimeActionResponse>, axum::http::StatusCode> {
    authorize_subscription_session_adapter(&state, &headers)?;
    let adapter_instance_id = session_adapter_instance_id(&headers)?;
    let mut runtime = state.runtime.lock().await;
    runtime
        .complete_subscription_session_enforcement(adapter_instance_id, &action_id, payload)
        .await
        .map_err(|_| axum::http::StatusCode::BAD_REQUEST)?;
    Ok(Json(RuntimeActionResponse {
        detail: "subscription session enforcement result reported".to_string(),
    }))
}

fn parse_sidecar_kind(value: &str) -> Result<LocalSidecarKind, axum::http::StatusCode> {
    match value {
        "hysteria2" | "hysteria" => Ok(LocalSidecarKind::Hysteria2),
        "wireguard" | "wg" => Ok(LocalSidecarKind::WireGuard),
        _ => Err(axum::http::StatusCode::BAD_REQUEST),
    }
}

fn parse_sidecar_action(value: &str) -> Result<LocalSidecarAction, axum::http::StatusCode> {
    match value {
        "install" => Ok(LocalSidecarAction::Install),
        "update" => Ok(LocalSidecarAction::Update),
        "validate" => Ok(LocalSidecarAction::Validate),
        "start" => Ok(LocalSidecarAction::Start),
        "stop" => Ok(LocalSidecarAction::Stop),
        "restart" => Ok(LocalSidecarAction::Restart),
        "status" => Ok(LocalSidecarAction::Status),
        "logs" => Ok(LocalSidecarAction::Logs),
        _ => Err(axum::http::StatusCode::BAD_REQUEST),
    }
}

fn authorize_local_api(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(), axum::http::StatusCode> {
    match state.local_api_token.as_deref() {
        Some(expected) if !expected.is_empty() => {
            if local_api_authorized(headers, expected) {
                Ok(())
            } else {
                Err(axum::http::StatusCode::UNAUTHORIZED)
            }
        }
        _ => Ok(()),
    }
}

fn authorize_subscription_session_adapter(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(), axum::http::StatusCode> {
    let Some(expected) = state.subscription_session_adapter_token.as_deref() else {
        return Err(axum::http::StatusCode::SERVICE_UNAVAILABLE);
    };
    let actual = headers
        .get("x-hydra-session-adapter-token")
        .and_then(|value| value.to_str().ok());
    if actual.is_some_and(|actual| constant_time_eq(actual.as_bytes(), expected.as_bytes())) {
        Ok(())
    } else {
        Err(axum::http::StatusCode::UNAUTHORIZED)
    }
}

fn session_adapter_instance_id(headers: &HeaderMap) -> Result<&str, axum::http::StatusCode> {
    headers
        .get("x-hydra-session-adapter-instance")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .ok_or(axum::http::StatusCode::BAD_REQUEST)
}

fn local_api_authorized(headers: &HeaderMap, expected: &str) -> bool {
    let header_token = headers
        .get("x-hydra-local-token")
        .and_then(|value| value.to_str().ok())
        .or_else(|| {
            headers
                .get(AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.strip_prefix("Bearer "))
        });
    header_token.is_some_and(|actual| constant_time_eq(actual.as_bytes(), expected.as_bytes()))
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let max_len = left.len().max(right.len());
    let mut diff = left.len() ^ right.len();
    for index in 0..max_len {
        let left = left.get(index).copied().unwrap_or(0);
        let right = right.get(index).copied().unwrap_or(0);
        diff |= usize::from(left ^ right);
    }
    diff == 0
}

fn init_tracing() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "node_app=info,node_core=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();
}

#[cfg(test)]
mod tests {
    use super::*;
    use node_core::RuntimeArtifactKind;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temp_test_dir(name: &str) -> std::path::PathBuf {
        let base = std::env::temp_dir().join(format!(
            "hydra-node-app-{name}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&base).unwrap();
        base
    }

    fn test_config(dir: &std::path::Path) -> NodeConfig {
        NodeConfig {
            node_token: "token".to_string(),
            local_state_path: dir.join("node-state.json").to_string_lossy().to_string(),
            local_config_path: dir
                .join("generated-config.json")
                .to_string_lossy()
                .to_string(),
            local_runtime_config_path: dir
                .join("node-runtime-config.json")
                .to_string_lossy()
                .to_string(),
            local_sidecar_runtime_config_path: dir
                .join("sidecar-runtime-config.json")
                .to_string_lossy()
                .to_string(),
            local_xray_config_path: dir.join("xray.json").to_string_lossy().to_string(),
            route_credentials_path: dir
                .join("route-credentials.json")
                .to_string_lossy()
                .to_string(),
            route_credentials_dir: dir.join("route-credentials").to_string_lossy().to_string(),
            apply_history_path: dir.join("apply-history.json").to_string_lossy().to_string(),
            runtime_event_history_path: dir
                .join("runtime-events.json")
                .to_string_lossy()
                .to_string(),
            ..NodeConfig::default()
        }
    }

    fn test_app_state(dir: &std::path::Path, local_api_token: Option<&str>) -> AppState {
        AppState {
            runtime: Arc::new(Mutex::new(NodeRuntime::new(test_config(dir)).unwrap())),
            local_api_token: local_api_token.map(str::to_string),
            subscription_session_adapter_token: None,
        }
    }

    #[test]
    fn local_api_auth_accepts_header_token() {
        let mut headers = HeaderMap::new();
        headers.insert("x-hydra-local-token", "secret".parse().unwrap());

        assert!(local_api_authorized(&headers, "secret"));
        assert!(!local_api_authorized(&headers, "other"));
    }

    #[test]
    fn local_api_auth_accepts_bearer_token() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, "Bearer secret".parse().unwrap());

        assert!(local_api_authorized(&headers, "secret"));
    }

    #[test]
    fn local_api_auth_rejects_different_length_token() {
        let mut headers = HeaderMap::new();
        headers.insert("x-hydra-local-token", "secret-extra".parse().unwrap());

        assert!(!local_api_authorized(&headers, "secret"));
    }

    #[test]
    fn local_api_exposure_allows_loopback_without_token() {
        let addr: SocketAddr = "127.0.0.1:8081".parse().unwrap();

        assert!(validate_local_api_exposure(addr, None).is_ok());
    }

    #[test]
    fn local_api_exposure_rejects_non_loopback_without_token() {
        let addr: SocketAddr = "0.0.0.0:8081".parse().unwrap();

        assert!(validate_local_api_exposure(addr, None).is_err());
    }

    #[test]
    fn local_api_exposure_allows_non_loopback_with_token() {
        let addr: SocketAddr = "0.0.0.0:8081".parse().unwrap();

        assert!(validate_local_api_exposure(addr, Some("secret")).is_ok());
    }

    #[test]
    fn session_adapter_endpoint_requires_dedicated_token() {
        let state = AppState {
            runtime: Arc::new(Mutex::new(
                NodeRuntime::new(NodeConfig {
                    node_token: "token".to_string(),
                    ..NodeConfig::default()
                })
                .unwrap(),
            )),
            local_api_token: None,
            subscription_session_adapter_token: Some("adapter-secret".to_string()),
        };
        let mut headers = HeaderMap::new();
        assert_eq!(
            authorize_subscription_session_adapter(&state, &headers),
            Err(axum::http::StatusCode::UNAUTHORIZED)
        );
        headers.insert(
            "x-hydra-session-adapter-token",
            "adapter-secret".parse().unwrap(),
        );
        assert!(authorize_subscription_session_adapter(&state, &headers).is_ok());
    }

    #[test]
    fn sidecar_route_parsers_accept_supported_contract_values() {
        assert_eq!(
            parse_sidecar_kind("hysteria2").unwrap(),
            LocalSidecarKind::Hysteria2
        );
        assert_eq!(
            parse_sidecar_kind("wireguard").unwrap(),
            LocalSidecarKind::WireGuard
        );
        assert_eq!(
            parse_sidecar_action("install").unwrap(),
            LocalSidecarAction::Install
        );
        assert_eq!(
            parse_sidecar_action("logs").unwrap(),
            LocalSidecarAction::Logs
        );
        assert_eq!(
            parse_sidecar_kind("unknown"),
            Err(axum::http::StatusCode::BAD_REQUEST)
        );
        assert_eq!(
            parse_sidecar_action("unknown"),
            Err(axum::http::StatusCode::BAD_REQUEST)
        );
    }

    #[tokio::test]
    async fn runtime_artifacts_endpoint_uses_local_auth_and_returns_manifest() {
        let dir = temp_test_dir("runtime-artifacts-endpoint");
        let state = test_app_state(&dir, Some("secret"));

        let unauthorized = runtime_artifacts(State(state.clone()), HeaderMap::new()).await;
        assert_eq!(
            unauthorized.err(),
            Some(axum::http::StatusCode::UNAUTHORIZED)
        );

        let mut headers = HeaderMap::new();
        headers.insert("x-hydra-local-token", "secret".parse().unwrap());
        let artifacts = runtime_artifacts(State(state), headers).await.unwrap().0;

        assert!(
            artifacts
                .iter()
                .any(|artifact| artifact.kind == RuntimeArtifactKind::XrayConfig
                    && artifact.executable_runtime_input)
        );
        assert!(artifacts.iter().any(|artifact| {
            artifact.kind == RuntimeArtifactKind::RouteCredentialManifest
                && artifact.secret_sensitive
        }));

        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn runtime_validation_report_endpoint_uses_local_auth_and_returns_report() {
        let dir = temp_test_dir("runtime-validation-report-endpoint");
        let state = test_app_state(&dir, Some("secret"));

        let unauthorized = runtime_validation_report(State(state.clone()), HeaderMap::new()).await;
        assert_eq!(
            unauthorized.err(),
            Some(axum::http::StatusCode::UNAUTHORIZED)
        );

        let mut headers = HeaderMap::new();
        headers.insert("x-hydra-local-token", "secret".parse().unwrap());
        let report = runtime_validation_report(State(state), headers)
            .await
            .unwrap()
            .0;

        assert!(!report.ready);
        assert_eq!(report.component_count, 3);
        assert_eq!(report.protocol_count, 3);
        assert!(report.required_protocols.is_empty());
        assert_eq!(report.sidecar_runtime.requirement_count, 0);
        assert!(
            report
                .disabled_reasons
                .iter()
                .any(|reason| reason.contains("Xray"))
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn runtime_sidecars_endpoint_uses_local_auth_and_returns_sidecar_state() {
        let dir = temp_test_dir("runtime-sidecars-endpoint");
        let state = test_app_state(&dir, Some("secret"));

        let unauthorized = runtime_sidecars(State(state.clone()), HeaderMap::new()).await;
        assert_eq!(
            unauthorized.err(),
            Some(axum::http::StatusCode::UNAUTHORIZED)
        );

        let mut headers = HeaderMap::new();
        headers.insert("x-hydra-local-token", "secret".parse().unwrap());
        let sidecars = runtime_sidecars(State(state), headers).await.unwrap().0;

        assert_eq!(sidecars.len(), 2);
        assert!(
            sidecars
                .iter()
                .any(|sidecar| sidecar.sidecar == LocalSidecarKind::Hysteria2)
        );
        assert!(
            sidecars
                .iter()
                .any(|sidecar| sidecar.sidecar == LocalSidecarKind::WireGuard)
        );
        assert!(sidecars.iter().all(|sidecar| !sidecar.supported));

        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn runtime_alerts_endpoint_uses_local_auth_and_returns_active_alerts() {
        let dir = temp_test_dir("runtime-alerts-endpoint");
        let state = test_app_state(&dir, Some("secret"));

        let unauthorized = runtime_alerts(State(state.clone()), HeaderMap::new()).await;
        assert_eq!(
            unauthorized.err(),
            Some(axum::http::StatusCode::UNAUTHORIZED)
        );

        let mut headers = HeaderMap::new();
        headers.insert("x-hydra-local-token", "secret".parse().unwrap());
        let alerts = runtime_alerts(State(state), headers).await.unwrap().0;

        assert!(
            alerts
                .iter()
                .any(|alert| alert.alert_id == "runtime_validation_failed")
        );
        assert!(alerts.iter().all(|alert| alert.active));

        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn sidecar_result_endpoint_uses_local_auth_and_validates_result_contract() {
        let dir = temp_test_dir("sidecar-result-endpoint");
        let state = test_app_state(&dir, Some("secret"));
        let payload = LocalSidecarExecutorResultRequest {
            command_id: "wrong-command".to_string(),
            status: node_core::LocalSidecarStatus::Running,
            completed_checks: Vec::new(),
            exit_code: Some(1),
            detail: Some("bad result".to_string()),
            completed_at_unix: Some(123),
        };

        let unauthorized = sidecar_action_result(
            State(state.clone()),
            Path(("wireguard".to_string(), "restart".to_string())),
            HeaderMap::new(),
            Json(payload.clone()),
        )
        .await;
        assert_eq!(
            unauthorized.err(),
            Some(axum::http::StatusCode::UNAUTHORIZED)
        );

        let mut headers = HeaderMap::new();
        headers.insert("x-hydra-local-token", "secret".parse().unwrap());
        let response = sidecar_action_result(
            State(state),
            Path(("wireguard".to_string(), "restart".to_string())),
            headers,
            Json(payload),
        )
        .await
        .unwrap()
        .0;

        assert!(!response.accepted);
        assert_eq!(response.status, node_core::LocalSidecarStatus::Failed);
        assert!(
            response
                .failed_checks
                .iter()
                .any(|check| check.contains("command_id mismatch"))
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn sidecar_executor_session_endpoints_use_local_auth() {
        let dir = temp_test_dir("sidecar-session-endpoints");
        let state = test_app_state(&dir, Some("secret"));

        let unauthorized = sidecar_executor_session(State(state.clone()), HeaderMap::new()).await;
        assert_eq!(
            unauthorized.err(),
            Some(axum::http::StatusCode::UNAUTHORIZED)
        );

        let mut headers = HeaderMap::new();
        headers.insert("x-hydra-local-token", "secret".parse().unwrap());
        let session = sidecar_executor_session(State(state.clone()), headers.clone())
            .await
            .unwrap()
            .0;
        assert_eq!(session.envelope_count, 0);
        assert!(session.fail_closed);

        let unauthorized_result = sidecar_executor_session_result(
            State(state.clone()),
            HeaderMap::new(),
            Json(SidecarExecutorSessionResultRequest {
                session_id: session.session_id.clone(),
                results: Vec::new(),
            }),
        )
        .await;
        assert_eq!(
            unauthorized_result.err(),
            Some(axum::http::StatusCode::UNAUTHORIZED)
        );

        let response = sidecar_executor_session_result(
            State(state),
            headers,
            Json(SidecarExecutorSessionResultRequest {
                session_id: session.session_id,
                results: Vec::new(),
            }),
        )
        .await
        .unwrap()
        .0;
        assert!(response.accepted);
        assert_eq!(response.expected_envelope_count, 0);

        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn runtime_history_endpoints_use_local_auth_and_return_bounded_lists() {
        let dir = temp_test_dir("runtime-history-endpoints");
        let state = test_app_state(&dir, Some("secret"));

        assert_eq!(
            runtime_events(State(state.clone()), HeaderMap::new())
                .await
                .err(),
            Some(axum::http::StatusCode::UNAUTHORIZED)
        );
        assert_eq!(
            runtime_apply_history(State(state.clone()), HeaderMap::new())
                .await
                .err(),
            Some(axum::http::StatusCode::UNAUTHORIZED)
        );

        let mut headers = HeaderMap::new();
        headers.insert("x-hydra-local-token", "secret".parse().unwrap());
        let events = runtime_events(State(state.clone()), headers.clone())
            .await
            .unwrap()
            .0;
        let apply_history = runtime_apply_history(State(state), headers)
            .await
            .unwrap()
            .0;

        assert!(events.is_empty());
        assert!(apply_history.is_empty());

        let _ = fs::remove_dir_all(dir);
    }
}

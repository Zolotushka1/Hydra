use anyhow::{Context, Result, bail};
use node_domain::{
    CompleteLocalSubscriptionSessionEnforcementRequest, LocalSubscriptionSessionAdapterLeaseView,
    LocalSubscriptionSessionEnforcementCommand, RegisterLocalSubscriptionSessionAdapterRequest,
    ReportSubscriptionSessionsRequest, SubscriptionSessionAdapterView,
};
use reqwest::header::{HeaderMap, HeaderValue};

const SESSION_ADAPTER_TOKEN_HEADER: &str = "x-hydra-session-adapter-token";
const SESSION_ADAPTER_INSTANCE_HEADER: &str = "x-hydra-session-adapter-instance";

#[derive(Debug, Clone)]
pub struct SessionAdapterClientConfig {
    pub node_local_api_url: String,
    pub adapter_token: String,
    pub adapter_instance_id: String,
}

#[derive(Debug, Clone)]
pub struct SessionAdapterClient {
    base_url: String,
    adapter_instance_id: String,
    http: reqwest::Client,
}

impl SessionAdapterClient {
    pub fn new(config: SessionAdapterClientConfig) -> Result<Self> {
        if config.node_local_api_url.trim().is_empty() {
            bail!("node local API URL is required");
        }
        if config.adapter_token.trim().is_empty() {
            bail!("session adapter token is required");
        }
        if config.adapter_instance_id.trim().is_empty() || config.adapter_instance_id.len() > 128 {
            bail!("session adapter instance id must be between 1 and 128 characters");
        }

        let mut headers = HeaderMap::new();
        headers.insert(
            SESSION_ADAPTER_TOKEN_HEADER,
            HeaderValue::from_str(&config.adapter_token)
                .context("invalid session adapter token header")?,
        );
        headers.insert(
            SESSION_ADAPTER_INSTANCE_HEADER,
            HeaderValue::from_str(&config.adapter_instance_id)
                .context("invalid session adapter instance header")?,
        );
        let http = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .context("failed to build session adapter HTTP client")?;

        Ok(Self {
            base_url: config.node_local_api_url.trim_end_matches('/').to_string(),
            adapter_instance_id: config.adapter_instance_id,
            http,
        })
    }

    pub fn adapter_instance_id(&self) -> &str {
        &self.adapter_instance_id
    }

    pub async fn register(
        &self,
        mut request: RegisterLocalSubscriptionSessionAdapterRequest,
    ) -> Result<LocalSubscriptionSessionAdapterLeaseView> {
        normalize_registration_instance_id(&mut request, &self.adapter_instance_id)?;
        self.http
            .post(format!(
                "{}/runtime/subscription-sessions/adapter/register",
                self.base_url
            ))
            .json(&request)
            .send()
            .await
            .context("session adapter register request failed")?
            .error_for_status()
            .context("session adapter register returned error status")?
            .json::<LocalSubscriptionSessionAdapterLeaseView>()
            .await
            .context("failed to decode session adapter lease response")
    }

    pub async fn submit_observations(
        &self,
        request: ReportSubscriptionSessionsRequest,
    ) -> Result<SubscriptionSessionAdapterView> {
        self.http
            .post(format!(
                "{}/runtime/subscription-sessions/observations",
                self.base_url
            ))
            .json(&request)
            .send()
            .await
            .context("session observation submit request failed")?
            .error_for_status()
            .context("session observation submit returned error status")?
            .json::<SubscriptionSessionAdapterView>()
            .await
            .context("failed to decode session adapter view")
    }

    pub async fn pending_actions(&self) -> Result<Vec<LocalSubscriptionSessionEnforcementCommand>> {
        self.http
            .get(format!(
                "{}/runtime/subscription-sessions/actions",
                self.base_url
            ))
            .send()
            .await
            .context("session actions poll request failed")?
            .error_for_status()
            .context("session actions poll returned error status")?
            .json::<Vec<LocalSubscriptionSessionEnforcementCommand>>()
            .await
            .context("failed to decode session enforcement actions")
    }

    pub async fn complete_action(
        &self,
        action_id: &str,
        request: CompleteLocalSubscriptionSessionEnforcementRequest,
    ) -> Result<()> {
        if action_id.trim().is_empty() {
            bail!("session action id is required");
        }
        self.http
            .post(format!(
                "{}/runtime/subscription-sessions/actions/{}/result",
                self.base_url, action_id
            ))
            .json(&request)
            .send()
            .await
            .context("session action result request failed")?
            .error_for_status()
            .context("session action result returned error status")?;
        Ok(())
    }
}

fn normalize_registration_instance_id(
    request: &mut RegisterLocalSubscriptionSessionAdapterRequest,
    adapter_instance_id: &str,
) -> Result<()> {
    if request.adapter_instance_id.trim().is_empty() {
        request.adapter_instance_id = adapter_instance_id.to_string();
    }
    if request.adapter_instance_id != adapter_instance_id {
        bail!("registration instance id must match client instance id");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Json, Router,
        extract::State,
        http::{HeaderMap, StatusCode},
        routing::{get, post},
    };
    use node_domain::{
        LocalSubscriptionSessionEnforcementCommand, RegisterLocalSubscriptionSessionAdapterRequest,
        SubscriptionSessionAdapterStatus, SubscriptionSessionAdapterView,
        SubscriptionSessionEnforcementAction, SubscriptionSessionObservationSource,
        SubscriptionSessionRuntimeCapability,
    };
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[derive(Clone)]
    struct TestState {
        seen_headers: Arc<Mutex<Vec<(String, String)>>>,
    }

    async fn register(
        State(state): State<TestState>,
        headers: HeaderMap,
        Json(payload): Json<RegisterLocalSubscriptionSessionAdapterRequest>,
    ) -> Result<Json<LocalSubscriptionSessionAdapterLeaseView>, StatusCode> {
        remember_headers(state, &headers).await;
        Ok(Json(LocalSubscriptionSessionAdapterLeaseView {
            adapter_instance_id: payload.adapter_instance_id,
            runtime_capabilities: payload.runtime_capabilities,
            registered_at_unix: 1,
            lease_expires_at_unix: 91,
        }))
    }

    async fn observations(
        State(state): State<TestState>,
        headers: HeaderMap,
    ) -> Result<Json<SubscriptionSessionAdapterView>, StatusCode> {
        remember_headers(state, &headers).await;
        Ok(Json(SubscriptionSessionAdapterView {
            status: SubscriptionSessionAdapterStatus::ObservationOnly,
            observation_source: Some(SubscriptionSessionObservationSource::NodeManagedRuntimeTable),
            runtime_capabilities: Vec::new(),
            exact_session_termination_ready: false,
            disabled_reason: None,
            buffered_observation_count: 0,
            last_observation_at_unix: None,
            last_report_at_unix: None,
            last_reported_count: 0,
            last_blocked_count: 0,
            pending_enforcement_count: 0,
            adapter_registered: true,
            active_lease_expires_at_unix: Some(91),
        }))
    }

    async fn actions(
        State(state): State<TestState>,
        headers: HeaderMap,
    ) -> Json<Vec<LocalSubscriptionSessionEnforcementCommand>> {
        remember_headers(state, &headers).await;
        Json(vec![LocalSubscriptionSessionEnforcementCommand {
            action_id: "action-a".to_string(),
            session_id: "session-a".to_string(),
            action: SubscriptionSessionEnforcementAction::TerminateSession,
            runtime_session_ref: "opaque-ref-a".to_string(),
            reason: "test".to_string(),
            requires_absence_verification: true,
            issued_at_unix: 1,
            expires_at_unix: 31,
        }])
    }

    async fn complete(State(state): State<TestState>, headers: HeaderMap) -> StatusCode {
        remember_headers(state, &headers).await;
        StatusCode::OK
    }

    async fn remember_headers(state: TestState, headers: &HeaderMap) {
        let token = headers
            .get(SESSION_ADAPTER_TOKEN_HEADER)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let instance = headers
            .get(SESSION_ADAPTER_INSTANCE_HEADER)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        state.seen_headers.lock().await.push((token, instance));
    }

    async fn spawn_server() -> (String, Arc<Mutex<Vec<(String, String)>>>) {
        let seen_headers = Arc::new(Mutex::new(Vec::new()));
        let state = TestState {
            seen_headers: seen_headers.clone(),
        };
        let app = Router::new()
            .route(
                "/runtime/subscription-sessions/adapter/register",
                post(register),
            )
            .route(
                "/runtime/subscription-sessions/observations",
                post(observations),
            )
            .route("/runtime/subscription-sessions/actions", get(actions))
            .route(
                "/runtime/subscription-sessions/actions/{action_id}/result",
                post(complete),
            )
            .with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (url, seen_headers)
    }

    #[tokio::test]
    async fn client_attaches_token_and_instance_to_every_request() {
        let (url, seen_headers) = spawn_server().await;
        let client = SessionAdapterClient::new(SessionAdapterClientConfig {
            node_local_api_url: url,
            adapter_token: "token-a".to_string(),
            adapter_instance_id: "adapter-a".to_string(),
        })
        .unwrap();

        let lease = client
            .register(RegisterLocalSubscriptionSessionAdapterRequest {
                adapter_instance_id: String::new(),
                runtime_capabilities: vec![
                    SubscriptionSessionRuntimeCapability::OpaqueSessionReference,
                    SubscriptionSessionRuntimeCapability::ExactSessionTermination,
                    SubscriptionSessionRuntimeCapability::PostActionAbsenceVerification,
                ],
            })
            .await
            .unwrap();
        assert_eq!(lease.adapter_instance_id, "adapter-a");

        client
            .submit_observations(node_domain::ReportSubscriptionSessionsRequest {
                observation_source: SubscriptionSessionObservationSource::NodeManagedRuntimeTable,
                runtime_capabilities: Vec::new(),
                observations: Vec::new(),
            })
            .await
            .unwrap();
        let actions = client.pending_actions().await.unwrap();
        assert_eq!(actions[0].expires_at_unix, 31);
        client
            .complete_action(
                "action-a",
                CompleteLocalSubscriptionSessionEnforcementRequest {
                    status: node_domain::SubscriptionSessionEnforcementStatus::Failed,
                    runtime_session_ref: None,
                    session_absent_after_action: None,
                    verified_at_unix: None,
                    detail: Some("test".to_string()),
                },
            )
            .await
            .unwrap();

        let headers = seen_headers.lock().await.clone();
        assert_eq!(headers.len(), 4);
        assert!(headers.iter().all(|(token, _)| token == "token-a"));
        assert!(headers.iter().all(|(_, instance)| instance == "adapter-a"));
    }

    #[test]
    fn client_rejects_mismatched_registration_instance() {
        let client = SessionAdapterClient::new(SessionAdapterClientConfig {
            node_local_api_url: "http://127.0.0.1:1".to_string(),
            adapter_token: "token-a".to_string(),
            adapter_instance_id: "adapter-a".to_string(),
        })
        .unwrap();
        let request = RegisterLocalSubscriptionSessionAdapterRequest {
            adapter_instance_id: "adapter-b".to_string(),
            runtime_capabilities: Vec::new(),
        };
        let mut request = request;
        let result = normalize_registration_instance_id(&mut request, client.adapter_instance_id());
        assert!(result.is_err());
    }
}

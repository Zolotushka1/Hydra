//! HTTP access to the panel.
//!
//! Two calls, because the slice needs two. Adding the other 190 before a screen
//! renders would be a client nobody has run.
//!
//! Paths are taken from `ROUTE_TABLE` by `RouteId`, not written as string
//! literals: the table is the same constant the axum router is built from, so a
//! renamed path breaks this at compile time instead of at runtime. That is the
//! whole reason the table moved into `panel-domain`.

use panel_domain::routes::{ROUTE_TABLE, RouteId};
use panel_domain::security::{LoginRequest, LoginSuccess};
use panel_domain::ui::UiBootstrapSnapshot;

/// The path a route is served on, resolved from the shared table.
///
/// Panics rather than returning an error: the table is a compile-time constant
/// with an exhaustive `RouteId`, so a missing row means the binary is built
/// against a table that cannot exist. There is no runtime recovery from that and
/// nothing useful to report to the operator.
fn path_of(id: RouteId) -> &'static str {
    ROUTE_TABLE
        .iter()
        .find(|spec| spec.id == id)
        .map(|spec| spec.path)
        .expect("every RouteId has exactly one ROUTE_TABLE row")
}

#[derive(Debug, Clone)]
pub enum ApiError {
    /// The request never reached the panel.
    Network(String),
    /// The panel answered, but not with success. Carries the status so the
    /// caller can distinguish "wrong password" from "session expired".
    Status(u16, String),
    /// The panel answered with a body this build cannot parse. Usually a
    /// version skew between the frontend and the server.
    Decode(String),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Network(detail) => write!(formatter, "the panel is unreachable: {detail}"),
            Self::Status(status, detail) if detail.is_empty() => {
                write!(formatter, "the panel refused the request ({status})")
            }
            Self::Status(status, detail) => write!(formatter, "{detail} ({status})"),
            Self::Decode(detail) => write!(
                formatter,
                "the panel answered in a shape this build does not understand, \
                 which usually means the panel and the frontend are different \
                 versions: {detail}"
            ),
        }
    }
}

async fn read_json<T: serde::de::DeserializeOwned>(
    response: gloo_net::http::Response,
) -> Result<T, ApiError> {
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| ApiError::Network(error.to_string()))?;

    if !(200..300).contains(&status) {
        // The panel reports failures as JSON with a `reason`, but an error from
        // a proxy in front of it will not, so the raw body is the fallback.
        let detail = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|value| {
                value
                    .get("reason")
                    .or_else(|| value.get("error"))
                    .and_then(|reason| reason.as_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| body.chars().take(200).collect());
        return Err(ApiError::Status(status, detail));
    }

    serde_json::from_str(&body).map_err(|error| ApiError::Decode(error.to_string()))
}

pub async fn login(username: String, password: String) -> Result<LoginSuccess, ApiError> {
    let payload = LoginRequest {
        username,
        password,
        two_factor_code: None,
        challenge_token: None,
    };

    let response = gloo_net::http::Request::post(path_of(RouteId::Login))
        .json(&payload)
        .map_err(|error| ApiError::Decode(error.to_string()))?
        .send()
        .await
        .map_err(|error| ApiError::Network(error.to_string()))?;

    read_json(response).await
}

pub async fn bootstrap(token: &str) -> Result<UiBootstrapSnapshot, ApiError> {
    let response = gloo_net::http::Request::get(path_of(RouteId::GetUiBootstrap))
        .header("Authorization", &format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| ApiError::Network(error.to_string()))?;

    read_json(response).await
}

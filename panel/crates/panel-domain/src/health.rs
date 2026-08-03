use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct HealthStatus {
    pub service: &'static str,
    pub status: &'static str,
}

impl HealthStatus {
    pub fn ok(service: &'static str) -> Self {
        Self {
            service,
            status: "ok",
        }
    }
}

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BanKind {
    Temporary,
    Permanent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BanSource {
    Automatic,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecuritySettings {
    pub login_protection_enabled: bool,
    pub smart_ban_enabled: bool,
    pub trust_x_forwarded_for: bool,
    pub trusted_proxy_ips: Vec<String>,
    pub trusted_proxy_cidrs: Vec<String>,
    pub max_failed_attempts: usize,
    pub attempt_window_seconds: u64,
    pub block_for_seconds: u64,
    pub session_ttl_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TwoFactorState {
    pub enabled: bool,
    pub two_step_enabled: bool,
    pub configured: bool,
    pub confirmed_at_unix: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TwoFactorSetupResponse {
    pub secret_base32: String,
    pub otpauth_url: String,
    pub state: TwoFactorState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnableTwoFactorRequest {
    pub code: String,
    pub two_step_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisableTwoFactorRequest {
    pub code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateTwoFactorTwoStepRequest {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSecuritySettingsRequest {
    pub login_protection_enabled: bool,
    pub smart_ban_enabled: bool,
    pub trust_x_forwarded_for: bool,
    pub trusted_proxy_ips: Vec<String>,
    pub trusted_proxy_cidrs: Vec<String>,
    pub max_failed_attempts: usize,
    pub attempt_window_seconds: u64,
    pub block_for_seconds: u64,
    pub session_ttl_seconds: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityPreset {
    Standard,
    Strict,
    Paranoid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplySecurityPresetRequest {
    pub preset: SecurityPreset,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveBanView {
    pub client_ip: String,
    pub ban_kind: BanKind,
    pub source: BanSource,
    pub reason: String,
    pub created_at_unix: u64,
    pub blocked_until_unix: u64,
    pub ban_level: usize,
    #[serde(default)]
    pub remaining_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBanRequest {
    pub client_ip: String,
    pub ban_kind: BanKind,
    pub duration_seconds: Option<u64>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedAdmin {
    pub username: String,
    pub password_hash: String,
    pub created_at_unix: u64,
    #[serde(default)]
    pub two_factor_secret_base32: Option<String>,
    #[serde(default)]
    pub two_factor_secret_ciphertext_b64: Option<String>,
    #[serde(default)]
    pub two_factor_secret_nonce_b64: Option<String>,
    pub two_factor_enabled: bool,
    pub two_factor_two_step_enabled: bool,
    pub two_factor_confirmed_at_unix: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuditEventType {
    LoginSucceeded,
    LoginFailed,
    LogoutSucceeded,
    SecuritySettingsUpdated,
    SecuritySettingsUpdateFailed,
    SecurityPresetApplied,
    TwoFactorSecretRegenerated,
    TwoFactorEnabled,
    TwoFactorDisabled,
    TwoFactorTwoStepUpdated,
    TelegramSettingsUpdated,
    TelegramTestMessageRequested,
    TelegramRetryDueRequested,
    AdminSessionRevoked,
    SystemThresholdsUpdated,
    BanCreated,
    BanRemoved,
    UserCreated,
    UserUpdated,
    UserDeleted,
    UserUsageReset,
    UserUsageReported,
    UserSubscriptionRevoked,
    SubscriptionDeviceRegistered,
    SubscriptionDeviceRevoked,
    SubscriptionDeviceEnrollmentCreated,
    SubscriptionDeviceEnrollmentRevoked,
    SubscriptionDeviceEnrollmentConsumed,
    SubscriptionSessionReported,
    SubscriptionSessionEnforcementReported,
    SubscriptionPlanCreated,
    SubscriptionPlanUpdated,
    SubscriptionPlanDeleted,
    SubscriptionClientCreated,
    SubscriptionClientUpdated,
    SubscriptionClientAccessUpdated,
    SubscriptionClientUsageReset,
    SubscriptionClientUsageReported,
    SubscriptionClientRevoked,
    SubscriptionClientDeleted,
    UserTemplateCreated,
    UserTemplateUpdated,
    UserTemplateDeleted,
    InboundCreated,
    InboundUpdated,
    InboundDeleted,
    HostCreated,
    HostUpdated,
    HostDeleted,
    ProxyProfileCreated,
    ProxyProfileUpdated,
    ProxyProfileDeleted,
    NodeCreated,
    NodeUpdated,
    NodeDeleted,
    NodeAuthTokenRotated,
    NodeLocalApiTokenUpdated,
    NodeLocalRuntimeActionRequested,
    NodeLocalXrayUpdateRequested,
    NodeBootstrapProbeRequested,
    NodeProvisioningStarted,
    NodeProvisioningUpdated,
    ProvisioningExecutorHandshaked,
    ProvisioningExecutorTrustUpdated,
    ProvisioningExecutorTokenRotated,
    NodeProvisioningFinished,
    NodeApplyRequested,
    NodeApplyRetryRequested,
    NodeRollbackRequested,
    NodeHeartbeatReceived,
    NodeSyncUpdated,
    PanelInstallerJobCreated,
    RouteCredentialRotated,
    RouteCredentialRevoked,
    RouteCaRotated,
    CoreConfigSaved,
    CoreApplyRequested,
    CoreActionRequested,
    CoreStartRequested,
    CoreStopRequested,
    CoreRestartRequested,
    CoreXrayUpdateRequested,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub event_type: AuditEventType,
    pub username: Option<String>,
    pub client_ip: Option<String>,
    pub created_at_unix: u64,
    pub detail: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AuditEventsQuery {
    pub event_type: Option<AuditEventType>,
    pub username: Option<String>,
    pub client_ip: Option<String>,
    pub search: Option<String>,
    pub created_from_unix: Option<u64>,
    pub created_to_unix: Option<u64>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionView {
    pub session_id: String,
    pub username: String,
    pub issued_at_unix: u64,
    pub expires_at_unix: u64,
    pub client_ip: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticatedAdmin {
    pub username: String,
    pub session: SessionView,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginSuccess {
    pub token: String,
    pub admin: AuthenticatedAdmin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginFailure {
    pub reason: &'static str,
    pub blocked_until_unix: Option<u64>,
    pub ban_kind: Option<BanKind>,
    pub challenge_token: Option<String>,
    pub wait_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginProtectionStatus {
    pub failed_attempts_in_window: usize,
    pub blocked_until_unix: Option<u64>,
    pub ban_kind: Option<BanKind>,
    pub ban_level: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretClass {
    Public,
    OperatorInternal,
    Sensitive,
    HighSensitivitySecret,
}

#[derive(Debug, Error)]
pub enum SecurityError {
    #[error("secret persistence requires explicit policy")]
    MissingPersistencePolicy,
    #[error("bootstrap admin is not configured")]
    AdminNotConfigured,
    #[error("invalid credentials")]
    InvalidCredentials,
    #[error("client ip is banned")]
    IpBanned,
    #[error("unauthorized")]
    Unauthorized,
    #[error("session limit reached")]
    SessionLimitReached,
    #[error("invalid security settings: {0}")]
    InvalidSecuritySettings(&'static str),
    #[error("failed to persist security settings")]
    PersistenceFailure,
    #[error("invalid two-factor code")]
    InvalidTwoFactorCode,
    #[error("two-factor is not configured")]
    TwoFactorNotConfigured,
    #[error("two-factor is not enabled")]
    TwoFactorNotEnabled,
}

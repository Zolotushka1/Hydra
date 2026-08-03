use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PanelAccessMode {
    DomainTls,
    IpHttp,
    IpSelfSignedTls,
    ReverseProxy,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PanelSecurityPosture {
    Recommended,
    LimitedWithoutDomain,
    DangerPlainHttpPublic,
    CustomReverseProxy,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PanelCertificateSource {
    LetsEncrypt,
    SelfSigned,
    OperatorManaged,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelAccessModeOption {
    pub mode: PanelAccessMode,
    pub label: String,
    pub recommended: bool,
    pub requires_domain: bool,
    pub tls_required: bool,
    pub description: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelAccessModesView {
    pub schema_version: u16,
    pub options: Vec<PanelAccessModeOption>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelInstallPlanRequest {
    pub access_mode: PanelAccessMode,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub public_ip: Option<String>,
    #[serde(default)]
    pub bind_host: Option<String>,
    #[serde(default)]
    pub bind_port: Option<u16>,
    #[serde(default)]
    pub acme_email: Option<String>,
    #[serde(default)]
    pub firewall_allowlist: Vec<String>,
    #[serde(default)]
    pub trusted_proxy_cidrs: Vec<String>,
    #[serde(default)]
    pub confirm_public_http: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelInstallPlanView {
    pub schema_version: u16,
    pub access_mode: PanelAccessMode,
    pub security_posture: PanelSecurityPosture,
    pub public_url: String,
    pub bind_address: String,
    pub requires_confirmation: bool,
    pub required_confirmations: Vec<String>,
    pub warnings: Vec<String>,
    pub hardening_defaults: Vec<String>,
    #[serde(default)]
    pub firewall_allowlist: Vec<String>,
    pub certificate_plan: PanelCertificatePlanView,
    pub reverse_proxy_plan: PanelReverseProxyPlanView,
    pub steps: Vec<PanelInstallPlanStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelCertificatePlanView {
    pub tls_enabled: bool,
    pub source: PanelCertificateSource,
    pub domain: Option<String>,
    #[serde(default)]
    pub acme_email: Option<String>,
    #[serde(default)]
    pub subject_alt_name: Option<String>,
    pub certificate_path: Option<String>,
    pub private_key_path: Option<String>,
    pub fingerprint_required: bool,
    pub renewal_required: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelReverseProxyPlanView {
    pub enabled: bool,
    pub trust_x_forwarded_for: bool,
    pub trusted_proxy_cidrs: Vec<String>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelInstallPlanStep {
    pub order: u32,
    pub id: String,
    pub title: String,
    pub detail: String,
    pub destructive: bool,
    pub requires_confirmation: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PanelInstallerExecutorPayloadKind {
    PreflightProbe,
    DependencyInstall,
    InstallBinary,
    CertificateOperation,
    ListenerConfig,
    FirewallConfig,
    SecurityDefaults,
    ServiceInstall,
    HealthCheck,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PanelInstallerExecutorResultStatus {
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PanelInstallerJobStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Rejected,
    Expired,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PanelInstallerTargetOs {
    Linux,
    Windows,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PanelInstallerPackageChannel {
    Stable,
    Latest,
    Pinned,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PanelInstallerTargetArch {
    X86_64,
    Aarch64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PanelInstallerArtifactKind {
    #[default]
    InstallerScript,
    PanelBinary,
    NodeBinary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelInstallerBootstrapRequest {
    pub plan: PanelInstallPlanRequest,
    pub target_os: PanelInstallerTargetOs,
    #[serde(default)]
    pub target_arch: Option<PanelInstallerTargetArch>,
    #[serde(default)]
    pub package_channel: Option<PanelInstallerPackageChannel>,
    #[serde(default)]
    pub pinned_version: Option<String>,
    #[serde(default)]
    pub installer_script_url: Option<String>,
    #[serde(default)]
    pub artifact_verification: Option<PanelInstallerArtifactVerificationRequest>,
    #[serde(default)]
    pub release_manifest: Option<PanelInstallerReleaseManifestRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelInstallerCreateJobRequest {
    pub bootstrap: PanelInstallerBootstrapRequest,
    #[serde(default)]
    pub executor_contract_version: Option<u16>,
    #[serde(default)]
    pub ttl_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelInstallerCreateJobResponse {
    pub job: PanelInstallerJobView,
    pub executor_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelInstallerJobView {
    pub job_id: String,
    pub status: PanelInstallerJobStatus,
    pub created_at_unix: u64,
    pub updated_at_unix: u64,
    pub expires_at_unix: u64,
    pub bootstrap: PanelInstallerBootstrapView,
    pub session: PanelInstallerExecutorSessionView,
    pub last_heartbeat: Option<PanelInstallerJobHeartbeatView>,
    pub result: Option<PanelInstallerResultView>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelInstallerJobHeartbeatRequest {
    pub job_id: String,
    pub executor_token: String,
    #[serde(default)]
    pub observed_phase: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelInstallerJobHeartbeatView {
    pub observed_phase: Option<String>,
    pub message: Option<String>,
    pub reported_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelInstallerJobResultRequest {
    pub job_id: String,
    pub executor_token: String,
    pub command_results: Vec<PanelInstallerCommandResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelInstallerJobAccessRequest {
    pub job_id: String,
    pub executor_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelInstallerBootstrapView {
    pub schema_version: u16,
    pub target_os: PanelInstallerTargetOs,
    pub target_arch: PanelInstallerTargetArch,
    pub package_channel: PanelInstallerPackageChannel,
    pub selected_artifact: Option<PanelInstallerSelectedArtifactView>,
    #[serde(default)]
    pub selected_payload_artifact: Option<PanelInstallerSelectedArtifactView>,
    pub ready_to_run: bool,
    pub missing_inputs: Vec<String>,
    pub plan: PanelInstallPlanView,
    pub artifact_verification: PanelInstallerArtifactVerificationView,
    pub supported_platforms: Vec<PanelInstallerSupportedPlatformView>,
    pub environment: Vec<PanelInstallerEnvVarView>,
    pub command_snippets: Vec<PanelInstallerCommandSnippetView>,
    pub warnings: Vec<String>,
    pub security_notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelInstallerArtifactVerificationRequest {
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub signature_url: Option<String>,
    #[serde(default)]
    pub signing_key_fingerprint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelInstallerReleaseManifestRequest {
    pub manifest_version: u16,
    pub artifacts: Vec<PanelInstallerReleaseArtifactRequest>,
    #[serde(default)]
    pub signature_url: Option<String>,
    #[serde(default)]
    pub signing_key_fingerprint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelInstallerReleaseArtifactRequest {
    pub name: String,
    #[serde(default)]
    pub artifact_kind: PanelInstallerArtifactKind,
    pub target_os: PanelInstallerTargetOs,
    pub target_arch: PanelInstallerTargetArch,
    pub package_channel: PanelInstallerPackageChannel,
    pub version: String,
    pub url: String,
    pub sha256: String,
    #[serde(default)]
    pub signature_url: Option<String>,
    #[serde(default)]
    pub signing_key_fingerprint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelInstallerSelectedArtifactView {
    pub source: String,
    pub name: String,
    pub artifact_kind: PanelInstallerArtifactKind,
    pub target_os: PanelInstallerTargetOs,
    pub target_arch: PanelInstallerTargetArch,
    pub package_channel: PanelInstallerPackageChannel,
    pub version: String,
    pub url: String,
    pub sha256: String,
    pub signature_url: Option<String>,
    pub signing_key_fingerprint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelInstallerArtifactVerificationView {
    pub required: bool,
    pub ready: bool,
    pub sha256: Option<String>,
    pub signature_url: Option<String>,
    pub signing_key_fingerprint: Option<String>,
    pub verifier_env: Vec<String>,
    pub fail_closed_checks: Vec<String>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelInstallerSupportedPlatformView {
    pub family: String,
    pub versions: Vec<String>,
    pub package_managers: Vec<String>,
    pub service_manager: String,
    pub supported: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelInstallerEnvVarView {
    pub name: String,
    pub value: String,
    pub required: bool,
    pub secret: bool,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelInstallerCommandSnippetView {
    pub shell: String,
    pub command: String,
    pub requires_admin: bool,
    pub secret_free: bool,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelInstallerSessionRequest {
    pub plan: PanelInstallPlanRequest,
    #[serde(default)]
    pub target_os: Option<PanelInstallerTargetOs>,
    #[serde(default)]
    pub target_arch: Option<PanelInstallerTargetArch>,
    #[serde(default)]
    pub package_channel: Option<PanelInstallerPackageChannel>,
    #[serde(default)]
    pub selected_artifact: Option<PanelInstallerSelectedArtifactView>,
    #[serde(default)]
    pub executor_contract_version: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelInstallerFirstHostSessionRequest {
    pub plan: PanelInstallPlanRequest,
    pub panel_binary: PanelInstallerReleaseArtifactRequest,
    #[serde(default)]
    pub executor_contract_version: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelInstallerExecutorSessionView {
    pub schema_version: u16,
    pub session_id: String,
    pub minimum_executor_contract_version: u16,
    pub supported_executor_contract_version: u16,
    pub compatible: bool,
    pub target_os: PanelInstallerTargetOs,
    pub target_arch: PanelInstallerTargetArch,
    pub package_channel: PanelInstallerPackageChannel,
    pub selected_artifact: Option<PanelInstallerSelectedArtifactView>,
    pub plan: PanelInstallPlanView,
    pub command_envelopes: Vec<PanelInstallerCommandEnvelopeView>,
    pub loop_contract: PanelInstallerLoopContractView,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelInstallerCommandEnvelopeView {
    pub command_id: String,
    pub order: u32,
    pub step_id: String,
    pub payload_kind: PanelInstallerExecutorPayloadKind,
    pub title: String,
    pub detail: String,
    pub destructive: bool,
    pub requires_confirmation: bool,
    pub executor_should_submit: bool,
    pub operations: Vec<PanelInstallerExecutorOperationView>,
    pub acceptance: PanelInstallerAcceptanceContractView,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PanelInstallerExecutorOperationKind {
    PreflightProbe,
    InstallPackageDependency,
    CreateDirectory,
    DownloadArtifact,
    VerifySha256,
    InstallBinary,
    WriteConfig,
    IssueLetsEncryptCertificate,
    GenerateSelfSignedCertificate,
    ApplyFirewall,
    ApplySecurityDefaults,
    WriteService,
    StartService,
    HealthCheck,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelInstallerExecutorOperationView {
    pub operation_id: String,
    pub kind: PanelInstallerExecutorOperationKind,
    #[serde(default)]
    pub program: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub target_path: Option<String>,
    #[serde(default)]
    pub content_template: Option<String>,
    pub requires_admin: bool,
    pub secret_free: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelInstallerAcceptanceContractView {
    pub accepted_result_endpoint: String,
    pub required_success_fields: Vec<String>,
    pub fail_closed_checks: Vec<String>,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelInstallerLoopContractView {
    pub phases: Vec<String>,
    pub terminal_when: Vec<String>,
    pub recovery_rules: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelInstallerSessionResultRequest {
    pub session_id: String,
    pub access_mode: PanelAccessMode,
    pub expected_command_ids: Vec<String>,
    #[serde(default)]
    pub expected_operation_ids: Vec<String>,
    pub command_results: Vec<PanelInstallerCommandResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelInstallerCommandResult {
    pub command_id: String,
    pub exit_code: i32,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub attestation: PanelInstallerCommandAttestation,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PanelInstallerCommandAttestation {
    #[serde(default)]
    pub operation_results: Vec<PanelInstallerOperationResult>,
    #[serde(default)]
    pub os_supported: Option<bool>,
    #[serde(default)]
    pub memory_total_mb: Option<u64>,
    #[serde(default)]
    pub disk_free_mb: Option<u64>,
    #[serde(default)]
    pub selected_port_available: Option<bool>,
    #[serde(default)]
    pub dependencies_ready: Option<bool>,
    #[serde(default)]
    pub binary_installed: Option<bool>,
    #[serde(default)]
    pub binary_path: Option<String>,
    #[serde(default)]
    pub artifact_source_url: Option<String>,
    #[serde(default)]
    pub artifact_sha256_verified: Option<bool>,
    #[serde(default)]
    pub artifact_signature_verified: Option<bool>,
    #[serde(default)]
    pub config_written: Option<bool>,
    #[serde(default)]
    pub bind_address: Option<String>,
    #[serde(default)]
    pub certificate_path: Option<String>,
    #[serde(default)]
    pub private_key_path: Option<String>,
    #[serde(default)]
    pub private_key_mode: Option<String>,
    #[serde(default)]
    pub fingerprint_sha256: Option<String>,
    #[serde(default)]
    pub firewall_rules_applied: Option<bool>,
    #[serde(default)]
    pub security_defaults_applied: Option<bool>,
    #[serde(default)]
    pub service_name: Option<String>,
    #[serde(default)]
    pub service_active: Option<bool>,
    #[serde(default)]
    pub health_check_ok: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelInstallerOperationResult {
    pub operation_id: String,
    pub exit_code: i32,
    pub completed: bool,
    #[serde(default)]
    pub verified: Option<bool>,
    #[serde(default)]
    pub target_path: Option<String>,
    #[serde(default)]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelInstallerResultView {
    pub session_id: String,
    pub status: PanelInstallerExecutorResultStatus,
    pub accepted: bool,
    pub checked_commands: usize,
    pub rejected_command_ids: Vec<String>,
    pub issues: Vec<String>,
    pub detail: String,
}

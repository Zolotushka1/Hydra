use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum NodeProvisioningTaskStatus {
    Pending,
    Running,
    Failed,
    Completed,
    Verified,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeProvisioningStepStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeProvisioningStepKind {
    Preflight,
    AgentReachability,
    RuntimeHealth,
    ConfigApply,
    BootstrapVerify,
    SshConnect,
    SudoCheck,
    XrayInstall,
    SidecarRuntimeInstall,
    NodeInstall,
    ServiceInstall,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeProvisioningFailureCategory {
    Connectivity,
    Authentication,
    Authorization,
    Validation,
    Runtime,
    Apply,
    BootstrapVerification,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum NodeProvisioningRemotePrerequisiteKind {
    OsSupported,
    SudoAvailable,
    DiskOk,
    MemoryOk,
    PortsAvailable,
    PackageManagerAvailable,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum NodeProvisioningRemediationAction {
    RetryTask,
    ReprovisionNode,
    RunBootstrapProbe,
    CheckLocalApi,
    CheckSshConnectivity,
    CheckSudoAccess,
    RestartNodeRuntime,
    RollbackNodeRuntime,
    UpdateXrayCore,
    ApplyNodeConfig,
    ReviewFirewall,
    InspectRuntimeState,
    RotateNodeAuthToken,
    FreeRequiredPorts,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningStep {
    pub step: NodeProvisioningStepKind,
    pub status: NodeProvisioningStepStatus,
    pub detail: String,
    pub failure_category: Option<NodeProvisioningFailureCategory>,
    pub created_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningRemediation {
    pub action: NodeProvisioningRemediationAction,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningFailure {
    pub step: NodeProvisioningStepKind,
    pub category: NodeProvisioningFailureCategory,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeProvisioningTransportKind {
    None,
    Ssh,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningRequestContext {
    pub transport: NodeProvisioningTransportKind,
    pub target_host: Option<String>,
    pub ssh_port: Option<u16>,
    pub ssh_username: Option<String>,
    pub uses_password_auth: bool,
    pub uses_private_key_auth: bool,
    #[serde(default)]
    pub sidecar_install: NodeProvisioningSidecarInstallSelection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningTask {
    pub task_id: String,
    pub node_id: String,
    #[serde(default)]
    pub parent_task_id: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    pub status: NodeProvisioningTaskStatus,
    pub created_at_unix: u64,
    pub started_at_unix: Option<u64>,
    pub finished_at_unix: Option<u64>,
    pub updated_at_unix: u64,
    pub verify_after_finish: bool,
    pub verified_ready: Option<bool>,
    pub verify_probe_id: Option<String>,
    pub recommendations: Vec<String>,
    pub failures: Vec<NodeProvisioningFailure>,
    pub remediation: Vec<NodeProvisioningRemediation>,
    pub request_context: NodeProvisioningRequestContext,
    #[serde(default = "default_node_provisioning_executor_readiness")]
    pub executor_readiness: NodeProvisioningExecutorReadinessReport,
    #[serde(default)]
    pub remote_prerequisites: Vec<NodeProvisioningRemotePrerequisiteCheck>,
    pub planned_steps: Vec<NodeProvisioningStepKind>,
    pub steps: Vec<NodeProvisioningStep>,
    #[serde(default)]
    pub handoff: Vec<NodeProvisioningHandoffRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningExecutorRequest {
    pub ssh_host: Option<String>,
    pub ssh_port: Option<u16>,
    pub ssh_username: Option<String>,
    pub ssh_password: Option<String>,
    pub ssh_private_key_pem: Option<String>,
    #[serde(default)]
    pub sidecar_install: Option<NodeProvisioningSidecarInstallSelection>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NodeProvisioningSidecarInstallSelection {
    #[serde(default)]
    pub install_hysteria2: bool,
    #[serde(default)]
    pub hysteria2_artifact_url: Option<String>,
    #[serde(default)]
    pub install_wireguard: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartNodeProvisioningRequest {
    pub verify_after_finish: Option<bool>,
    pub executor: Option<NodeProvisioningExecutorRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryNodeProvisioningRequest {
    pub verify_after_finish: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReprovisionNodeRequest {
    pub verify_after_finish: Option<bool>,
    pub executor: Option<NodeProvisioningExecutorRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateNodeProvisioningStepRequest {
    pub step: NodeProvisioningStepKind,
    pub status: NodeProvisioningStepStatus,
    pub detail: String,
    pub failure_category: Option<NodeProvisioningFailureCategory>,
    #[serde(default)]
    pub remote_prerequisites: Vec<NodeProvisioningRemotePrerequisiteCheck>,
    #[serde(default)]
    pub ssh_preflight_output: Option<String>,
    #[serde(default)]
    pub from_command_report: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TouchNodeProvisioningTaskRequest {
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum NodeProvisioningHandoffKind {
    TokenIssued,
    NodeEnvWritten,
    ServiceStarted,
    AgentReturned,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeProvisioningHandoffStatus {
    Pending,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningHandoffRecord {
    pub kind: NodeProvisioningHandoffKind,
    pub status: NodeProvisioningHandoffStatus,
    pub detail: String,
    pub created_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportNodeProvisioningHandoffRequest {
    pub kind: NodeProvisioningHandoffKind,
    pub status: NodeProvisioningHandoffStatus,
    pub detail: String,
    #[serde(default)]
    pub node_env_attestation: Option<NodeProvisioningNodeEnvAttestation>,
    #[serde(default)]
    pub service_started_attestation: Option<NodeProvisioningServiceStartedAttestation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningNodeEnvAttestation {
    pub path: String,
    pub mode: String,
    pub owner_uid: u32,
    pub owner_gid: u32,
    pub is_regular_file: bool,
    pub atomic_write: bool,
    pub env_keys: Vec<String>,
    pub schema_fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningServiceStartedAttestation {
    pub service_name: String,
    pub unit_file_path: String,
    pub load_state: String,
    pub active_state: String,
    pub unit_file_state: String,
    pub exec_start_path: String,
    pub environment_file_path: String,
    pub working_directory: String,
    #[serde(default)]
    pub additional_services: Vec<NodeProvisioningManagedServiceAttestation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningManagedServiceAttestation {
    pub service_name: String,
    pub unit_file_path: String,
    pub load_state: String,
    pub active_state: String,
    pub unit_file_state: String,
    pub exec_start_path: String,
    pub environment_file_path: String,
    pub working_directory: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportNodeProvisioningCommandRequest {
    pub step: NodeProvisioningStepKind,
    pub exit_code: i32,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningCommandReportView {
    pub step: NodeProvisioningStepKind,
    pub status: NodeProvisioningStepStatus,
    pub exit_code: i32,
    pub stdout_summary: Option<String>,
    pub stderr_summary: Option<String>,
    pub failure_category: Option<NodeProvisioningFailureCategory>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningSshPreflightProbeView {
    pub required_ports: Vec<u16>,
    pub script: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningSshInstallScriptStep {
    pub step: NodeProvisioningStepKind,
    pub description: String,
    pub script: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningSshInstallPlanView {
    #[serde(default)]
    pub sidecar_install: NodeProvisioningSidecarInstallSelection,
    #[serde(default)]
    pub env_schema: Vec<NodeProvisioningEnvVarSpec>,
    pub scripts: Vec<NodeProvisioningSshInstallScriptStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningExecutorSessionStep {
    pub step: NodeProvisioningStepKind,
    pub executor_action: String,
    pub report_endpoint_suffix: String,
    pub requires_heartbeat: bool,
    pub expected_report: NodeProvisioningExecutorExpectedReport,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeProvisioningExecutorReportMode {
    StructuredStep,
    CommandReport,
    OrchestrationStep,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningExecutorExpectedReport {
    pub mode: NodeProvisioningExecutorReportMode,
    pub required_fields: Vec<String>,
    pub description: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeProvisioningExecutorWorkflowNodeKind {
    Step,
    Handoff,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningExecutorWorkflowNode {
    pub id: String,
    pub kind: NodeProvisioningExecutorWorkflowNodeKind,
    pub step: Option<NodeProvisioningStepKind>,
    pub handoff: Option<NodeProvisioningHandoffKind>,
    pub action: String,
    pub endpoint_suffix: String,
    pub depends_on: Vec<String>,
    pub requires_heartbeat: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningExecutorWorkflowTransition {
    pub from: String,
    pub to: String,
    pub condition: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningExecutorWorkflowView {
    pub nodes: Vec<NodeProvisioningExecutorWorkflowNode>,
    pub transitions: Vec<NodeProvisioningExecutorWorkflowTransition>,
    pub validation: NodeProvisioningExecutorWorkflowValidationView,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningExecutorWorkflowValidationView {
    pub valid: bool,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeProvisioningExecutorReplayPolicy {
    SafeToReplay,
    ReissueRequired,
    PanelObservedOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningExecutorResumeNode {
    pub id: String,
    pub replay_policy: NodeProvisioningExecutorReplayPolicy,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningExecutorNextActionView {
    pub node_id: String,
    pub kind: NodeProvisioningExecutorWorkflowNodeKind,
    pub step: Option<NodeProvisioningStepKind>,
    pub handoff: Option<NodeProvisioningHandoffKind>,
    pub action: String,
    pub endpoint_suffix: String,
    pub executor_should_submit: bool,
    pub requires_heartbeat: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeProvisioningExecutorCommandPayloadKind {
    None,
    SshPreflightProbe,
    SshInstallScript,
    MaterialHandoff,
    PanelObservedWait,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeProvisioningExecutorResultSubmissionKind {
    StepReport,
    CommandReport,
    HandoffReport,
    PanelObserved,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningExecutorAcceptanceContractView {
    pub submission_kind: NodeProvisioningExecutorResultSubmissionKind,
    pub endpoint_suffix: Option<String>,
    pub executor_may_submit: bool,
    pub required_success_fields: Vec<String>,
    pub fail_closed_checks: Vec<String>,
    pub description: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeProvisioningExecutorRejectionCode {
    IncompatibleExecutorContract,
    ExecutorIdentityRequired,
    ExecutorNotRegistered,
    ExecutorDisabled,
    InvalidPreflightEvidence,
    WrongResultChannel,
    MissingNodeEnvAttestation,
    MissingServiceAttestation,
    TaskNotActive,
    TaskNotFound,
    InvalidResult,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeProvisioningExecutorRecoveryHint {
    RetryWithCorrectPayload,
    UseCommandReport,
    ReattestRemoteState,
    RefreshSession,
    Stop,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningExecutorRejectionView {
    pub error: String,
    pub code: NodeProvisioningExecutorRejectionCode,
    pub recovery_hint: NodeProvisioningExecutorRecoveryHint,
    pub retryable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningExecutorAcceptedResultView {
    pub task_id: String,
    pub node_id: String,
    pub accepted_node_id: Option<String>,
    pub task_status: NodeProvisioningTaskStatus,
    pub completed_node_ids: Vec<String>,
    pub next_node_ids: Vec<String>,
    pub recommended_next_command: Option<NodeProvisioningExecutorCommandEnvelopeView>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeProvisioningExecutorLoopPhase {
    ReadNextCommand,
    ExecuteCommand,
    SubmitResult,
    HandleAcceptedResult,
    HandleRejectedResult,
    ContinueOrRecover,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningExecutorLoopPhaseView {
    pub phase: NodeProvisioningExecutorLoopPhase,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningExecutorLoopContractView {
    pub phases: Vec<NodeProvisioningExecutorLoopPhaseView>,
    pub command_source: String,
    pub accepted_result_shape: String,
    pub rejected_result_shape: String,
    pub terminal_when: Vec<String>,
    pub recovery_rules: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeProvisioningExecutorCapability {
    WorkflowGraph,
    ResumeProjection,
    RecommendedNextCommand,
    AcceptanceContract,
    MachineReadableRejections,
    AcceptedResultProjection,
    LoopContract,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeProvisioningExecutorCompatibilityStatus {
    Compatible,
    UnknownExecutor,
    ExecutorUpgradeRequired,
    PanelUpgradeRequired,
    MissingRequiredCapabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningExecutorCompatibilityView {
    pub status: NodeProvisioningExecutorCompatibilityStatus,
    pub compatible: bool,
    pub requested_contract_version: Option<u16>,
    pub supported_contract_version: u16,
    pub minimum_executor_contract_version: u16,
    pub minimum_executor_version: String,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningExecutorHandshakeRequest {
    pub executor_id: String,
    pub executor_contract_version: Option<u16>,
    pub executor_version: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<NodeProvisioningExecutorCapability>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningExecutorPanelIdentityView {
    pub product: String,
    pub api_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningExecutorHeartbeatPolicyView {
    pub heartbeat_interval_seconds: u64,
    pub stale_after_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningExecutorSecurityExpectationsView {
    pub result_submissions_require_compatible_contract_version: bool,
    pub panel_observed_transitions_cannot_be_self_reported: bool,
    pub secret_payloads_must_not_be_persisted: bool,
    pub remote_attestations_required_for_sensitive_handoffs: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningExecutorHandshakeView {
    pub schema_version: u16,
    pub panel: NodeProvisioningExecutorPanelIdentityView,
    pub minimum_executor_version: String,
    pub supported_capabilities: Vec<NodeProvisioningExecutorCapability>,
    pub required_capabilities: Vec<NodeProvisioningExecutorCapability>,
    pub missing_required_capabilities: Vec<NodeProvisioningExecutorCapability>,
    pub compatibility: NodeProvisioningExecutorCompatibilityView,
    pub heartbeat_policy: NodeProvisioningExecutorHeartbeatPolicyView,
    pub security_expectations: NodeProvisioningExecutorSecurityExpectationsView,
    pub executor: NodeProvisioningExecutorRegistrationView,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NodeProvisioningExecutorSessionQuery {
    pub executor_contract_version: Option<u16>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NodeProvisioningExecutorResultQuery {
    pub executor_id: Option<String>,
    pub executor_contract_version: Option<u16>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeProvisioningExecutorSubmissionStatus {
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningExecutorSubmissionEntry {
    pub executor_id: String,
    pub node_id: String,
    pub task_id: String,
    pub submission_kind: NodeProvisioningExecutorResultSubmissionKind,
    pub status: NodeProvisioningExecutorSubmissionStatus,
    pub accepted_node_id: Option<String>,
    pub detail: String,
    pub created_at_unix: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NodeProvisioningExecutorSubmissionsQuery {
    pub executor_id: Option<String>,
    pub node_id: Option<String>,
    pub task_id: Option<String>,
    pub status: Option<NodeProvisioningExecutorSubmissionStatus>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningExecutorRegistrationView {
    pub executor_id: String,
    #[serde(default = "default_executor_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub auth_token_configured: bool,
    pub auth_token_issued_at_unix: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_token_hash: Option<String>,
    pub executor_version: Option<String>,
    pub last_contract_version: Option<u16>,
    pub capabilities: Vec<NodeProvisioningExecutorCapability>,
    pub last_handshake_at_unix: u64,
    pub last_compatibility_status: NodeProvisioningExecutorCompatibilityStatus,
    pub accepted_result_count: u64,
    pub rejected_result_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RotateNodeProvisioningExecutorTokenResponse {
    pub executor_id: String,
    pub auth_token: String,
    pub generated_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateNodeProvisioningExecutorTrustRequest {
    pub enabled: bool,
    pub reason: Option<String>,
}

fn default_executor_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningExecutorResultActorView {
    pub executor_id: String,
    pub executor_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningExecutorCommandEnvelopeView {
    pub node_id: String,
    pub payload_kind: NodeProvisioningExecutorCommandPayloadKind,
    pub script: Option<String>,
    pub replay_policy: Option<NodeProvisioningExecutorReplayPolicy>,
    pub expected_report: Option<NodeProvisioningExecutorExpectedReport>,
    pub required_handoff: Option<NodeProvisioningHandoffKind>,
    pub attestation_script: Option<String>,
    pub action: String,
    pub endpoint_suffix: String,
    pub executor_should_submit: bool,
    pub requires_heartbeat: bool,
    pub reason: String,
    pub acceptance: NodeProvisioningExecutorAcceptanceContractView,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningExecutorResumeView {
    pub completed_node_ids: Vec<String>,
    pub runnable_node_ids: Vec<String>,
    pub replayable_nodes: Vec<NodeProvisioningExecutorResumeNode>,
    pub blocked_node_ids: Vec<String>,
    pub next_node_ids: Vec<String>,
    pub recommended_next_action: Option<NodeProvisioningExecutorNextActionView>,
    pub recommended_next_command: Option<NodeProvisioningExecutorCommandEnvelopeView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningExecutorFinishCondition {
    pub status: NodeProvisioningTaskStatus,
    pub description: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeProvisioningCompletionProofSource {
    PanelObserved,
    RemoteAttestation,
    Missing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningHandoffCompletionView {
    pub kind: NodeProvisioningHandoffKind,
    pub status: Option<NodeProvisioningHandoffStatus>,
    pub proof_source: NodeProvisioningCompletionProofSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningExecutorCompletionView {
    pub can_finish_completed: bool,
    pub can_finish_verified: bool,
    pub executor_steps_complete: bool,
    pub missing_executor_steps: Vec<NodeProvisioningStepKind>,
    pub failed_executor_steps: Vec<NodeProvisioningStepKind>,
    pub required_handoffs_complete: bool,
    pub missing_handoffs: Vec<NodeProvisioningHandoffKind>,
    pub failed_handoffs: Vec<NodeProvisioningHandoffKind>,
    pub handoff_evidence: Vec<NodeProvisioningHandoffCompletionView>,
    pub bootstrap_verified: Option<bool>,
    pub blockers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningExecutorSessionView {
    pub schema_version: u16,
    pub minimum_executor_version: String,
    pub capabilities: Vec<NodeProvisioningExecutorCapability>,
    pub compatibility: NodeProvisioningExecutorCompatibilityView,
    pub task_id: String,
    pub node_id: String,
    pub heartbeat_interval_seconds: u64,
    pub stale_after_seconds: u64,
    pub preflight_probe: Option<NodeProvisioningSshPreflightProbeView>,
    pub install_plan: Option<NodeProvisioningSshInstallPlanView>,
    pub material_handoff: Option<NodeProvisioningMaterialHandoffPlanView>,
    pub steps: Vec<NodeProvisioningExecutorSessionStep>,
    pub workflow: NodeProvisioningExecutorWorkflowView,
    pub resume: NodeProvisioningExecutorResumeView,
    pub loop_contract: NodeProvisioningExecutorLoopContractView,
    pub finish_conditions: Vec<NodeProvisioningExecutorFinishCondition>,
    pub completion: NodeProvisioningExecutorCompletionView,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeProvisioningMaterialSensitivity {
    Public,
    Sensitive,
    HighSensitivity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningMaterialDirectory {
    pub path: String,
    pub mode: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningMaterialFile {
    pub path: String,
    pub mode: String,
    pub sensitivity: NodeProvisioningMaterialSensitivity,
    pub source: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningMaterialFetch {
    pub name: String,
    pub endpoint: String,
    pub authentication: String,
    pub plaintext_returned_once: bool,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningEnvVarSpec {
    pub name: String,
    pub required: bool,
    pub sensitivity: NodeProvisioningMaterialSensitivity,
    pub source: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningMaterialHandoffPlanView {
    pub directories: Vec<NodeProvisioningMaterialDirectory>,
    pub files: Vec<NodeProvisioningMaterialFile>,
    pub fetches: Vec<NodeProvisioningMaterialFetch>,
    pub env_schema: Vec<NodeProvisioningEnvVarSpec>,
    pub node_env_attestation_schema_fingerprint: Option<String>,
    pub node_env_attestation_script: Option<String>,
    pub service_started_attestation_script: Option<String>,
    pub write_sequence: Vec<String>,
    pub expected_reports: Vec<NodeProvisioningHandoffKind>,
    pub post_install_checks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinishNodeProvisioningRequest {
    pub status: NodeProvisioningTaskStatus,
    pub detail: Option<String>,
    pub run_bootstrap_probe: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningFinishRejectionView {
    pub error: String,
    pub completion: NodeProvisioningExecutorCompletionView,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningEvent {
    pub task_id: String,
    pub node_id: String,
    pub kind: String,
    pub detail: String,
    pub created_at_unix: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NodeProvisioningEventsQuery {
    pub task_id: Option<String>,
    pub kind: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeProvisioningPreflightStatus {
    Passed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningPreflightCheck {
    pub check: String,
    pub status: NodeProvisioningPreflightStatus,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningRemotePrerequisiteCheck {
    pub prerequisite: NodeProvisioningRemotePrerequisiteKind,
    pub status: NodeProvisioningPreflightStatus,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningPreflightReport {
    pub node_id: String,
    pub passed: bool,
    pub checked_at_unix: u64,
    pub checks: Vec<NodeProvisioningPreflightCheck>,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningExecutorReadinessReport {
    pub transport: NodeProvisioningTransportKind,
    pub ready: bool,
    pub checked_at_unix: u64,
    pub checks: Vec<NodeProvisioningPreflightCheck>,
    pub recommendations: Vec<String>,
}

fn default_node_provisioning_executor_readiness() -> NodeProvisioningExecutorReadinessReport {
    NodeProvisioningExecutorReadinessReport {
        transport: NodeProvisioningTransportKind::None,
        ready: false,
        checked_at_unix: 0,
        checks: Vec::new(),
        recommendations: Vec::new(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningStatusView {
    pub node_id: String,
    pub checked_at_unix: u64,
    pub latest_task: Option<NodeProvisioningTask>,
    pub latest_status: Option<NodeProvisioningTaskStatus>,
    pub stale_active_task: bool,
    pub current_step: Option<NodeProvisioningStep>,
    pub failed_step: Option<NodeProvisioningStep>,
    pub can_retry: bool,
    pub can_reprovision: bool,
    pub preflight: NodeProvisioningPreflightReport,
    pub completion: Option<NodeProvisioningExecutorCompletionView>,
    pub next_actions: Vec<NodeProvisioningRemediation>,
    pub recovery: NodeProvisioningRecoveryView,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeProvisioningRecoveryDecision {
    Ready,
    Retry,
    Reprovision,
    RepairFirst,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProvisioningRecoveryView {
    pub decision: NodeProvisioningRecoveryDecision,
    pub detail: String,
    pub required_actions: Vec<NodeProvisioningRemediation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedNodeProvisioningTasks {
    pub tasks: Vec<NodeProvisioningTask>,
    #[serde(default)]
    pub executors: Vec<NodeProvisioningExecutorRegistrationView>,
    #[serde(default)]
    pub executor_submissions: Vec<NodeProvisioningExecutorSubmissionEntry>,
}

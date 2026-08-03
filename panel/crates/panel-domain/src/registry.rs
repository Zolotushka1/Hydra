//! Registry of enums that cross the HTTP API.
//!
//! `enum_registry!` gives a type two things:
//!
//! - `ALL`, every variant in declaration order, from which the
//!   `GET /api/ui/contracts` document is built;
//! - an exhaustive `match`, so a variant added to the enum but not to the
//!   registry **fails to compile**.
//!
//! Contract values are not taken from here but from the serde serialization of
//! `ALL`, so the declared string and the wire string are the same string by
//! construction. They used to be two independent lists, and
//! `panel_installer_payload_kind` had already lost `dependency_install`.

macro_rules! enum_registry {
    ($name:ty { $($variant:ident),+ $(,)? }) => {
        impl $name {
            /// Every variant in declaration order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// Exists for the exhaustive `match`: a new enum variant that is not
            /// added to the registry breaks compilation instead of quietly
            /// dropping out of the contract.
            #[allow(dead_code)]
            const fn assert_registry_is_exhaustive(self) {
                match self {
                    $(Self::$variant => ()),+
                }
            }
        }
    };
}

use crate::cluster::{ClusterNodeRole, ClusterStatus};
enum_registry!(ClusterStatus {
    Draft,
    Active,
    Disabled
});
enum_registry!(ClusterNodeRole { Entry, Relay, Exit });

use crate::installer::{
    PanelAccessMode, PanelInstallerExecutorPayloadKind, PanelInstallerExecutorResultStatus,
    PanelInstallerJobStatus, PanelInstallerPackageChannel, PanelInstallerTargetArch,
    PanelInstallerTargetOs, PanelSecurityPosture,
};
enum_registry!(PanelAccessMode {
    DomainTls,
    IpHttp,
    IpSelfSignedTls,
    ReverseProxy
});
enum_registry!(PanelSecurityPosture {
    Recommended,
    LimitedWithoutDomain,
    DangerPlainHttpPublic,
    CustomReverseProxy
});
enum_registry!(PanelInstallerExecutorPayloadKind {
    PreflightProbe,
    DependencyInstall,
    InstallBinary,
    CertificateOperation,
    ListenerConfig,
    FirewallConfig,
    SecurityDefaults,
    ServiceInstall,
    HealthCheck,
});
enum_registry!(PanelInstallerExecutorResultStatus { Accepted, Rejected });
enum_registry!(PanelInstallerJobStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Rejected,
    Expired
});
enum_registry!(PanelInstallerTargetOs { Linux, Windows });
enum_registry!(PanelInstallerTargetArch { X86_64, Aarch64 });
enum_registry!(PanelInstallerPackageChannel {
    Stable,
    Latest,
    Pinned
});

use crate::network::{
    DeploymentScenarioId, HostSecurity, InboundTransport, ProtocolSupportStatus, ProxyType,
    VlessFlow, XhttpMode,
};
enum_registry!(ProxyType {
    Vless,
    Hysteria2,
    Wireguard
});
enum_registry!(ProtocolSupportStatus {
    Production,
    Legacy,
    Planned
});
enum_registry!(InboundTransport {
    Tcp,
    Udp,
    Ws,
    Grpc,
    HttpUpgrade,
    Quic,
    Xhttp
});
enum_registry!(XhttpMode {
    Auto,
    PacketUp,
    StreamUp,
    StreamOne
});
enum_registry!(VlessFlow { XtlsRprxVision });
enum_registry!(DeploymentScenarioId {
    DirectMaxStealth,
    DirectMaxThroughput,
    BehindCdn
});
enum_registry!(HostSecurity { None, Tls, Reality });

use crate::node::{
    NodeApplyLifecycleState, NodeApplyResultStatus, NodeApplyTimelineStage,
    NodeApplyTimelineStatus, NodeHealthFlag, NodeProvisioningStatus, NodeRuntimeAlertKind,
    NodeStatus, NodeSyncStatus, RuntimeComponent, RuntimeComponentAction,
};
enum_registry!(NodeStatus {
    Unknown,
    Healthy,
    Degraded,
    Offline
});
enum_registry!(NodeSyncStatus {
    Unknown,
    Synced,
    Drifted,
    Pending
});
enum_registry!(NodeProvisioningStatus {
    None,
    Pending,
    Running,
    Failed,
    Completed
});
enum_registry!(NodeApplyLifecycleState {
    Unknown,
    Pending,
    Downloaded,
    Rendered,
    Validated,
    Applied,
    Failed,
    RolledBack,
});
enum_registry!(NodeApplyResultStatus {
    Applied,
    Failed,
    RolledBack,
    Skipped
});
enum_registry!(NodeRuntimeAlertKind {
    PollBackoff,
    RuntimeValidationFailed,
    XrayRuntimeFailed,
    XrayUpdateFailed,
    SidecarFailed,
    SidecarDegraded
});
enum_registry!(NodeApplyTimelineStatus {
    Pending,
    Active,
    Ok,
    Warning,
    Failed,
    Skipped,
    Unknown,
});

use crate::provisioning::{
    NodeProvisioningFailureCategory, NodeProvisioningRecoveryDecision, NodeProvisioningStepKind,
};
enum_registry!(NodeProvisioningStepKind {
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
});
enum_registry!(NodeProvisioningFailureCategory {
    Connectivity,
    Authentication,
    Authorization,
    Validation,
    Runtime,
    Apply,
    BootstrapVerification,
    Unknown,
});
enum_registry!(NodeProvisioningRecoveryDecision {
    Ready,
    Retry,
    Reprovision,
    RepairFirst
});

use crate::security::AuditEventType;
enum_registry!(AuditEventType {
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
});

use crate::subscription::{
    SubscriptionCatalogClientStatus, SubscriptionDeviceEnrollmentStatus, SubscriptionDeviceStatus,
    SubscriptionFormat, SubscriptionSessionEnforcementStatus, SubscriptionSessionObservationSource,
    SubscriptionSessionRuntimeAdapter, SubscriptionSessionRuntimeCapability,
    SubscriptionSessionVerdict, SubscriptionUsageWindowPreset,
};
enum_registry!(SubscriptionCatalogClientStatus {
    Active,
    Disabled,
    Expired,
    Revoked
});
enum_registry!(SubscriptionFormat {
    Json,
    PlainText,
    Base64,
    DiagnosticJson
});
enum_registry!(SubscriptionDeviceEnrollmentStatus {
    Active,
    Consumed,
    Revoked,
    Expired
});
enum_registry!(SubscriptionDeviceStatus { Active, Revoked });
enum_registry!(SubscriptionUsageWindowPreset {
    Hours12,
    Day1,
    Days3,
    Week1,
    Month1,
    Months3,
    Custom,
});
enum_registry!(SubscriptionSessionVerdict { Allow, Block });
enum_registry!(SubscriptionSessionEnforcementStatus {
    Pending,
    Applied,
    Failed
});
enum_registry!(SubscriptionSessionRuntimeAdapter {
    NodeManagedExactSession
});
enum_registry!(SubscriptionSessionObservationSource {
    NodeManagedRuntimeTable
});
enum_registry!(SubscriptionSessionRuntimeCapability {
    OpaqueSessionReference,
    ExactSessionTermination,
    PostActionAbsenceVerification,
    PrincipalWideTerminationOnly
});

use crate::system::{AlertEventStatus, AlertKind, AlertSeverity, ResourceBudgetStatus};
enum_registry!(ResourceBudgetStatus {
    Ok,
    Warning,
    OverLimit
});
enum_registry!(AlertKind {
    DiskUsage,
    MemoryUsage,
    PanelMemoryBudget,
    NodeOffline,
    NodeStaleHeartbeat,
    NodeConfigDrift,
    NodeProvisioningStale,
    NodeProvisioningFailed,
    NodeReportedApplyFailed,
    NodeRuntimeAlert,
});
enum_registry!(AlertSeverity { Warning, Critical });
enum_registry!(AlertEventStatus {
    Activated,
    Resolved
});

use crate::telegram::TelegramEventStatus;
enum_registry!(TelegramEventStatus {
    Queued,
    Delivered,
    RetryScheduled,
    Skipped,
    Failed
});

use crate::user::UserStatus;
enum_registry!(UserStatus {
    Active,
    Disabled,
    Expired,
    OnHold
});

enum_registry!(RuntimeComponent {
    Xray,
    Hysteria2Sidecar,
    WireguardNodeNative
});
enum_registry!(RuntimeComponentAction {
    Install,
    Update,
    Validate,
    Start,
    Stop,
    Restart,
    Status,
    Logs,
});
enum_registry!(NodeApplyTimelineStage {
    FetchRuntimeConfig,
    FetchRouteCredentials,
    RenderXrayConfig,
    ValidateXrayConfig,
    WriteRuntimeState,
    RestartXray,
    ReportSync,
    ReportApplyResult,
});
enum_registry!(NodeHealthFlag {
    Disabled,
    Offline,
    Degraded,
    UnknownStatus,
    StaleHeartbeat,
    StaleMetrics,
    ConfigDrift,
    ApplyPending,
    RetryBackoffActive,
    RollbackAvailable,
    ReportedApplyFailed,
    ProvisioningRunning,
    ProvisioningStale,
    ProvisioningFailed,
    RuntimeAlertsActive,
    DiskHigh,
    MemoryHigh,
});

use serde::{Deserialize, Serialize};

use crate::node::{RuntimeComponent, RuntimeComponentAction};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum InboundTransport {
    Tcp,
    Udp,
    Ws,
    Grpc,
    HttpUpgrade,
    Quic,
    /// Splits upstream and downstream into separate HTTP transactions, so the
    /// connection profile after the handshake stops looking like a tunnel.
    /// Reality covers the handshake; XHTTP covers what follows it.
    Xhttp,
}

/// XHTTP mode.
///
/// Not cosmetic: the mode decides compatibility with `flow: xtls-rprx-vision`
/// and whether the connection survives a CDN. Xray accepts every combination at
/// `run -test`, so these constraints have to live in our own validation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum XhttpMode {
    /// Xray picks the mode itself. With XHTTP + Reality that is `stream-one`.
    Auto,
    /// The only mode guaranteed to pass through a CDN or Nginx. Incompatible
    /// with Vision.
    PacketUp,
    StreamUp,
    /// The only mode compatible with `flow: xtls-rprx-vision`.
    StreamOne,
}

impl XhttpMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::PacketUp => "packet-up",
            Self::StreamUp => "stream-up",
            Self::StreamOne => "stream-one",
        }
    }

    /// The mode that will actually be in force.
    ///
    /// Xray resolves `auto` to `stream-one` when Reality is enabled. Reflected
    /// explicitly so the operator does not have to guess which mode is running.
    pub const fn effective(self, reality_enabled: bool) -> Self {
        match (self, reality_enabled) {
            (Self::Auto, true) => Self::StreamOne,
            (mode, _) => mode,
        }
    }

    /// Compatible with `flow: xtls-rprx-vision`.
    pub const fn supports_vision(self) -> bool {
        matches!(self, Self::StreamOne)
    }

    /// Guaranteed to pass through a CDN or Nginx.
    pub const fn survives_cdn(self) -> bool {
        matches!(self, Self::PacketUp)
    }

    /// Reads a mode back from its wire spelling.
    ///
    /// Resolved through `ALL` rather than a hand-written match, so a new variant
    /// cannot parse into nothing while still being published in the contract.
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|candidate| candidate.as_str() == value)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolSupportStatus {
    Production,
    Legacy,
    Planned,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolRuntimeOwner {
    Xray,
    Sidecar,
    NodeNative,
    Planned,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolSecurityMode {
    None,
    Tls,
    Reality,
    MutualTls,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolModeCapability {
    pub transport: InboundTransport,
    pub security: ProtocolSecurityMode,
    pub production_ready: bool,
    pub requires_domain: bool,
    pub requires_path: bool,
    pub requires_secret_material: bool,
    pub notes: Vec<String>,
}

/// Flow VLESS.
///
/// Vision is a **`flow` flag, not a transport**. It lives over plain tcp and
/// inside XHTTP `stream-one` alike. The axis of choice is the transport, `tcp`
/// against `xhttp`, never "Vision against XHTTP".
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum VlessFlow {
    XtlsRprxVision,
}

impl VlessFlow {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::XtlsRprxVision => "xtls-rprx-vision",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentScenarioId {
    DirectMaxStealth,
    DirectMaxThroughput,
    BehindCdn,
}

/// A typical deployment scenario.
///
/// Not a new abstraction: each scenario **points at an existing capability
/// matrix row** and explains when to choose it. The matrix answers "what is
/// technically possible"; a scenario answers "what should I pick", which a grid
/// of protocol/transport/security checkboxes does not.
///
/// The `protocol + transport + security` triple must exist in the matrix as a
/// production-ready row. A test enforces that, so a scenario cannot advertise
/// something the matrix does not have.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentScenario {
    pub id: DeploymentScenarioId,
    pub title: String,
    pub protocol: String,
    pub transport: InboundTransport,
    pub security: ProtocolSecurityMode,
    /// `None` means Vision is not used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow: Option<VlessFlow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xhttp_mode: Option<XhttpMode>,
    /// Passes through a CDN. Neither Reality nor Vision is available there.
    pub cdn_compatible: bool,
    /// The recommended default choice.
    pub recommended: bool,
    /// Why this scenario and not its neighbour. A trade-off, not a pitch.
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolCapability {
    pub protocol: String,
    pub display_name: String,
    pub status: ProtocolSupportStatus,
    pub recommended_default: bool,
    pub runtime: String,
    pub runtime_owner: ProtocolRuntimeOwner,
    pub required_binaries: Vec<String>,
    pub required_secret_classes: Vec<String>,
    pub supported_transports: Vec<InboundTransport>,
    pub supported_security_modes: Vec<ProtocolSecurityMode>,
    pub disabled_reason: Option<String>,
    pub modes: Vec<ProtocolModeCapability>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolCapabilitiesView {
    pub schema_version: u16,
    /// Typical scenarios layered over the matrix rows. They answer "what should
    /// I pick", which the rows themselves do not.
    #[serde(default)]
    pub deployment_scenarios: Vec<DeploymentScenario>,
    pub runtime_components: Vec<ProtocolRuntimeComponentView>,
    pub capabilities: Vec<ProtocolCapability>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolRuntimeComponentView {
    pub owner: ProtocolRuntimeOwner,
    pub component: RuntimeComponent,
    pub status: ProtocolSupportStatus,
    pub production_ready: bool,
    pub supervised_by: String,
    pub supported_actions: Vec<RuntimeComponentAction>,
    pub required_binaries: Vec<String>,
    pub update_strategy: String,
    pub validation_strategy: String,
    pub disabled_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Inbound {
    pub tag: String,
    pub port: u16,
    pub protocol: String,
    pub network: InboundTransport,
    pub tls_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cluster_id: Option<String>,
    pub created_at_unix: u64,
    pub updated_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateInboundRequest {
    pub tag: String,
    pub port: u16,
    pub protocol: String,
    pub network: InboundTransport,
    pub tls_enabled: bool,
    pub node_id: Option<String>,
    pub cluster_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateInboundRequest {
    pub port: Option<u16>,
    pub protocol: Option<String>,
    pub network: Option<InboundTransport>,
    pub tls_enabled: Option<bool>,
    pub node_id: Option<String>,
    pub cluster_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HostSecurity {
    None,
    Tls,
    Reality,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Host {
    pub id: String,
    pub remark: String,
    pub address: String,
    pub port: u16,
    pub path: Option<String>,
    pub sni: Option<String>,
    pub security: HostSecurity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cluster_id: Option<String>,
    pub created_at_unix: u64,
    pub updated_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateHostRequest {
    pub remark: String,
    pub address: String,
    pub port: u16,
    pub path: Option<String>,
    pub sni: Option<String>,
    pub security: HostSecurity,
    pub node_id: Option<String>,
    pub cluster_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateHostRequest {
    pub remark: Option<String>,
    pub address: Option<String>,
    pub port: Option<u16>,
    pub path: Option<String>,
    pub sni: Option<String>,
    pub security: Option<HostSecurity>,
    pub node_id: Option<String>,
    pub cluster_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProxyType {
    Vless,
    Hysteria2,
    Wireguard,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyProfile {
    pub id: String,
    pub name: String,
    pub proxy_type: ProxyType,
    pub settings_json: String,
    pub excluded_inbound_tags: Vec<String>,
    pub created_at_unix: u64,
    pub updated_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProxyProfileRequest {
    pub name: String,
    pub proxy_type: ProxyType,
    pub settings_json: String,
    pub excluded_inbound_tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateProxyProfileRequest {
    pub name: Option<String>,
    pub settings_json: Option<String>,
    pub excluded_inbound_tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedNetworkResources {
    pub inbounds: Vec<Inbound>,
    pub hosts: Vec<Host>,
    pub proxy_profiles: Vec<ProxyProfile>,
}

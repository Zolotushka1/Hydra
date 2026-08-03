use std::{
    collections::{HashMap, HashSet},
    fs,
    net::SocketAddr,
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use hmac::{Hmac, Mac};
use node_domain::{
    SUBSCRIPTION_SESSION_RUNTIME_DRIVER_PROTOCOL_VERSION, SubscriptionSessionObservation,
    SubscriptionSessionRuntimeCapability, SubscriptionSessionRuntimeDriverOperation,
    SubscriptionSessionRuntimeDriverRequest, SubscriptionSessionRuntimeDriverResponse,
    WireGuardSessionMappingDocument, WireGuardSessionPeerMapping,
};
use sha2::Sha256;
use tokio::{io::AsyncReadExt, process::Command};

const DEFAULT_ACTIVE_WITHIN_SECONDS: u64 = 180;
const DEFAULT_COMMAND_TIMEOUT_SECONDS: u64 = 5;
const DEFAULT_MAX_COMMAND_OUTPUT_BYTES: usize = 1_048_576;
const MAX_MAPPING_BYTES: u64 = 1_048_576;
const MAX_INTERFACES: usize = 32;
const MAX_PEERS: usize = 4_096;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone)]
struct DriverConfig {
    wg_binary_path: PathBuf,
    mapping_path: PathBuf,
    session_ref_key: Vec<u8>,
    active_within_seconds: u64,
    command_timeout: Duration,
    max_command_output_bytes: usize,
}

#[derive(Debug, Clone)]
struct LivePeer {
    interface_name: String,
    public_key: String,
    endpoint: Option<String>,
    latest_handshake_at_unix: u64,
    mapping: Option<WireGuardSessionPeerMapping>,
}

impl DriverConfig {
    fn from_env() -> Result<Self> {
        let wg_binary_path = PathBuf::from(
            std::env::var("HYDRA_NODE_WIREGUARD_BINARY_PATH")
                .context("HYDRA_NODE_WIREGUARD_BINARY_PATH is required")?,
        );
        validate_executable(&wg_binary_path)?;
        let mapping_path = std::env::var("HYDRA_NODE_WIREGUARD_SESSION_MAP_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("data/sidecar-generated/wireguard-session-map.json"));
        let session_ref_key = std::env::var("HYDRA_NODE_WIREGUARD_SESSION_REF_KEY")
            .context("HYDRA_NODE_WIREGUARD_SESSION_REF_KEY is required")?
            .into_bytes();
        if session_ref_key.len() < 32 || session_ref_key.len() > 512 {
            bail!("WireGuard session reference key must be between 32 and 512 bytes");
        }
        Ok(Self {
            wg_binary_path,
            mapping_path,
            session_ref_key,
            active_within_seconds: parse_bounded_env_u64(
                "HYDRA_NODE_WIREGUARD_ACTIVE_WITHIN_SECONDS",
                DEFAULT_ACTIVE_WITHIN_SECONDS,
                30,
                3_600,
            )?,
            command_timeout: Duration::from_secs(parse_bounded_env_u64(
                "HYDRA_NODE_WIREGUARD_COMMAND_TIMEOUT_SECONDS",
                DEFAULT_COMMAND_TIMEOUT_SECONDS,
                1,
                30,
            )?),
            max_command_output_bytes: parse_bounded_env_usize(
                "HYDRA_NODE_WIREGUARD_MAX_COMMAND_OUTPUT_BYTES",
                DEFAULT_MAX_COMMAND_OUTPUT_BYTES,
                4_096,
                1_048_576,
            )?,
        })
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = DriverConfig::from_env()?;
    let operation_arg = parse_operation_arg()?;
    let request = read_request().await?;
    if request.protocol_version != SUBSCRIPTION_SESSION_RUNTIME_DRIVER_PROTOCOL_VERSION {
        bail!("unsupported session runtime driver protocol version");
    }
    if request.operation != operation_arg {
        bail!("session runtime driver operation argument does not match stdin request");
    }
    let response = execute(&config, request).await?;
    let encoded = serde_json::to_vec(&response)
        .context("failed to encode WireGuard session driver response")?;
    use std::io::Write as _;
    std::io::stdout()
        .write_all(&encoded)
        .context("failed to write WireGuard session driver response")?;
    Ok(())
}

async fn execute(
    config: &DriverConfig,
    request: SubscriptionSessionRuntimeDriverRequest,
) -> Result<SubscriptionSessionRuntimeDriverResponse> {
    let mapping = load_mapping(&config.mapping_path)?;
    let live_peers = collect_live_peers(config, &mapping).await?;
    match request.operation {
        SubscriptionSessionRuntimeDriverOperation::Handshake => {
            if mapping.interfaces.is_empty() {
                bail!("WireGuard session mapping has no configured interfaces");
            }
            Ok(success_response())
        }
        SubscriptionSessionRuntimeDriverOperation::Observe => {
            Ok(SubscriptionSessionRuntimeDriverResponse {
                observations: observations_for_live_peers(config, &live_peers),
                ..success_response()
            })
        }
        SubscriptionSessionRuntimeDriverOperation::Terminate => {
            let target = find_target(config, &request, &live_peers)?;
            run_wg(
                config,
                &[
                    "set",
                    &target.interface_name,
                    "peer",
                    &target.public_key,
                    "remove",
                ],
            )
            .await?;
            Ok(success_response())
        }
        SubscriptionSessionRuntimeDriverOperation::Verify => {
            let runtime_session_ref = request
                .runtime_session_ref
                .as_deref()
                .context("verify requires runtime_session_ref")?;
            let present = live_peers.iter().any(|peer| {
                constant_time_slice_eq(
                    runtime_ref(config, peer).as_bytes(),
                    runtime_session_ref.as_bytes(),
                )
            });
            Ok(SubscriptionSessionRuntimeDriverResponse {
                session_absent: Some(!present),
                verified_at_unix: Some(now_unix()),
                ..success_response()
            })
        }
    }
}

fn success_response() -> SubscriptionSessionRuntimeDriverResponse {
    SubscriptionSessionRuntimeDriverResponse {
        protocol_version: SUBSCRIPTION_SESSION_RUNTIME_DRIVER_PROTOCOL_VERSION,
        success: true,
        runtime_capabilities: vec![
            SubscriptionSessionRuntimeCapability::OpaqueSessionReference,
            SubscriptionSessionRuntimeCapability::ExactSessionTermination,
            SubscriptionSessionRuntimeCapability::PostActionAbsenceVerification,
        ],
        observations: Vec::new(),
        session_absent: None,
        verified_at_unix: None,
    }
}

async fn collect_live_peers(
    config: &DriverConfig,
    mapping: &WireGuardSessionMappingDocument,
) -> Result<Vec<LivePeer>> {
    let mut peers = Vec::new();
    for interface in &mapping.interfaces {
        let mapped = interface
            .peers
            .iter()
            .map(|peer| (peer.public_key.as_str(), peer))
            .collect::<HashMap<_, _>>();
        let output = run_wg(config, &["show", &interface.interface_name, "dump"]).await?;
        peers.extend(parse_dump(
            &interface.interface_name,
            &output,
            &mapped,
            config.active_within_seconds,
            now_unix(),
        )?);
        if peers.len() > MAX_PEERS {
            bail!("WireGuard active peer count exceeds configured safety limit");
        }
    }
    Ok(peers)
}

fn parse_dump(
    interface_name: &str,
    output: &[u8],
    mapped: &HashMap<&str, &WireGuardSessionPeerMapping>,
    active_within_seconds: u64,
    now: u64,
) -> Result<Vec<LivePeer>> {
    let text = std::str::from_utf8(output).context("wg dump output is not valid UTF-8")?;
    let mut peers = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if index == 0 || line.trim().is_empty() {
            continue;
        }
        let fields = line.split('\t').collect::<Vec<_>>();
        if fields.len() < 8 {
            bail!("wg dump peer row is incomplete");
        }
        let public_key = fields[0];
        validate_public_key(public_key)?;
        let latest_handshake_at_unix = fields[4]
            .parse::<u64>()
            .context("wg dump contains invalid latest handshake timestamp")?;
        if latest_handshake_at_unix == 0
            || now.saturating_sub(latest_handshake_at_unix) > active_within_seconds
        {
            continue;
        }
        peers.push(LivePeer {
            interface_name: interface_name.to_string(),
            public_key: public_key.to_string(),
            endpoint: (fields[2] != "(none)").then(|| fields[2].to_string()),
            latest_handshake_at_unix,
            mapping: mapped.get(public_key).map(|mapping| (*mapping).clone()),
        });
    }
    Ok(peers)
}

fn observations_for_live_peers(
    config: &DriverConfig,
    peers: &[LivePeer],
) -> Vec<SubscriptionSessionObservation> {
    peers
        .iter()
        .map(|peer| SubscriptionSessionObservation {
            session_id: session_id(config, peer),
            runtime_username: peer
                .mapping
                .as_ref()
                .map(|mapping| mapping.runtime_username.clone())
                .unwrap_or_else(|| "unassigned/wireguard".to_string()),
            runtime_session_ref: Some(runtime_ref(config, peer)),
            device_fingerprint: peer
                .mapping
                .as_ref()
                .map(|mapping| mapping.device_fingerprint.clone()),
            source_ip: peer.endpoint.as_deref().and_then(endpoint_ip),
            connected_at_unix: Some(peer.latest_handshake_at_unix),
        })
        .collect()
}

fn find_target<'a>(
    config: &DriverConfig,
    request: &SubscriptionSessionRuntimeDriverRequest,
    peers: &'a [LivePeer],
) -> Result<&'a LivePeer> {
    let expected_session_id = request
        .session_id
        .as_deref()
        .context("terminate requires session_id")?;
    let runtime_session_ref = request
        .runtime_session_ref
        .as_deref()
        .context("terminate requires runtime_session_ref")?;
    peers
        .iter()
        .find(|peer| {
            constant_time_slice_eq(
                session_id(config, peer).as_bytes(),
                expected_session_id.as_bytes(),
            ) && constant_time_slice_eq(
                runtime_ref(config, peer).as_bytes(),
                runtime_session_ref.as_bytes(),
            )
        })
        .context("target WireGuard peer is absent from the current active peer table")
}

fn session_id(config: &DriverConfig, peer: &LivePeer) -> String {
    opaque_id(
        &config.session_ref_key,
        b"wireguard-session-id-v1",
        &peer.interface_name,
        &peer.public_key,
    )
}

fn runtime_ref(config: &DriverConfig, peer: &LivePeer) -> String {
    opaque_id(
        &config.session_ref_key,
        b"wireguard-runtime-ref-v1",
        &peer.interface_name,
        &peer.public_key,
    )
}

fn opaque_id(key: &[u8], domain: &[u8], interface_name: &str, public_key: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(key).expect("validated HMAC key length");
    mac.update(domain);
    mac.update(b"\0");
    mac.update(interface_name.as_bytes());
    mac.update(b"\0");
    mac.update(public_key.as_bytes());
    let bytes = mac.finalize().into_bytes();
    let mut encoded = String::with_capacity(64);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

fn endpoint_ip(endpoint: &str) -> Option<String> {
    endpoint
        .parse::<SocketAddr>()
        .ok()
        .map(|address| address.ip().to_string())
}

async fn run_wg(config: &DriverConfig, args: &[&str]) -> Result<Vec<u8>> {
    let mut child = Command::new(&config.wg_binary_path)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .context("failed to start wg command")?;
    let stdout = child.stdout.take().context("wg stdout is unavailable")?;
    let execution = async {
        let (status, stdout) = tokio::try_join!(
            async { child.wait().await.context("failed to wait for wg command") },
            read_bounded(stdout, config.max_command_output_bytes),
        )?;
        Ok::<_, anyhow::Error>((status, stdout))
    };
    let (status, stdout) = tokio::time::timeout(config.command_timeout, execution)
        .await
        .context("wg command timed out")??;
    if !status.success() {
        bail!("wg command exited unsuccessfully");
    }
    Ok(stdout)
}

async fn read_bounded(
    mut reader: impl tokio::io::AsyncRead + Unpin,
    max_bytes: usize,
) -> Result<Vec<u8>> {
    let mut output = Vec::with_capacity(max_bytes.min(16_384));
    let mut buffer = [0u8; 8_192];
    loop {
        let read = reader
            .read(&mut buffer)
            .await
            .context("failed to read wg output")?;
        if read == 0 {
            break;
        }
        if output.len().saturating_add(read) > max_bytes {
            bail!("wg command output exceeds configured limit");
        }
        output.extend_from_slice(&buffer[..read]);
    }
    Ok(output)
}

fn load_mapping(path: &Path) -> Result<WireGuardSessionMappingDocument> {
    let metadata = fs::symlink_metadata(path).with_context(|| {
        format!(
            "failed to stat WireGuard session mapping {}",
            path.display()
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        bail!("WireGuard session mapping must be a regular non-symlink file");
    }
    if metadata.len() > MAX_MAPPING_BYTES {
        bail!("WireGuard session mapping exceeds byte limit");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            bail!("WireGuard session mapping must use owner-only permissions");
        }
    }
    let mapping = serde_json::from_slice::<WireGuardSessionMappingDocument>(
        &fs::read(path).context("failed to read WireGuard session mapping")?,
    )
    .context("failed to decode WireGuard session mapping")?;
    validate_mapping(&mapping)?;
    Ok(mapping)
}

fn validate_mapping(mapping: &WireGuardSessionMappingDocument) -> Result<()> {
    if mapping.schema_version != 1 {
        bail!("unsupported WireGuard session mapping schema");
    }
    if mapping.interfaces.len() > MAX_INTERFACES {
        bail!("WireGuard session mapping exceeds interface limit");
    }
    let mut interface_names = HashSet::new();
    let mut peer_count = 0usize;
    for interface in &mapping.interfaces {
        validate_interface_name(&interface.interface_name)?;
        if !interface_names.insert(interface.interface_name.as_str()) {
            bail!("WireGuard session mapping contains duplicate interfaces");
        }
        let mut public_keys = HashSet::new();
        for peer in &interface.peers {
            peer_count += 1;
            if peer_count > MAX_PEERS {
                bail!("WireGuard session mapping exceeds peer limit");
            }
            validate_public_key(&peer.public_key)?;
            if !public_keys.insert(peer.public_key.as_str()) {
                bail!("WireGuard session mapping contains duplicate peer keys");
            }
            if peer.runtime_username.trim().is_empty() || peer.runtime_username.len() > 128 {
                bail!("WireGuard session mapping contains invalid runtime username");
            }
            if peer.device_fingerprint.trim().is_empty() || peer.device_fingerprint.len() > 256 {
                bail!("WireGuard session mapping contains invalid device fingerprint");
            }
        }
    }
    Ok(())
}

fn validate_interface_name(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 64
        || value.starts_with('-')
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "_=+.-".contains(character))
    {
        bail!("invalid WireGuard interface name");
    }
    Ok(())
}

fn validate_public_key(value: &str) -> Result<()> {
    if value.len() != 44
        || !value.ends_with('=')
        || !value[..43].chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '+' || character == '/'
        })
    {
        bail!("invalid WireGuard public key");
    }
    Ok(())
}

fn validate_executable(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        bail!("wg binary path must be absolute");
    }
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("failed to stat wg binary {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        bail!("wg binary must be a regular non-symlink file");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = metadata.permissions().mode();
        if mode & 0o111 == 0 || mode & 0o022 != 0 {
            bail!("wg binary must be executable and not group/world-writable");
        }
    }
    Ok(())
}

async fn read_request() -> Result<SubscriptionSessionRuntimeDriverRequest> {
    let mut input = Vec::new();
    tokio::io::stdin()
        .take(16_384)
        .read_to_end(&mut input)
        .await
        .context("failed to read WireGuard session driver request")?;
    if input.len() >= 16_384 {
        bail!("WireGuard session driver request exceeds byte limit");
    }
    serde_json::from_slice(&input).context("failed to decode WireGuard session driver request")
}

fn parse_operation_arg() -> Result<SubscriptionSessionRuntimeDriverOperation> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 2 || args[0] != "--operation" {
        bail!("expected exactly --operation <operation>");
    }
    match args[1].as_str() {
        "handshake" => Ok(SubscriptionSessionRuntimeDriverOperation::Handshake),
        "observe" => Ok(SubscriptionSessionRuntimeDriverOperation::Observe),
        "terminate" => Ok(SubscriptionSessionRuntimeDriverOperation::Terminate),
        "verify" => Ok(SubscriptionSessionRuntimeDriverOperation::Verify),
        _ => bail!("unsupported WireGuard session driver operation"),
    }
}

fn parse_bounded_env_u64(name: &str, default: u64, min: u64, max: u64) -> Result<u64> {
    let value = std::env::var(name)
        .ok()
        .map(|value| {
            value
                .parse::<u64>()
                .with_context(|| format!("{name} must be an integer"))
        })
        .transpose()?
        .unwrap_or(default);
    if !(min..=max).contains(&value) {
        bail!("{name} must be between {min} and {max}");
    }
    Ok(value)
}

fn parse_bounded_env_usize(name: &str, default: usize, min: usize, max: usize) -> Result<usize> {
    let value = std::env::var(name)
        .ok()
        .map(|value| {
            value
                .parse::<usize>()
                .with_context(|| format!("{name} must be an integer"))
        })
        .transpose()?
        .unwrap_or(default);
    if !(min..=max).contains(&value) {
        bail!("{name} must be between {min} and {max}");
    }
    Ok(value)
}

fn constant_time_slice_eq(left: &[u8], right: &[u8]) -> bool {
    let max_len = left.len().max(right.len());
    let mut diff = left.len() ^ right.len();
    for index in 0..max_len {
        let left = left.get(index).copied().unwrap_or(0);
        let right = right.get(index).copied().unwrap_or(0);
        diff |= usize::from(left ^ right);
    }
    diff == 0
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn config() -> DriverConfig {
        DriverConfig {
            wg_binary_path: PathBuf::from("/usr/bin/wg"),
            mapping_path: PathBuf::from("mapping.json"),
            session_ref_key: vec![7; 32],
            active_within_seconds: 180,
            command_timeout: Duration::from_secs(1),
            max_command_output_bytes: 16_384,
        }
    }

    fn mapping_peer() -> WireGuardSessionPeerMapping {
        WireGuardSessionPeerMapping {
            runtime_username: "catalog/client-a".to_string(),
            public_key: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".to_string(),
            device_fingerprint: "wireguard-sha256:device-a".to_string(),
        }
    }

    #[test]
    fn dump_parser_keeps_only_recent_peers_and_attaches_mapping() {
        let peer = mapping_peer();
        let mapped = HashMap::from([(peer.public_key.as_str(), &peer)]);
        let dump = format!(
            "private\tpublic\t51820\toff\n{}\t(none)\t198.51.100.10:40000\t10.0.0.2/32\t950\t1\t2\t25\nBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB=\t(none)\t(none)\t10.0.0.3/32\t1\t1\t2\t25\n",
            peer.public_key
        );

        let peers = parse_dump("wg0", dump.as_bytes(), &mapped, 180, 1_000).unwrap();

        assert_eq!(peers.len(), 1);
        assert_eq!(
            peers[0].mapping.as_ref().unwrap().runtime_username,
            "catalog/client-a"
        );
    }

    #[test]
    fn opaque_ids_are_domain_separated_and_stable() {
        let config = config();
        let peer = LivePeer {
            interface_name: "wg0".to_string(),
            public_key: mapping_peer().public_key,
            endpoint: None,
            latest_handshake_at_unix: 1,
            mapping: None,
        };

        assert_eq!(session_id(&config, &peer), session_id(&config, &peer));
        assert_ne!(session_id(&config, &peer), runtime_ref(&config, &peer));
        assert!(!runtime_ref(&config, &peer).contains(&peer.public_key));
    }

    #[test]
    fn mapping_rejects_duplicate_interfaces_and_peer_keys() {
        let peer = mapping_peer();
        let mapping = WireGuardSessionMappingDocument {
            schema_version: 1,
            source_revision: "rev-a".to_string(),
            created_at_unix: 1,
            interfaces: vec![node_domain::WireGuardSessionInterfaceMapping {
                interface_name: "wg0".to_string(),
                peers: vec![peer.clone(), peer],
            }],
        };

        assert!(validate_mapping(&mapping).is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn fake_wg_completes_exact_peer_termination_and_verification() {
        use std::os::unix::fs::PermissionsExt;

        let script_path = temp_path("fake-wg").with_extension("sh");
        let state_path = temp_path("fake-wg-state");
        let mapping_path = temp_path("fake-wg-mapping").with_extension("json");
        let public_key = mapping_peer().public_key;
        let now = now_unix();
        let script = format!(
            r#"#!/bin/sh
state="{state}"
if [ "$1" = "show" ]; then
  printf 'private\tpublic\t51820\toff\n'
  if [ ! -f "$state" ]; then
    printf '{public_key}\t(none)\t198.51.100.10:40000\t10.0.0.2/32\t{now}\t1\t2\t25\n'
  fi
  exit 0
fi
if [ "$1" = "set" ] && [ "$3" = "peer" ] && [ "$5" = "remove" ]; then
  : > "$state"
  exit 0
fi
exit 1
"#,
            state = state_path.display(),
        );
        fs::write(&script_path, script).unwrap();
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700)).unwrap();
        let mapping = WireGuardSessionMappingDocument {
            schema_version: 1,
            source_revision: "rev-a".to_string(),
            created_at_unix: now,
            interfaces: vec![node_domain::WireGuardSessionInterfaceMapping {
                interface_name: "wg0".to_string(),
                peers: vec![mapping_peer()],
            }],
        };
        fs::write(&mapping_path, serde_json::to_vec(&mapping).unwrap()).unwrap();
        fs::set_permissions(&mapping_path, fs::Permissions::from_mode(0o600)).unwrap();
        let config = DriverConfig {
            wg_binary_path: script_path.clone(),
            mapping_path: mapping_path.clone(),
            ..config()
        };

        let observed = execute(
            &config,
            request(
                SubscriptionSessionRuntimeDriverOperation::Observe,
                None,
                None,
            ),
        )
        .await
        .unwrap();
        assert_eq!(observed.observations.len(), 1);
        let observation = &observed.observations[0];
        execute(
            &config,
            request(
                SubscriptionSessionRuntimeDriverOperation::Terminate,
                Some(observation.session_id.clone()),
                observation.runtime_session_ref.clone(),
            ),
        )
        .await
        .unwrap();
        let verified = execute(
            &config,
            request(
                SubscriptionSessionRuntimeDriverOperation::Verify,
                Some(observation.session_id.clone()),
                observation.runtime_session_ref.clone(),
            ),
        )
        .await
        .unwrap();

        fs::remove_file(script_path).ok();
        fs::remove_file(state_path).ok();
        fs::remove_file(mapping_path).ok();
        assert_eq!(verified.session_absent, Some(true));
    }

    fn request(
        operation: SubscriptionSessionRuntimeDriverOperation,
        session_id: Option<String>,
        runtime_session_ref: Option<String>,
    ) -> SubscriptionSessionRuntimeDriverRequest {
        SubscriptionSessionRuntimeDriverRequest {
            protocol_version: SUBSCRIPTION_SESSION_RUNTIME_DRIVER_PROTOCOL_VERSION,
            operation,
            session_id,
            runtime_session_ref,
        }
    }

    fn temp_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "hydra-wireguard-session-driver-{name}-{}-{nanos}",
            std::process::id()
        ))
    }
}

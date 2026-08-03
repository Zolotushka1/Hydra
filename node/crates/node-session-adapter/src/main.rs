use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use node_domain::{
    CompleteLocalSubscriptionSessionEnforcementRequest, LocalSubscriptionSessionEnforcementCommand,
    RegisterLocalSubscriptionSessionAdapterRequest, ReportSubscriptionSessionsRequest,
    SUBSCRIPTION_SESSION_RUNTIME_DRIVER_PROTOCOL_VERSION, SubscriptionSessionEnforcementAction,
    SubscriptionSessionEnforcementStatus, SubscriptionSessionObservation,
    SubscriptionSessionObservationSource, SubscriptionSessionRuntimeCapability,
    SubscriptionSessionRuntimeDriverOperation, SubscriptionSessionRuntimeDriverRequest,
    SubscriptionSessionRuntimeDriverResponse,
};
use node_session_adapter_client::{SessionAdapterClient, SessionAdapterClientConfig};
use tokio::{io::AsyncReadExt, io::AsyncWriteExt, process::Command};
use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

const DEFAULT_DRIVER_TIMEOUT_SECONDS: u64 = 10;
const DEFAULT_DRIVER_MAX_OUTPUT_BYTES: usize = 1_048_576;

#[derive(Debug, Clone)]
struct AdapterConfig {
    node_local_api_url: String,
    adapter_token: String,
    adapter_instance_id: String,
    poll_interval_seconds: u64,
    dry_run_observation_only: bool,
    snapshot_path: Option<PathBuf>,
    max_snapshot_bytes: u64,
    max_snapshot_observations: usize,
    snapshot_stability_millis: u64,
    runtime_driver: Option<RuntimeDriver>,
}

impl AdapterConfig {
    fn from_env() -> Result<Self> {
        let node_local_api_url = std::env::var("HYDRA_NODE_LOCAL_API_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:8081".to_string());
        let adapter_token = std::env::var("HYDRA_NODE_SESSION_ADAPTER_TOKEN")
            .context("HYDRA_NODE_SESSION_ADAPTER_TOKEN is required")?;
        let adapter_instance_id = std::env::var("HYDRA_NODE_SESSION_ADAPTER_INSTANCE_ID")
            .unwrap_or_else(|_| default_adapter_instance_id());
        let poll_interval_seconds =
            std::env::var("HYDRA_NODE_SESSION_ADAPTER_POLL_INTERVAL_SECONDS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(15);
        let dry_run_observation_only =
            std::env::var("HYDRA_NODE_SESSION_ADAPTER_DRY_RUN_OBSERVATION_ONLY")
                .ok()
                .map(|value| parse_bool_default_true(&value))
                .unwrap_or(true);
        let snapshot_path = std::env::var("HYDRA_NODE_SESSION_ADAPTER_SNAPSHOT_PATH")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from);
        let max_snapshot_bytes = std::env::var("HYDRA_NODE_SESSION_ADAPTER_MAX_SNAPSHOT_BYTES")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(1_048_576);
        let max_snapshot_observations =
            std::env::var("HYDRA_NODE_SESSION_ADAPTER_MAX_SNAPSHOT_OBSERVATIONS")
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(2_048);
        let snapshot_stability_millis =
            std::env::var("HYDRA_NODE_SESSION_ADAPTER_SNAPSHOT_STABILITY_MILLIS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(100);
        let runtime_driver = (!dry_run_observation_only)
            .then(RuntimeDriver::from_env)
            .transpose()?;

        Ok(Self {
            node_local_api_url,
            adapter_token,
            adapter_instance_id,
            poll_interval_seconds,
            dry_run_observation_only,
            snapshot_path,
            max_snapshot_bytes,
            max_snapshot_observations,
            snapshot_stability_millis,
            runtime_driver,
        })
    }
}

#[derive(Debug, Clone)]
struct RuntimeDriver {
    executable_path: PathBuf,
    arguments: Vec<String>,
    timeout: Duration,
    max_output_bytes: usize,
}

fn driver_operation_arg(operation: SubscriptionSessionRuntimeDriverOperation) -> &'static str {
    match operation {
        SubscriptionSessionRuntimeDriverOperation::Handshake => "handshake",
        SubscriptionSessionRuntimeDriverOperation::Observe => "observe",
        SubscriptionSessionRuntimeDriverOperation::Terminate => "terminate",
        SubscriptionSessionRuntimeDriverOperation::Verify => "verify",
    }
}

impl RuntimeDriver {
    fn from_env() -> Result<Self> {
        let executable_path = PathBuf::from(
            std::env::var("HYDRA_NODE_SESSION_ADAPTER_DRIVER_PATH")
                .context("HYDRA_NODE_SESSION_ADAPTER_DRIVER_PATH is required in exact mode")?,
        );
        validate_driver_executable(&executable_path)?;
        let arguments = std::env::var("HYDRA_NODE_SESSION_ADAPTER_DRIVER_ARGS_JSON")
            .ok()
            .map(|value| {
                serde_json::from_str::<Vec<String>>(&value)
                    .context("failed to decode session adapter driver args JSON")
            })
            .transpose()?
            .unwrap_or_default();
        validate_driver_arguments(&arguments)?;
        let timeout_seconds = parse_bounded_env_u64(
            "HYDRA_NODE_SESSION_ADAPTER_DRIVER_TIMEOUT_SECONDS",
            DEFAULT_DRIVER_TIMEOUT_SECONDS,
            1,
            60,
        )?;
        let max_output_bytes = parse_bounded_env_usize(
            "HYDRA_NODE_SESSION_ADAPTER_DRIVER_MAX_OUTPUT_BYTES",
            DEFAULT_DRIVER_MAX_OUTPUT_BYTES,
            4_096,
            1_048_576,
        )?;
        Ok(Self {
            executable_path,
            arguments,
            timeout: Duration::from_secs(timeout_seconds),
            max_output_bytes,
        })
    }

    async fn invoke(
        &self,
        operation: SubscriptionSessionRuntimeDriverOperation,
        session_id: Option<&str>,
        runtime_session_ref: Option<&str>,
    ) -> Result<SubscriptionSessionRuntimeDriverResponse> {
        let request = serde_json::to_vec(&SubscriptionSessionRuntimeDriverRequest {
            protocol_version: SUBSCRIPTION_SESSION_RUNTIME_DRIVER_PROTOCOL_VERSION,
            operation,
            session_id: session_id.map(str::to_string),
            runtime_session_ref: runtime_session_ref.map(str::to_string),
        })
        .context("failed to encode session runtime driver request")?;
        let mut child = Command::new(&self.executable_path)
            .args(&self.arguments)
            .arg("--operation")
            .arg(driver_operation_arg(operation))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .context("failed to start session runtime driver")?;
        let mut stdin = child
            .stdin
            .take()
            .context("session runtime driver stdin is unavailable")?;
        stdin
            .write_all(&request)
            .await
            .context("failed to write session runtime driver request")?;
        stdin
            .shutdown()
            .await
            .context("failed to close session runtime driver request")?;
        drop(stdin);
        let stdout = child
            .stdout
            .take()
            .context("session runtime driver stdout is unavailable")?;
        let stderr = child
            .stderr
            .take()
            .context("session runtime driver stderr is unavailable")?;
        let execution = async {
            let (status, stdout, stderr) = tokio::try_join!(
                async {
                    child
                        .wait()
                        .await
                        .context("failed to wait for session runtime driver")
                },
                read_bounded_output(stdout, self.max_output_bytes),
                read_bounded_output(stderr, self.max_output_bytes),
            )?;
            Ok::<_, anyhow::Error>((status, stdout, stderr))
        };
        let (status, stdout, _stderr) = tokio::time::timeout(self.timeout, execution)
            .await
            .context("session runtime driver timed out")??;
        if !status.success() {
            bail!("session runtime driver exited unsuccessfully");
        }
        let response = serde_json::from_slice::<SubscriptionSessionRuntimeDriverResponse>(&stdout)
            .context("failed to decode session runtime driver response")?;
        if response.protocol_version != SUBSCRIPTION_SESSION_RUNTIME_DRIVER_PROTOCOL_VERSION {
            bail!("session runtime driver protocol version mismatch");
        }
        if !response.success {
            bail!("session runtime driver rejected the operation");
        }
        Ok(response)
    }

    async fn handshake(&self) -> Result<()> {
        let response = self
            .invoke(
                SubscriptionSessionRuntimeDriverOperation::Handshake,
                None,
                None,
            )
            .await?;
        if !exact_runtime_capabilities(&response.runtime_capabilities) {
            bail!("session runtime driver did not declare the complete exact capability set");
        }
        Ok(())
    }

    async fn observe(&self, max_observations: usize) -> Result<ReportSubscriptionSessionsRequest> {
        let response = self
            .invoke(
                SubscriptionSessionRuntimeDriverOperation::Observe,
                None,
                None,
            )
            .await?;
        if !exact_runtime_capabilities(&response.runtime_capabilities) {
            bail!("session runtime driver observation omitted exact capabilities");
        }
        let snapshot = ReportSubscriptionSessionsRequest {
            observation_source: SubscriptionSessionObservationSource::NodeManagedRuntimeTable,
            runtime_capabilities: exact_capabilities(),
            observations: response.observations,
        };
        validate_exact_snapshot(&snapshot, max_observations)?;
        Ok(snapshot)
    }

    async fn terminate(&self, command: &LocalSubscriptionSessionEnforcementCommand) -> Result<()> {
        let response = self
            .invoke(
                SubscriptionSessionRuntimeDriverOperation::Terminate,
                Some(&command.session_id),
                Some(&command.runtime_session_ref),
            )
            .await?;
        if response.session_absent == Some(false) {
            bail!("session runtime driver reported that the target remains active");
        }
        Ok(())
    }

    async fn verify_absent(
        &self,
        command: &LocalSubscriptionSessionEnforcementCommand,
    ) -> Result<u64> {
        let response = self
            .invoke(
                SubscriptionSessionRuntimeDriverOperation::Verify,
                Some(&command.session_id),
                Some(&command.runtime_session_ref),
            )
            .await?;
        if response.session_absent != Some(true) {
            bail!("session runtime driver did not prove exact target absence");
        }
        let verified_at_unix = response
            .verified_at_unix
            .context("session runtime driver omitted absence verification timestamp")?;
        if verified_at_unix < command.issued_at_unix {
            bail!("session runtime driver returned stale absence verification");
        }
        if verified_at_unix > now_unix().saturating_add(300) {
            bail!("session runtime driver returned an invalid verification timestamp");
        }
        Ok(verified_at_unix)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let config = AdapterConfig::from_env()?;
    let client = SessionAdapterClient::new(SessionAdapterClientConfig {
        node_local_api_url: config.node_local_api_url.clone(),
        adapter_token: config.adapter_token.clone(),
        adapter_instance_id: config.adapter_instance_id.clone(),
    })?;

    info!(
        node_local_api_url = %config.node_local_api_url,
        adapter_instance_id = %config.adapter_instance_id,
        dry_run_observation_only = config.dry_run_observation_only,
        "hydra session adapter started"
    );

    run_loop(client, config).await
}

async fn run_loop(client: SessionAdapterClient, config: AdapterConfig) -> Result<()> {
    let interval = Duration::from_secs(config.poll_interval_seconds.max(1));
    loop {
        if let Err(error) = tick(&client, &config).await {
            warn!(error = %error, "session adapter tick failed");
        }
        tokio::time::sleep(interval).await;
    }
}

async fn tick(client: &SessionAdapterClient, config: &AdapterConfig) -> Result<()> {
    if let Some(driver) = config.runtime_driver.as_ref() {
        return tick_exact(client, config, driver).await;
    }

    let lease = client
        .register(RegisterLocalSubscriptionSessionAdapterRequest {
            adapter_instance_id: client.adapter_instance_id().to_string(),
            runtime_capabilities: Vec::new(),
        })
        .await?;

    let snapshot = load_observation_only_snapshot(config).await?;
    let observed_count = snapshot.observations.len();
    let view = client.submit_observations(snapshot).await?;

    let actions = client.pending_actions().await?;
    if !actions.is_empty() {
        warn!(
            pending_action_count = actions.len(),
            "dry-run observation-only adapter received actions; node should fail unsupported actions"
        );
    }

    info!(
        lease_expires_at_unix = lease.lease_expires_at_unix,
        observed_count,
        buffered_observation_count = view.buffered_observation_count,
        pending_action_count = actions.len(),
        "session adapter dry-run tick completed"
    );
    Ok(())
}

async fn tick_exact(
    client: &SessionAdapterClient,
    config: &AdapterConfig,
    driver: &RuntimeDriver,
) -> Result<()> {
    driver.handshake().await?;
    let capabilities = exact_capabilities();
    let mut snapshot = driver.observe(config.max_snapshot_observations).await?;
    let lease = client
        .register(RegisterLocalSubscriptionSessionAdapterRequest {
            adapter_instance_id: client.adapter_instance_id().to_string(),
            runtime_capabilities: capabilities.clone(),
        })
        .await?;
    let observed_count = snapshot.observations.len();
    let view = client.submit_observations(snapshot.clone()).await?;
    let actions = client.pending_actions().await?;
    let pending_action_count = actions.len();
    let mut applied_count = 0usize;
    let mut failed_count = 0usize;

    for action in actions {
        match execute_exact_action(driver, &snapshot, &action, config.max_snapshot_observations)
            .await
        {
            Ok((completion, refreshed_snapshot)) => {
                client
                    .complete_action(&action.action_id, completion)
                    .await?;
                snapshot = refreshed_snapshot;
                client.submit_observations(snapshot.clone()).await?;
                applied_count += 1;
            }
            Err(error) => {
                let detail = safe_failure_detail(&error, &action.runtime_session_ref);
                client
                    .complete_action(
                        &action.action_id,
                        CompleteLocalSubscriptionSessionEnforcementRequest {
                            status: SubscriptionSessionEnforcementStatus::Failed,
                            runtime_session_ref: None,
                            session_absent_after_action: None,
                            verified_at_unix: None,
                            detail: Some(detail),
                        },
                    )
                    .await?;
                failed_count += 1;
            }
        }
    }

    info!(
        lease_expires_at_unix = lease.lease_expires_at_unix,
        observed_count,
        buffered_observation_count = view.buffered_observation_count,
        pending_action_count,
        applied_count,
        failed_count,
        "session adapter exact-enforcement tick completed"
    );
    Ok(())
}

async fn execute_exact_action(
    driver: &RuntimeDriver,
    snapshot: &ReportSubscriptionSessionsRequest,
    command: &LocalSubscriptionSessionEnforcementCommand,
    max_observations: usize,
) -> Result<(
    CompleteLocalSubscriptionSessionEnforcementRequest,
    ReportSubscriptionSessionsRequest,
)> {
    if command.action != SubscriptionSessionEnforcementAction::TerminateSession {
        bail!("session runtime driver received an unsupported action");
    }
    if !command.requires_absence_verification {
        bail!("exact session action omitted required absence verification");
    }
    if command.expires_at_unix < now_unix() {
        bail!("exact session action expired before execution");
    }
    let matching_observation = snapshot.observations.iter().find(|observation| {
        observation.session_id == command.session_id
            && observation
                .runtime_session_ref
                .as_deref()
                .is_some_and(|value| {
                    constant_time_slice_eq(value.as_bytes(), command.runtime_session_ref.as_bytes())
                })
    });
    if matching_observation.is_none() {
        bail!("exact session action is not present in the latest trusted runtime table");
    }

    driver.terminate(command).await?;
    if command.expires_at_unix < now_unix() {
        bail!("exact session action expired after targeted termination");
    }
    let verified_at_unix = driver.verify_absent(command).await?;
    if command.expires_at_unix < now_unix() {
        bail!("exact session action expired after absence verification");
    }
    let refreshed_snapshot = driver.observe(max_observations).await?;
    if command.expires_at_unix < now_unix() {
        bail!("exact session action expired before completion");
    }
    let target_still_present = refreshed_snapshot.observations.iter().any(|observation| {
        observation
            .runtime_session_ref
            .as_deref()
            .is_some_and(|value| {
                constant_time_slice_eq(value.as_bytes(), command.runtime_session_ref.as_bytes())
            })
    });
    if target_still_present {
        bail!("exact target remains present in the refreshed trusted runtime table");
    }
    Ok((
        CompleteLocalSubscriptionSessionEnforcementRequest {
            status: SubscriptionSessionEnforcementStatus::Applied,
            runtime_session_ref: Some(command.runtime_session_ref.clone()),
            session_absent_after_action: Some(true),
            verified_at_unix: Some(verified_at_unix),
            detail: Some(
                "exact runtime session terminated and absence verified by refreshed runtime table"
                    .to_string(),
            ),
        },
        refreshed_snapshot,
    ))
}

async fn load_observation_only_snapshot(
    config: &AdapterConfig,
) -> Result<ReportSubscriptionSessionsRequest> {
    let Some(path) = config.snapshot_path.as_ref() else {
        return Ok(empty_observation_snapshot());
    };
    let Some(metadata) = read_snapshot_metadata(path)? else {
        return Ok(empty_observation_snapshot());
    };
    validate_snapshot_metadata(path, &metadata, config.max_snapshot_bytes)?;
    if config.snapshot_stability_millis > 0 {
        tokio::time::sleep(Duration::from_millis(config.snapshot_stability_millis)).await;
        let Some(stable_metadata) = read_snapshot_metadata(path)? else {
            bail!(
                "session adapter snapshot disappeared during stability check: {}",
                path.display()
            );
        };
        validate_snapshot_metadata(path, &stable_metadata, config.max_snapshot_bytes)?;
        if metadata_changed(&metadata, &stable_metadata) {
            bail!(
                "session adapter snapshot changed during stability check: {}",
                path.display()
            );
        }
    }
    let data = fs::read(path)
        .with_context(|| format!("failed to read session adapter snapshot {}", path.display()))?;
    let Some(after_read_metadata) = read_snapshot_metadata(path)? else {
        bail!(
            "session adapter snapshot disappeared after read: {}",
            path.display()
        );
    };
    validate_snapshot_metadata(path, &after_read_metadata, config.max_snapshot_bytes)?;
    if metadata_changed(&metadata, &after_read_metadata) {
        bail!(
            "session adapter snapshot changed while being read: {}",
            path.display()
        );
    }
    let snapshot = serde_json::from_slice::<ReportSubscriptionSessionsRequest>(&data)
        .context("failed to decode session adapter snapshot JSON")?;
    validate_observation_only_snapshot(&snapshot, config.max_snapshot_observations)?;
    Ok(snapshot)
}

fn read_snapshot_metadata(path: &Path) -> Result<Option<fs::Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error)
            .with_context(|| format!("failed to stat session adapter snapshot {}", path.display())),
    }
}

fn validate_snapshot_metadata(
    path: &Path,
    metadata: &fs::Metadata,
    max_snapshot_bytes: u64,
) -> Result<()> {
    if !metadata.file_type().is_file() {
        bail!(
            "session adapter snapshot path is not a regular file: {}",
            path.display()
        );
    }
    if metadata.len() > max_snapshot_bytes {
        bail!("session adapter snapshot exceeds configured byte limit");
    }
    Ok(())
}

fn metadata_changed(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    if left.len() != right.len() {
        return true;
    }
    match (left.modified(), right.modified()) {
        (Ok(left_modified), Ok(right_modified)) => left_modified != right_modified,
        _ => false,
    }
}

fn empty_observation_snapshot() -> ReportSubscriptionSessionsRequest {
    ReportSubscriptionSessionsRequest {
        observation_source: SubscriptionSessionObservationSource::NodeManagedRuntimeTable,
        runtime_capabilities: Vec::new(),
        observations: Vec::new(),
    }
}

fn validate_observation_only_snapshot(
    snapshot: &ReportSubscriptionSessionsRequest,
    max_observations: usize,
) -> Result<()> {
    if snapshot.observation_source != SubscriptionSessionObservationSource::NodeManagedRuntimeTable
    {
        bail!("session adapter snapshot has unsupported observation source");
    }
    if !snapshot.runtime_capabilities.is_empty() {
        bail!("dry-run session adapter snapshot must not declare runtime capabilities");
    }
    if snapshot.observations.len() > max_observations {
        bail!("session adapter snapshot exceeds configured observation limit");
    }
    for observation in &snapshot.observations {
        validate_common_observation(observation)?;
        if observation.runtime_session_ref.is_some() {
            bail!("dry-run observation-only snapshot must not contain runtime_session_ref");
        }
    }
    Ok(())
}

fn validate_exact_snapshot(
    snapshot: &ReportSubscriptionSessionsRequest,
    max_observations: usize,
) -> Result<()> {
    if snapshot.observation_source != SubscriptionSessionObservationSource::NodeManagedRuntimeTable
    {
        bail!("session runtime driver returned an unsupported observation source");
    }
    if !exact_runtime_capabilities(&snapshot.runtime_capabilities) {
        bail!("exact runtime snapshot omitted required capabilities");
    }
    if snapshot.observations.len() > max_observations {
        bail!("session runtime driver snapshot exceeds configured observation limit");
    }
    let mut session_ids = HashSet::with_capacity(snapshot.observations.len());
    let mut runtime_refs = HashSet::with_capacity(snapshot.observations.len());
    for observation in &snapshot.observations {
        validate_common_observation(observation)?;
        let runtime_ref = observation
            .runtime_session_ref
            .as_deref()
            .context("exact runtime observation omitted opaque session reference")?;
        if runtime_ref.trim().is_empty() || runtime_ref.len() > 256 {
            bail!("exact runtime observation contains an invalid opaque session reference");
        }
        if !session_ids.insert(observation.session_id.as_str()) {
            bail!("session runtime driver returned duplicate session ids");
        }
        if !runtime_refs.insert(runtime_ref) {
            bail!("session runtime driver returned duplicate opaque session references");
        }
    }
    Ok(())
}

fn validate_common_observation(observation: &SubscriptionSessionObservation) -> Result<()> {
    if observation.session_id.trim().is_empty() || observation.session_id.len() > 128 {
        bail!("session adapter snapshot contains invalid session_id");
    }
    if observation.runtime_username.trim().is_empty() || observation.runtime_username.len() > 128 {
        bail!("session adapter snapshot contains invalid runtime_username");
    }
    if observation
        .device_fingerprint
        .as_ref()
        .is_some_and(|value| value.trim().is_empty() || value.len() > 256)
    {
        bail!("session adapter snapshot contains invalid device_fingerprint");
    }
    if observation
        .source_ip
        .as_ref()
        .is_some_and(|value| value.trim().is_empty() || value.len() > 128)
    {
        bail!("session adapter snapshot contains invalid source_ip");
    }
    Ok(())
}

fn exact_capabilities() -> Vec<SubscriptionSessionRuntimeCapability> {
    vec![
        SubscriptionSessionRuntimeCapability::OpaqueSessionReference,
        SubscriptionSessionRuntimeCapability::ExactSessionTermination,
        SubscriptionSessionRuntimeCapability::PostActionAbsenceVerification,
    ]
}

fn exact_runtime_capabilities(capabilities: &[SubscriptionSessionRuntimeCapability]) -> bool {
    capabilities.len() == 3
        && capabilities.contains(&SubscriptionSessionRuntimeCapability::OpaqueSessionReference)
        && capabilities.contains(&SubscriptionSessionRuntimeCapability::ExactSessionTermination)
        && capabilities
            .contains(&SubscriptionSessionRuntimeCapability::PostActionAbsenceVerification)
}

fn validate_driver_executable(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        bail!("session runtime driver path must be absolute");
    }
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("failed to stat session runtime driver {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        bail!("session runtime driver must be a regular non-symlink file");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = metadata.permissions().mode();
        if mode & 0o111 == 0 {
            bail!("session runtime driver is not executable");
        }
        if mode & 0o022 != 0 {
            bail!("session runtime driver must not be writable by group or others");
        }
    }
    Ok(())
}

fn validate_driver_arguments(arguments: &[String]) -> Result<()> {
    if arguments.len() > 16 {
        bail!("session runtime driver has too many static arguments");
    }
    if arguments
        .iter()
        .any(|argument| argument.len() > 256 || argument.contains('\0'))
    {
        bail!("session runtime driver contains an invalid static argument");
    }
    Ok(())
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

async fn read_bounded_output(
    mut reader: impl tokio::io::AsyncRead + Unpin,
    max_bytes: usize,
) -> Result<Vec<u8>> {
    let mut output = Vec::with_capacity(max_bytes.min(16_384));
    let mut buffer = [0u8; 8_192];
    loop {
        let read = reader
            .read(&mut buffer)
            .await
            .context("failed to read session runtime driver output")?;
        if read == 0 {
            break;
        }
        if output.len().saturating_add(read) > max_bytes {
            bail!("session runtime driver output exceeds configured limit");
        }
        output.extend_from_slice(&buffer[..read]);
    }
    Ok(output)
}

fn safe_failure_detail(error: &anyhow::Error, runtime_session_ref: &str) -> String {
    let detail = error
        .to_string()
        .replace(runtime_session_ref, "[redacted-session-ref]")
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    detail.chars().take(512).collect()
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

fn default_adapter_instance_id() -> String {
    format!(
        "session-adapter-{}-{}",
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

fn parse_bool_default_true(value: &str) -> bool {
    value != "0" && !value.eq_ignore_ascii_case("false")
}

fn init_tracing() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "node_session_adapter=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();
}

#[cfg(test)]
mod tests {
    use super::*;
    use node_domain::SubscriptionSessionObservation;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn dry_run_flag_parser_is_safe_by_default() {
        assert!(!parse_bool_default_true("0"));
        assert!(!parse_bool_default_true("false"));
        assert!(parse_bool_default_true("true"));
        assert!(parse_bool_default_true(""));
    }

    #[test]
    fn default_instance_id_is_bounded() {
        let id = default_adapter_instance_id();
        assert!(!id.is_empty());
        assert!(id.len() <= 128);
    }

    #[test]
    fn observation_only_snapshot_accepts_bounded_observations() {
        let snapshot = ReportSubscriptionSessionsRequest {
            observation_source: SubscriptionSessionObservationSource::NodeManagedRuntimeTable,
            runtime_capabilities: Vec::new(),
            observations: vec![SubscriptionSessionObservation {
                session_id: "session-a".to_string(),
                runtime_username: "catalog/client-a".to_string(),
                runtime_session_ref: None,
                device_fingerprint: Some("device-a".to_string()),
                source_ip: Some("198.51.100.10".to_string()),
                connected_at_unix: Some(1),
            }],
        };

        validate_observation_only_snapshot(&snapshot, 4).unwrap();
    }

    #[test]
    fn observation_only_snapshot_rejects_exact_runtime_handles() {
        let snapshot = ReportSubscriptionSessionsRequest {
            observation_source: SubscriptionSessionObservationSource::NodeManagedRuntimeTable,
            runtime_capabilities: Vec::new(),
            observations: vec![SubscriptionSessionObservation {
                session_id: "session-a".to_string(),
                runtime_username: "catalog/client-a".to_string(),
                runtime_session_ref: Some("opaque-ref-a".to_string()),
                device_fingerprint: None,
                source_ip: None,
                connected_at_unix: None,
            }],
        };

        assert!(validate_observation_only_snapshot(&snapshot, 4).is_err());
    }

    #[test]
    fn exact_snapshot_requires_unique_opaque_runtime_handles() {
        let snapshot = ReportSubscriptionSessionsRequest {
            observation_source: SubscriptionSessionObservationSource::NodeManagedRuntimeTable,
            runtime_capabilities: exact_capabilities(),
            observations: vec![
                exact_observation("session-a", "opaque-ref-a"),
                exact_observation("session-b", "opaque-ref-a"),
            ],
        };

        assert!(validate_exact_snapshot(&snapshot, 4).is_err());
    }

    #[test]
    fn exact_snapshot_accepts_complete_bounded_runtime_table() {
        let snapshot = ReportSubscriptionSessionsRequest {
            observation_source: SubscriptionSessionObservationSource::NodeManagedRuntimeTable,
            runtime_capabilities: exact_capabilities(),
            observations: vec![
                exact_observation("session-a", "opaque-ref-a"),
                exact_observation("session-b", "opaque-ref-b"),
            ],
        };

        validate_exact_snapshot(&snapshot, 4).unwrap();
    }

    #[test]
    fn failure_detail_redacts_runtime_handle_and_control_characters() {
        let error = anyhow::anyhow!("driver failed for secret-ref\nwith detail");

        let detail = safe_failure_detail(&error, "secret-ref");

        assert!(!detail.contains("secret-ref"));
        assert!(!detail.contains('\n'));
        assert!(detail.contains("[redacted-session-ref]"));
    }

    #[tokio::test]
    async fn bounded_driver_output_rejects_oversized_stream() {
        let (mut writer, reader) = tokio::io::duplex(128);
        let writer_task = tokio::spawn(async move {
            writer.write_all(&[b'x'; 65]).await.unwrap();
        });

        let result = read_bounded_output(reader, 64).await;

        writer_task.await.unwrap();
        assert!(result.is_err());
    }

    #[cfg(unix)]
    #[test]
    fn driver_path_rejects_group_or_world_writable_executable() {
        use std::os::unix::fs::PermissionsExt;

        let path = temp_snapshot_path("unsafe-driver").with_extension("sh");
        fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o722)).unwrap();

        let result = validate_driver_executable(&path);

        fs::remove_file(path).ok();
        assert!(result.is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn executable_driver_completes_targeted_termination_with_absence_proof() {
        use std::os::unix::fs::PermissionsExt;

        let script_path = temp_snapshot_path("exact-driver").with_extension("sh");
        let state_path = temp_snapshot_path("exact-driver-state");
        let script = r#"#!/bin/sh
state="$1"
operation="$3"
cat >/dev/null
capabilities='["opaque_session_reference","exact_session_termination","post_action_absence_verification"]'
case "$operation" in
  handshake)
    printf '{"protocol_version":1,"success":true,"runtime_capabilities":%s,"observations":[]}' "$capabilities"
    ;;
  observe)
    if [ -f "$state" ]; then
      printf '{"protocol_version":1,"success":true,"runtime_capabilities":%s,"observations":[]}' "$capabilities"
    else
      printf '{"protocol_version":1,"success":true,"runtime_capabilities":%s,"observations":[{"session_id":"session-a","runtime_username":"catalog/client-a","runtime_session_ref":"opaque-ref-a","device_fingerprint":null,"source_ip":null,"connected_at_unix":1}]}' "$capabilities"
    fi
    ;;
  terminate)
    : > "$state"
    printf '{"protocol_version":1,"success":true,"runtime_capabilities":[],"observations":[]}'
    ;;
  verify)
    if [ -f "$state" ]; then
      printf '{"protocol_version":1,"success":true,"runtime_capabilities":[],"observations":[],"session_absent":true,"verified_at_unix":1}'
    else
      exit 1
    fi
    ;;
  *) exit 1 ;;
esac
"#;
        fs::write(&script_path, script).unwrap();
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700)).unwrap();
        let driver = RuntimeDriver {
            executable_path: script_path.clone(),
            arguments: vec![state_path.to_string_lossy().into_owned()],
            timeout: Duration::from_secs(2),
            max_output_bytes: 16_384,
        };
        driver.handshake().await.unwrap();
        let snapshot = driver.observe(4).await.unwrap();
        let command = LocalSubscriptionSessionEnforcementCommand {
            action_id: "action-a".to_string(),
            session_id: "session-a".to_string(),
            action: SubscriptionSessionEnforcementAction::TerminateSession,
            runtime_session_ref: "opaque-ref-a".to_string(),
            reason: "device limit".to_string(),
            requires_absence_verification: true,
            issued_at_unix: 1,
            expires_at_unix: now_unix().saturating_add(30),
        };

        let (completion, refreshed) = execute_exact_action(&driver, &snapshot, &command, 4)
            .await
            .unwrap();

        fs::remove_file(script_path).ok();
        fs::remove_file(state_path).ok();
        assert_eq!(
            completion.status,
            SubscriptionSessionEnforcementStatus::Applied
        );
        assert_eq!(completion.session_absent_after_action, Some(true));
        assert!(refreshed.observations.is_empty());
    }

    #[tokio::test]
    async fn snapshot_loader_reads_valid_atomic_snapshot() {
        let path = temp_snapshot_path("valid");
        let snapshot = ReportSubscriptionSessionsRequest {
            observation_source: SubscriptionSessionObservationSource::NodeManagedRuntimeTable,
            runtime_capabilities: Vec::new(),
            observations: vec![SubscriptionSessionObservation {
                session_id: "session-a".to_string(),
                runtime_username: "catalog/client-a".to_string(),
                runtime_session_ref: None,
                device_fingerprint: None,
                source_ip: Some("203.0.113.10".to_string()),
                connected_at_unix: Some(42),
            }],
        };
        fs::write(&path, serde_json::to_vec(&snapshot).unwrap()).unwrap();

        let loaded = load_observation_only_snapshot(&test_config_for_path(path.clone())).await;

        fs::remove_file(path).ok();
        assert_eq!(loaded.unwrap().observations.len(), 1);
    }

    #[tokio::test]
    async fn missing_snapshot_file_returns_empty_snapshot() {
        let path = temp_snapshot_path("missing");
        fs::remove_file(&path).ok();

        let loaded = load_observation_only_snapshot(&test_config_for_path(path)).await;

        assert!(loaded.unwrap().observations.is_empty());
    }

    #[tokio::test]
    async fn snapshot_loader_rejects_oversized_snapshot() {
        let path = temp_snapshot_path("oversized");
        fs::write(&path, b"{}").unwrap();
        let mut config = test_config_for_path(path.clone());
        config.max_snapshot_bytes = 1;

        let loaded = load_observation_only_snapshot(&config).await;

        fs::remove_file(path).ok();
        assert!(loaded.is_err());
    }

    fn test_config_for_path(path: PathBuf) -> AdapterConfig {
        AdapterConfig {
            node_local_api_url: "http://127.0.0.1:8081".to_string(),
            adapter_token: "token".to_string(),
            adapter_instance_id: "adapter-a".to_string(),
            poll_interval_seconds: 1,
            dry_run_observation_only: true,
            snapshot_path: Some(path),
            max_snapshot_bytes: 1_048_576,
            max_snapshot_observations: 2_048,
            snapshot_stability_millis: 0,
            runtime_driver: None,
        }
    }

    fn exact_observation(
        session_id: &str,
        runtime_session_ref: &str,
    ) -> SubscriptionSessionObservation {
        SubscriptionSessionObservation {
            session_id: session_id.to_string(),
            runtime_username: "catalog/client-a".to_string(),
            runtime_session_ref: Some(runtime_session_ref.to_string()),
            device_fingerprint: None,
            source_ip: None,
            connected_at_unix: Some(42),
        }
    }

    fn temp_snapshot_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "hydra-node-session-adapter-{name}-{}-{nanos}.json",
            std::process::id()
        ))
    }
}

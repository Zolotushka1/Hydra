use std::{
    env,
    ffi::OsStr,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail, ensure};
use panel_domain::installer::{
    PanelAccessMode, PanelCertificateSource, PanelInstallPlanRequest, PanelInstallerArtifactKind,
    PanelInstallerCommandAttestation, PanelInstallerCommandEnvelopeView,
    PanelInstallerCommandResult, PanelInstallerExecutorOperationKind,
    PanelInstallerExecutorOperationView, PanelInstallerExecutorSessionView,
    PanelInstallerFirstHostSessionRequest, PanelInstallerJobAccessRequest,
    PanelInstallerJobHeartbeatRequest, PanelInstallerJobResultRequest, PanelInstallerJobStatus,
    PanelInstallerJobView, PanelInstallerOperationResult, PanelInstallerPackageChannel,
    PanelInstallerReleaseArtifactRequest, PanelInstallerSessionResultRequest,
    PanelInstallerTargetArch, PanelInstallerTargetOs,
};
use rand::{Rng, distr::Alphanumeric};
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair};
use reqwest::{Client, StatusCode, Url, header::LOCATION};
use sha2::{Digest, Sha256};
use sysinfo::{Disks, System};
use tokio::{process::Command, time::timeout};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

const EXECUTOR_CONTRACT_VERSION: u16 = 1;
const DEFAULT_MAX_DOWNLOAD_BYTES: u64 = 128 * 1024 * 1024;
const MAX_COMMAND_OUTPUT_BYTES: usize = 4 * 1024;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(120);
const PACKAGE_COMMAND_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Debug)]
struct ExecutorConfig {
    mode: ExecutorMode,
    dry_run: bool,
    confirm_destructive: bool,
    max_download_bytes: u64,
    bootstrap_admin_username: String,
    bootstrap_admin_password: String,
    generated_bootstrap_password: bool,
}

#[derive(Debug)]
enum ExecutorMode {
    Managed {
        panel_url: Url,
        job_id: String,
        executor_token: String,
    },
    FirstHost {
        request: PanelInstallerFirstHostSessionRequest,
    },
}

#[derive(Default)]
struct ExecutionState {
    downloaded_artifact_path: Option<PathBuf>,
    artifact_source_url: Option<String>,
    artifact_sha256_verified: bool,
    installed_binary_path: Option<String>,
    fingerprint_sha256: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let config = ExecutorConfig::from_env()?;
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("hydra-panel-installer-executor/0.1")
        .build()
        .context("failed to build installer HTTP client")?;

    let session = match &config.mode {
        ExecutorMode::Managed { .. } => {
            let job = fetch_job(&client, &config).await?;
            validate_job(&job, &config)?;
            ensure_admin_privileges().await?;
            let command_results = execute_session(&client, &config, &job.session).await;
            let accepted = submit_result(&client, &config, command_results).await?;
            ensure!(
                accepted.status == PanelInstallerJobStatus::Succeeded,
                "panel rejected installer result: {}",
                accepted.detail
            );
            job.session
        }
        ExecutorMode::FirstHost { request } => {
            let session = panel_core::build_panel_installer_first_host_session(request.clone())
                .context("first-host installer plan was rejected")?;
            validate_session(&session, &config)?;
            if config.dry_run {
                println!("{}", serde_json::to_string_pretty(&session)?);
                return Ok(());
            }
            ensure_admin_privileges().await?;
            let command_results = execute_session(&client, &config, &session).await;
            let result = panel_core::validate_panel_installer_result(local_result_request(
                &session,
                command_results,
            ));
            ensure!(
                result.accepted,
                "local installer result rejected: {}",
                result.issues.join("; ")
            );
            session
        }
    };

    println!("Hydra Panel installation completed.");
    println!("Panel URL: {}", session.plan.public_url);
    println!(
        "Bootstrap admin username: {}",
        config.bootstrap_admin_username
    );
    if config.generated_bootstrap_password {
        println!(
            "Bootstrap admin password (shown once): {}",
            config.bootstrap_admin_password
        );
    }
    Ok(())
}

impl ExecutorConfig {
    fn from_env() -> Result<Self> {
        let mode = match env::var("HYDRA_INSTALLER_MODE")
            .unwrap_or_else(|_| "first_host".to_string())
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "managed" => {
                let panel_url = required_env("HYDRA_INSTALLER_PANEL_URL")?
                    .parse::<Url>()
                    .context("HYDRA_INSTALLER_PANEL_URL is invalid")?;
                validate_panel_url(&panel_url)?;
                let job_id = required_env("HYDRA_INSTALLER_JOB_ID")?;
                validate_identifier("installer job id", &job_id, 160)?;
                let executor_token = required_env("HYDRA_INSTALLER_EXECUTOR_TOKEN")?;
                ensure!(
                    executor_token.len() >= 32,
                    "installer executor token is too short"
                );
                ExecutorMode::Managed {
                    panel_url,
                    job_id,
                    executor_token,
                }
            }
            "first_host" => ExecutorMode::FirstHost {
                request: first_host_request_from_env()?,
            },
            _ => bail!("HYDRA_INSTALLER_MODE must be first_host or managed"),
        };
        let confirm_destructive = env_flag("HYDRA_INSTALLER_CONFIRM_DESTRUCTIVE");
        let dry_run = env_flag("HYDRA_INSTALLER_DRY_RUN");
        let max_download_bytes = env::var("HYDRA_INSTALLER_MAX_DOWNLOAD_BYTES")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(DEFAULT_MAX_DOWNLOAD_BYTES)
            .clamp(1024 * 1024, 512 * 1024 * 1024);
        let bootstrap_admin_username =
            env::var("HYDRA_BOOTSTRAP_ADMIN_USERNAME").unwrap_or_else(|_| "admin".to_string());
        validate_identifier("bootstrap admin username", &bootstrap_admin_username, 64)?;
        let supplied_password = env::var("HYDRA_BOOTSTRAP_ADMIN_PASSWORD").ok();
        let generated_bootstrap_password = supplied_password.is_none();
        let bootstrap_admin_password = supplied_password.unwrap_or_else(generate_password);
        validate_bootstrap_password(&bootstrap_admin_password)?;
        Ok(Self {
            mode,
            dry_run,
            confirm_destructive,
            max_download_bytes,
            bootstrap_admin_username,
            bootstrap_admin_password,
            generated_bootstrap_password,
        })
    }

    fn managed(&self) -> Result<(&Url, &str, &str)> {
        match &self.mode {
            ExecutorMode::Managed {
                panel_url,
                job_id,
                executor_token,
            } => Ok((panel_url, job_id, executor_token)),
            ExecutorMode::FirstHost { .. } => {
                bail!("managed installer credentials are unavailable")
            }
        }
    }
}

async fn fetch_job(client: &Client, config: &ExecutorConfig) -> Result<PanelInstallerJobView> {
    let (panel_url, job_id, executor_token) = config.managed()?;
    let url = panel_url.join("/api/installer/jobs/executor-session")?;
    client
        .post(url)
        .json(&PanelInstallerJobAccessRequest {
            job_id: job_id.to_string(),
            executor_token: executor_token.to_string(),
        })
        .send()
        .await
        .context("failed to fetch installer job")?
        .error_for_status()
        .context("panel rejected installer job access")?
        .json()
        .await
        .context("failed to decode installer job")
}

fn validate_job(job: &PanelInstallerJobView, config: &ExecutorConfig) -> Result<()> {
    let (_, job_id, _) = config.managed()?;
    ensure!(job.job_id == job_id, "installer job id mismatch");
    validate_session(&job.session, config)
}

fn validate_session(
    session: &PanelInstallerExecutorSessionView,
    config: &ExecutorConfig,
) -> Result<()> {
    ensure!(
        session.compatible,
        "installer executor contract is incompatible"
    );
    ensure!(
        session.minimum_executor_contract_version <= EXECUTOR_CONTRACT_VERSION
            && session.supported_executor_contract_version >= EXECUTOR_CONTRACT_VERSION,
        "installer executor contract version is not supported"
    );
    ensure!(
        target_os_matches(session.target_os),
        "installer job target OS does not match this host"
    );
    ensure!(
        session.target_os == PanelInstallerTargetOs::Linux,
        "Windows managed installation is fail-closed until service environment, ACL, and certificate recipes are production-ready"
    );
    let artifact = session
        .selected_artifact
        .as_ref()
        .context("installer session does not contain a panel binary artifact")?;
    ensure!(
        artifact.artifact_kind == panel_domain::installer::PanelInstallerArtifactKind::PanelBinary,
        "installer payload artifact is not a panel binary"
    );
    ensure!(
        artifact.signature_url.is_none(),
        "detached signature metadata is present but signature verification is not implemented by this executor"
    );
    if !config.dry_run
        && session
            .command_envelopes
            .iter()
            .any(|command| command.requires_confirmation || command.destructive)
    {
        ensure!(
            config.confirm_destructive,
            "installer plan requires HYDRA_INSTALLER_CONFIRM_DESTRUCTIVE=1"
        );
    }
    validate_session_operations(session)?;
    Ok(())
}

fn validate_session_operations(session: &PanelInstallerExecutorSessionView) -> Result<()> {
    let artifact = session.selected_artifact.as_ref().unwrap();
    let mut saw_download = false;
    let mut saw_verify = false;
    let mut saw_install = false;
    for command in &session.command_envelopes {
        ensure!(
            command.executor_should_submit,
            "installer command is not executable"
        );
        for operation in &command.operations {
            validate_operation_target(session.target_os, operation)?;
            match operation.kind {
                PanelInstallerExecutorOperationKind::InstallPackageDependency => {
                    ensure!(
                        operation.args.as_slice() == ["certbot"],
                        "installer dependency is not allowlisted"
                    );
                }
                PanelInstallerExecutorOperationKind::DownloadArtifact => {
                    ensure!(
                        operation.args.as_slice() == [artifact.url.as_str()],
                        "artifact URL mismatch"
                    );
                    saw_download = true;
                }
                PanelInstallerExecutorOperationKind::VerifySha256 => {
                    ensure!(
                        operation.args.as_slice() == [artifact.sha256.as_str()],
                        "artifact SHA-256 mismatch"
                    );
                    saw_verify = true;
                }
                PanelInstallerExecutorOperationKind::InstallBinary => saw_install = true,
                _ => {}
            }
        }
    }
    ensure!(
        saw_download && saw_verify && saw_install,
        "installer binary operation chain is incomplete"
    );
    Ok(())
}

async fn execute_session(
    client: &Client,
    config: &ExecutorConfig,
    session: &PanelInstallerExecutorSessionView,
) -> Vec<PanelInstallerCommandResult> {
    let mut commands = session.command_envelopes.clone();
    commands.sort_by_key(|command| command.order);
    let mut state = ExecutionState::default();
    let mut results = Vec::with_capacity(commands.len());
    let mut stopped = false;
    for command in commands {
        if stopped {
            results.push(failed_command_result(
                &command,
                "skipped after prior failure",
            ));
            continue;
        }
        if matches!(&config.mode, ExecutorMode::Managed { .. }) {
            let _ = send_heartbeat(client, config, &command.step_id).await;
        }
        match execute_command(client, config, session, &command, &mut state).await {
            Ok(result) => results.push(result),
            Err(error) => {
                results.push(failed_command_result(&command, &safe_error(&error)));
                stopped = true;
            }
        }
    }
    results
}

async fn execute_command(
    client: &Client,
    config: &ExecutorConfig,
    session: &PanelInstallerExecutorSessionView,
    command: &PanelInstallerCommandEnvelopeView,
    state: &mut ExecutionState,
) -> Result<PanelInstallerCommandResult> {
    info!(command_id = %command.command_id, step = %command.step_id, "executing installer command");
    let mut attestation = PanelInstallerCommandAttestation::default();
    for operation in &command.operations {
        let qualified_id = format!("{}/{}", command.command_id, operation.operation_id);
        let result =
            execute_operation(client, config, session, operation, state, &mut attestation).await;
        match result {
            Ok(target_path) => attestation
                .operation_results
                .push(PanelInstallerOperationResult {
                    operation_id: qualified_id,
                    exit_code: 0,
                    completed: true,
                    verified: Some(true),
                    target_path,
                    detail: Some("operation completed and verified".to_string()),
                }),
            Err(error) => {
                attestation
                    .operation_results
                    .push(PanelInstallerOperationResult {
                        operation_id: qualified_id,
                        exit_code: 1,
                        completed: false,
                        verified: Some(false),
                        target_path: operation.target_path.clone(),
                        detail: Some(safe_error(&error)),
                    });
                return Err(error);
            }
        }
    }
    Ok(PanelInstallerCommandResult {
        command_id: command.command_id.clone(),
        exit_code: 0,
        detail: Some("command completed".to_string()),
        attestation,
    })
}

async fn execute_operation(
    client: &Client,
    config: &ExecutorConfig,
    session: &PanelInstallerExecutorSessionView,
    operation: &PanelInstallerExecutorOperationView,
    state: &mut ExecutionState,
    attestation: &mut PanelInstallerCommandAttestation,
) -> Result<Option<String>> {
    match operation.kind {
        PanelInstallerExecutorOperationKind::PreflightProbe => {
            let system = System::new_all();
            let memory_total_mb = system.total_memory() / (1024 * 1024);
            let disk_free_mb = installation_disk_free_mb();
            let selected_port_available = selected_port_available(&session.plan.bind_address);
            let os_supported = supported_host_os();
            ensure!(os_supported, "host operating system is unsupported");
            ensure!(memory_total_mb >= 512, "host has less than 512 MB RAM");
            ensure!(disk_free_mb >= 1024, "host has less than 1024 MB free disk");
            ensure!(
                selected_port_available,
                "selected panel port is not available"
            );
            attestation.os_supported = Some(true);
            attestation.memory_total_mb = Some(memory_total_mb);
            attestation.disk_free_mb = Some(disk_free_mb);
            attestation.selected_port_available = Some(true);
            Ok(None)
        }
        PanelInstallerExecutorOperationKind::InstallPackageDependency => {
            ensure!(
                operation.args.as_slice() == ["certbot"],
                "installer dependency is not allowlisted"
            );
            install_certbot().await?;
            attestation.dependencies_ready = Some(true);
            Ok(None)
        }
        PanelInstallerExecutorOperationKind::CreateDirectory => {
            let path = required_target(operation)?;
            fs::create_dir_all(path)
                .with_context(|| format!("failed to create {}", path.display()))?;
            set_mode(path, 0o700)?;
            if cfg!(unix) {
                fs::create_dir_all("/var/lib/hydra-panel")?;
                fs::create_dir_all("/etc/hydra-panel/certs")?;
            }
            Ok(operation.target_path.clone())
        }
        PanelInstallerExecutorOperationKind::DownloadArtifact => {
            let url = operation.args.first().context("download URL is missing")?;
            let path = download_artifact(client, url, config.max_download_bytes).await?;
            state.downloaded_artifact_path = Some(path);
            state.artifact_source_url = Some(url.clone());
            Ok(None)
        }
        PanelInstallerExecutorOperationKind::VerifySha256 => {
            let expected = operation
                .args
                .first()
                .context("expected SHA-256 is missing")?;
            let path = state
                .downloaded_artifact_path
                .as_ref()
                .context("artifact was not downloaded")?;
            verify_sha256(path, expected)?;
            state.artifact_sha256_verified = true;
            Ok(None)
        }
        PanelInstallerExecutorOperationKind::InstallBinary => {
            ensure!(
                state.artifact_sha256_verified,
                "artifact checksum was not verified"
            );
            let source = state
                .downloaded_artifact_path
                .as_ref()
                .context("artifact was not downloaded")?
                .clone();
            let target = required_target(operation)?;
            atomic_copy(&source, target, 0o755)?;
            fs::remove_file(&source).context("failed to remove verified temporary artifact")?;
            state.downloaded_artifact_path = None;
            state.installed_binary_path = Some(target.to_string_lossy().to_string());
            attestation.binary_installed = Some(true);
            attestation.binary_path = state.installed_binary_path.clone();
            attestation.artifact_source_url = state.artifact_source_url.clone();
            attestation.artifact_sha256_verified = Some(true);
            Ok(operation.target_path.clone())
        }
        PanelInstallerExecutorOperationKind::WriteConfig => {
            let target = required_target(operation)?;
            let template = operation
                .content_template
                .as_deref()
                .context("config template is missing")?;
            let content = render_panel_env(template, config);
            atomic_write(target, content.as_bytes(), 0o600)?;
            attestation.config_written = Some(true);
            attestation.bind_address = Some(session.plan.bind_address.clone());
            Ok(operation.target_path.clone())
        }
        PanelInstallerExecutorOperationKind::GenerateSelfSignedCertificate => {
            generate_self_signed(session, state)?;
            fill_certificate_attestation(session, state, attestation)?;
            Ok(operation.target_path.clone())
        }
        PanelInstallerExecutorOperationKind::IssueLetsEncryptCertificate => {
            ensure!(
                cfg!(unix),
                "Let's Encrypt executor recipe is currently Linux-only"
            );
            ensure!(
                selected_port_available("0.0.0.0:80"),
                "Let's Encrypt standalone challenge requires local TCP port 80 to be available"
            );
            run_declared_program(operation).await?;
            install_letsencrypt_material(session)?;
            install_letsencrypt_renewal_hook(session)?;
            fill_certificate_attestation(session, state, attestation)?;
            Ok(operation.target_path.clone())
        }
        PanelInstallerExecutorOperationKind::ApplyFirewall => {
            apply_firewall(session, operation).await?;
            attestation.firewall_rules_applied = Some(true);
            Ok(None)
        }
        PanelInstallerExecutorOperationKind::ApplySecurityDefaults => {
            apply_security_defaults().await?;
            attestation.security_defaults_applied = Some(true);
            Ok(operation.target_path.clone())
        }
        PanelInstallerExecutorOperationKind::WriteService => {
            if let Some(template) = operation.content_template.as_deref() {
                let target = required_target(operation)?;
                atomic_write(target, template.as_bytes(), 0o644)?;
            } else {
                run_declared_program(operation).await?;
            }
            Ok(operation.target_path.clone())
        }
        PanelInstallerExecutorOperationKind::StartService => {
            run_declared_program(operation).await?;
            verify_service_active(session.target_os).await?;
            health_check(session).await?;
            attestation.service_name = Some(service_name(session.target_os).to_string());
            attestation.service_active = Some(true);
            attestation.health_check_ok = Some(true);
            Ok(None)
        }
        PanelInstallerExecutorOperationKind::HealthCheck => {
            health_check(session).await?;
            attestation.service_name = Some(service_name(session.target_os).to_string());
            attestation.service_active = Some(true);
            attestation.health_check_ok = Some(true);
            Ok(None)
        }
    }
}

async fn submit_result(
    client: &Client,
    config: &ExecutorConfig,
    command_results: Vec<PanelInstallerCommandResult>,
) -> Result<PanelInstallerJobView> {
    let (panel_url, job_id, executor_token) = config.managed()?;
    let url = panel_url.join("/api/installer/jobs/result")?;
    client
        .post(url)
        .json(&PanelInstallerJobResultRequest {
            job_id: job_id.to_string(),
            executor_token: executor_token.to_string(),
            command_results,
        })
        .send()
        .await
        .context("failed to submit installer result")?
        .error_for_status()
        .context("panel rejected installer result submission")?
        .json()
        .await
        .context("failed to decode installer result")
}

async fn send_heartbeat(client: &Client, config: &ExecutorConfig, phase: &str) -> Result<()> {
    let (panel_url, job_id, executor_token) = config.managed()?;
    let url = panel_url.join("/api/installer/jobs/heartbeat")?;
    client
        .post(url)
        .json(&PanelInstallerJobHeartbeatRequest {
            job_id: job_id.to_string(),
            executor_token: executor_token.to_string(),
            observed_phase: Some(phase.to_string()),
            message: Some("executor started phase".to_string()),
        })
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

async fn download_artifact(client: &Client, source: &str, max_bytes: u64) -> Result<PathBuf> {
    let mut current = source.parse::<Url>().context("artifact URL is invalid")?;
    validate_artifact_url(&current)?;
    for _ in 0..=5 {
        let mut response = client
            .get(current.clone())
            .send()
            .await
            .context("artifact download failed")?;
        if response.status().is_redirection() {
            let location = response
                .headers()
                .get(LOCATION)
                .context("redirect is missing Location")?;
            let next = current.join(location.to_str().context("redirect Location is invalid")?)?;
            validate_redirect(&current, &next)?;
            current = next;
            continue;
        }
        ensure!(
            response.status() == StatusCode::OK,
            "artifact server returned {}",
            response.status()
        );
        if let Some(length) = response.content_length() {
            ensure!(
                length <= max_bytes,
                "artifact exceeds configured download limit"
            );
        }
        let path = temp_artifact_path();
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)?;
        let mut written = 0_u64;
        while let Some(chunk) = response
            .chunk()
            .await
            .context("artifact body read failed")?
        {
            written = written.saturating_add(chunk.len() as u64);
            ensure!(
                written <= max_bytes,
                "artifact exceeds configured download limit"
            );
            file.write_all(&chunk)?;
        }
        file.sync_all()?;
        ensure!(written > 0, "downloaded artifact is empty");
        return Ok(path);
    }
    bail!("artifact redirect limit exceeded")
}

fn verify_sha256(path: &Path, expected: &str) -> Result<()> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let actual = format!("{:x}", hasher.finalize());
    ensure!(
        actual.eq_ignore_ascii_case(expected.trim()),
        "artifact SHA-256 mismatch"
    );
    Ok(())
}

fn generate_self_signed(
    session: &PanelInstallerExecutorSessionView,
    state: &mut ExecutionState,
) -> Result<()> {
    let certificate = &session.plan.certificate_plan;
    let san = certificate
        .subject_alt_name
        .as_deref()
        .and_then(|value| value.split_once(':').map(|(_, name)| name.to_string()))
        .context("self-signed certificate SAN is missing")?;
    let mut params = CertificateParams::new(vec![san])?;
    let mut distinguished_name = DistinguishedName::new();
    distinguished_name.push(DnType::CommonName, "Hydra Panel");
    params.distinguished_name = distinguished_name;
    let key = KeyPair::generate()?;
    let cert = params.self_signed(&key)?;
    let cert_path = certificate
        .certificate_path
        .as_deref()
        .context("certificate path is missing")?;
    let key_path = certificate
        .private_key_path
        .as_deref()
        .context("private key path is missing")?;
    atomic_write(Path::new(cert_path), cert.pem().as_bytes(), 0o644)?;
    atomic_write(Path::new(key_path), key.serialize_pem().as_bytes(), 0o600)?;
    state.fingerprint_sha256 = Some(file_sha256(Path::new(cert_path))?);
    Ok(())
}

fn install_letsencrypt_material(session: &PanelInstallerExecutorSessionView) -> Result<()> {
    let plan = &session.plan.certificate_plan;
    let domain = plan.domain.as_deref().context("ACME domain is missing")?;
    let source_dir = Path::new("/etc/letsencrypt/live").join(domain);
    let cert_target = Path::new(
        plan.certificate_path
            .as_deref()
            .context("certificate path is missing")?,
    );
    let key_target = Path::new(
        plan.private_key_path
            .as_deref()
            .context("private key path is missing")?,
    );
    atomic_copy(&source_dir.join("fullchain.pem"), cert_target, 0o644)?;
    atomic_copy(&source_dir.join("privkey.pem"), key_target, 0o600)?;
    Ok(())
}

fn install_letsencrypt_renewal_hook(session: &PanelInstallerExecutorSessionView) -> Result<()> {
    let plan = &session.plan.certificate_plan;
    let domain = plan.domain.as_deref().context("ACME domain is missing")?;
    let cert = plan
        .certificate_path
        .as_deref()
        .context("certificate path is missing")?;
    let key = plan
        .private_key_path
        .as_deref()
        .context("private key path is missing")?;
    let content = format!(
        "#!/bin/sh\nset -eu\ninstall -m 0644 '/etc/letsencrypt/live/{domain}/fullchain.pem' '{cert}'\ninstall -m 0600 '/etc/letsencrypt/live/{domain}/privkey.pem' '{key}'\nsystemctl restart hydra-panel.service\n"
    );
    atomic_write(
        Path::new("/etc/letsencrypt/renewal-hooks/deploy/hydra-panel"),
        content.as_bytes(),
        0o700,
    )
}

fn fill_certificate_attestation(
    session: &PanelInstallerExecutorSessionView,
    state: &ExecutionState,
    attestation: &mut PanelInstallerCommandAttestation,
) -> Result<()> {
    let plan = &session.plan.certificate_plan;
    let cert = plan
        .certificate_path
        .as_deref()
        .context("certificate path is missing")?;
    let key = plan
        .private_key_path
        .as_deref()
        .context("private key path is missing")?;
    ensure!(
        Path::new(cert).is_file(),
        "certificate file was not created"
    );
    ensure!(Path::new(key).is_file(), "private key file was not created");
    attestation.certificate_path = Some(cert.to_string());
    attestation.private_key_path = Some(key.to_string());
    attestation.private_key_mode = Some("0600".to_string());
    if session.plan.access_mode == PanelAccessMode::IpSelfSignedTls {
        attestation.fingerprint_sha256 = state.fingerprint_sha256.clone();
    }
    Ok(())
}

async fn apply_security_defaults() -> Result<()> {
    ensure!(
        cfg!(unix),
        "production security-default recipe is currently Linux-only"
    );
    if run_program("id", &["-u", "hydra"]).await.is_err() {
        run_program(
            "useradd",
            &[
                "--system",
                "--home",
                "/var/lib/hydra-panel",
                "--shell",
                "/usr/sbin/nologin",
                "hydra",
            ],
        )
        .await?;
    }
    fs::create_dir_all("/var/lib/hydra-panel")?;
    run_program(
        "chown",
        &[
            "-R",
            "hydra:hydra",
            "/var/lib/hydra-panel",
            "/etc/hydra-panel",
        ],
    )
    .await?;
    Ok(())
}

async fn install_certbot() -> Result<()> {
    ensure!(cfg!(unix), "certbot package installation is Linux-only");
    if command_exists("certbot") {
        return Ok(());
    }
    if command_exists("apt-get") {
        run_program_with_timeout("apt-get", &["update"], PACKAGE_COMMAND_TIMEOUT).await?;
        run_program_with_timeout(
            "apt-get",
            &["install", "-y", "--no-install-recommends", "certbot"],
            PACKAGE_COMMAND_TIMEOUT,
        )
        .await?;
    } else if command_exists("dnf") {
        run_program_with_timeout(
            "dnf",
            &["install", "-y", "certbot"],
            PACKAGE_COMMAND_TIMEOUT,
        )
        .await?;
    } else if command_exists("yum") {
        if run_program_with_timeout(
            "yum",
            &["install", "-y", "certbot"],
            PACKAGE_COMMAND_TIMEOUT,
        )
        .await
        .is_err()
        {
            run_program_with_timeout(
                "yum",
                &["install", "-y", "epel-release"],
                PACKAGE_COMMAND_TIMEOUT,
            )
            .await?;
            run_program_with_timeout(
                "yum",
                &["install", "-y", "certbot"],
                PACKAGE_COMMAND_TIMEOUT,
            )
            .await?;
        }
    } else {
        bail!("no supported package manager found for certbot (apt-get, dnf, or yum)");
    }
    ensure!(
        command_exists("certbot"),
        "certbot package installation completed but executable is unavailable"
    );
    Ok(())
}

async fn apply_firewall(
    session: &PanelInstallerExecutorSessionView,
    operation: &PanelInstallerExecutorOperationView,
) -> Result<()> {
    ensure!(
        !operation.args.is_empty(),
        "firewall bind address is missing"
    );
    let port = bind_port(&operation.args[0])?;
    let allowlist = &operation.args[1..];
    ensure!(!allowlist.is_empty(), "firewall allowlist is empty");
    match session.target_os {
        PanelInstallerTargetOs::Linux => {
            if command_exists("firewall-cmd") {
                for source in allowlist {
                    let family = if source.contains(':') { "ipv6" } else { "ipv4" };
                    let rule = format!(
                        "rule family={family} source address={source} port port={port} protocol=tcp accept"
                    );
                    run_program(
                        "firewall-cmd",
                        &["--permanent", &format!("--add-rich-rule={rule}")],
                    )
                    .await?;
                }
                run_program("firewall-cmd", &["--reload"]).await?;
            } else if command_exists("ufw") {
                for source in allowlist {
                    run_program(
                        "ufw",
                        &[
                            "allow",
                            "from",
                            source,
                            "to",
                            "any",
                            "port",
                            &port.to_string(),
                            "proto",
                            "tcp",
                        ],
                    )
                    .await?;
                }
            } else {
                bail!("no supported Linux firewall backend found (firewall-cmd or ufw)");
            }
        }
        PanelInstallerTargetOs::Windows => {
            for (index, source) in allowlist.iter().enumerate() {
                run_program(
                    "netsh.exe",
                    &[
                        "advfirewall",
                        "firewall",
                        "add",
                        "rule",
                        &format!("name=Hydra Panel {index}"),
                        "dir=in",
                        "action=allow",
                        "protocol=TCP",
                        &format!("localport={port}"),
                        &format!("remoteip={source}"),
                    ],
                )
                .await?;
            }
        }
    }
    Ok(())
}

async fn run_declared_program(operation: &PanelInstallerExecutorOperationView) -> Result<()> {
    let program = operation
        .program
        .as_deref()
        .context("declared program is missing")?;
    let allowed = match operation.kind {
        PanelInstallerExecutorOperationKind::IssueLetsEncryptCertificate => program == "certbot",
        PanelInstallerExecutorOperationKind::WriteService => program == "sc.exe",
        PanelInstallerExecutorOperationKind::StartService => {
            program == "systemctl" || program == "sc.exe"
        }
        _ => false,
    };
    ensure!(
        allowed,
        "declared program is not allowed for this operation"
    );
    let args = operation
        .args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    run_program(program, &args).await
}

async fn run_program<S: AsRef<OsStr>>(program: &str, args: &[S]) -> Result<()> {
    run_program_with_timeout(program, args, COMMAND_TIMEOUT).await
}

async fn run_program_with_timeout<S: AsRef<OsStr>>(
    program: &str,
    args: &[S],
    command_timeout: Duration,
) -> Result<()> {
    let mut command = Command::new(program);
    command.args(args).stdin(Stdio::null()).kill_on_drop(true);
    let output = timeout(command_timeout, command.output())
        .await
        .with_context(|| format!("{program} timed out"))?
        .with_context(|| format!("failed to execute {program}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("{program} failed: {}", bounded_text(&stderr));
    }
    Ok(())
}

async fn ensure_admin_privileges() -> Result<()> {
    ensure!(cfg!(unix), "host mutation is currently Linux-only");
    let output = Command::new("id")
        .args(["-u"])
        .stdin(Stdio::null())
        .output()
        .await
        .context("failed to determine effective user id")?;
    ensure!(
        output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "0",
        "installer host mutation requires root privileges"
    );
    Ok(())
}

async fn verify_service_active(target_os: PanelInstallerTargetOs) -> Result<()> {
    match target_os {
        PanelInstallerTargetOs::Linux => {
            run_program(
                "systemctl",
                &["is-active", "--quiet", "hydra-panel.service"],
            )
            .await
        }
        PanelInstallerTargetOs::Windows => run_program("sc.exe", &["query", "HydraPanel"]).await,
    }
}

async fn health_check(session: &PanelInstallerExecutorSessionView) -> Result<()> {
    let scheme = if session.plan.certificate_plan.tls_enabled {
        "https"
    } else {
        "http"
    };
    let port = bind_port(&session.plan.bind_address)?;
    let url = format!("{scheme}://127.0.0.1:{port}/health");
    let local_client = Client::builder()
        .timeout(Duration::from_secs(3))
        .danger_accept_invalid_certs(
            session.plan.certificate_plan.source == PanelCertificateSource::SelfSigned,
        )
        .build()?;
    for _ in 0..20 {
        if let Ok(response) = local_client.get(&url).send().await
            && response.status().is_success()
        {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    bail!("panel local health check failed")
}

fn render_panel_env(template: &str, config: &ExecutorConfig) -> String {
    format!(
        "{template}HYDRA_BOOTSTRAP_ADMIN_USERNAME={}\nHYDRA_BOOTSTRAP_ADMIN_PASSWORD={}\nHYDRA_ADMIN_SECRETS_KEY_PATH=/var/lib/hydra-panel/admin-secrets.key\nHYDRA_SECURITY_SETTINGS_PATH=/var/lib/hydra-panel/security-settings.json\n",
        config.bootstrap_admin_username, config.bootstrap_admin_password
    )
}

fn validate_operation_target(
    target_os: PanelInstallerTargetOs,
    operation: &PanelInstallerExecutorOperationView,
) -> Result<()> {
    let Some(path) = operation.target_path.as_deref() else {
        return Ok(());
    };
    let allowed = match target_os {
        PanelInstallerTargetOs::Linux => {
            path == "/etc/hydra-panel"
                || path == "/etc/hydra-panel/panel.env"
                || path == "/etc/systemd/system/hydra-panel.service"
                || path == "/usr/local/bin/hydra-panel"
                || path.starts_with("/etc/hydra-panel/certs/")
        }
        PanelInstallerTargetOs::Windows => {
            path.starts_with("C:\\ProgramData\\HydraPanel")
                || path == "C:\\Program Files\\HydraPanel\\hydra-panel.exe"
        }
    };
    ensure!(
        allowed,
        "installer operation target path is outside the allowlist"
    );
    Ok(())
}

fn atomic_write(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = temporary_sibling(path);
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    set_mode(&temp, mode)?;
    replace_file(&temp, path)?;
    Ok(())
}

fn atomic_copy(source: &Path, target: &Path, mode: u32) -> Result<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = temporary_sibling(target);
    let mut source_file = File::open(source)?;
    let mut target_file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp)?;
    io::copy(&mut source_file, &mut target_file)?;
    target_file.sync_all()?;
    set_mode(&temp, mode)?;
    replace_file(&temp, target)?;
    Ok(())
}

fn replace_file(source: &Path, target: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        fs::rename(source, target)?;
    }
    #[cfg(not(unix))]
    {
        if target.exists() {
            fs::remove_file(target)?;
        }
        fs::rename(source, target)?;
    }
    Ok(())
}

fn set_mode(path: &Path, mode: u32) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    }
    #[cfg(not(unix))]
    let _ = (path, mode);
    Ok(())
}

fn selected_port_available(bind_address: &str) -> bool {
    TcpListener::bind(bind_address).is_ok()
}

fn installation_disk_free_mb() -> u64 {
    let install_path = if cfg!(windows) {
        Path::new("C:\\Program Files\\HydraPanel")
    } else {
        Path::new("/usr/local/bin")
    };
    Disks::new_with_refreshed_list()
        .iter()
        .filter(|disk| install_path.starts_with(disk.mount_point()))
        .max_by_key(|disk| disk.mount_point().as_os_str().len())
        .map(|disk| disk.available_space() / (1024 * 1024))
        .unwrap_or(0)
}

fn bind_port(bind_address: &str) -> Result<u16> {
    bind_address
        .rsplit_once(':')
        .and_then(|(_, port)| port.parse::<u16>().ok())
        .context("bind address does not contain a valid port")
}

fn supported_host_os() -> bool {
    if cfg!(windows) {
        return true;
    }
    if !cfg!(target_os = "linux") {
        return false;
    }
    let os_release = fs::read_to_string("/etc/os-release")
        .unwrap_or_default()
        .to_ascii_lowercase();
    ["ubuntu", "debian", "centos", "rhel", "almalinux", "astra"]
        .iter()
        .any(|id| {
            os_release.contains(&format!("id={id}")) || os_release.contains(&format!("id=\"{id}\""))
        })
}

fn target_os_matches(target_os: PanelInstallerTargetOs) -> bool {
    matches!(target_os, PanelInstallerTargetOs::Linux) && cfg!(target_os = "linux")
        || matches!(target_os, PanelInstallerTargetOs::Windows) && cfg!(windows)
}

fn validate_panel_url(url: &Url) -> Result<()> {
    let localhost = matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "::1"));
    ensure!(
        url.scheme() == "https" || localhost && url.scheme() == "http",
        "panel URL must use HTTPS except localhost development"
    );
    ensure!(
        url.username().is_empty() && url.password().is_none(),
        "panel URL must not contain credentials"
    );
    Ok(())
}

fn validate_artifact_url(url: &Url) -> Result<()> {
    ensure!(url.scheme() == "https", "artifact URL must use HTTPS");
    ensure!(
        url.username().is_empty() && url.password().is_none(),
        "artifact URL must not contain credentials"
    );
    ensure!(url.host_str().is_some(), "artifact URL host is missing");
    Ok(())
}

fn validate_redirect(previous: &Url, next: &Url) -> Result<()> {
    validate_artifact_url(next)?;
    let previous_host = previous.host_str().unwrap_or_default();
    let next_host = next.host_str().unwrap_or_default();
    let trusted_github = matches!(
        next_host,
        "github.com" | "objects.githubusercontent.com" | "release-assets.githubusercontent.com"
    );
    ensure!(
        previous_host == next_host || trusted_github,
        "artifact redirect changed to an untrusted host"
    );
    Ok(())
}

fn required_target(operation: &PanelInstallerExecutorOperationView) -> Result<&Path> {
    operation
        .target_path
        .as_deref()
        .map(Path::new)
        .context("operation target path is missing")
}

fn command_exists(program: &str) -> bool {
    env::var_os("PATH").is_some_and(|paths| {
        env::split_paths(&paths).any(|path| {
            let candidate = path.join(program);
            candidate.is_file() || cfg!(windows) && path.join(format!("{program}.exe")).is_file()
        })
    })
}

fn service_name(target_os: PanelInstallerTargetOs) -> &'static str {
    match target_os {
        PanelInstallerTargetOs::Linux => "hydra-panel.service",
        PanelInstallerTargetOs::Windows => "HydraPanel",
    }
}

fn file_sha256(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn temp_artifact_path() -> PathBuf {
    env::temp_dir().join(format!(
        "hydra-panel-artifact-{}-{}",
        std::process::id(),
        now_unix()
    ))
}

fn temporary_sibling(path: &Path) -> PathBuf {
    let name = path.file_name().and_then(OsStr::to_str).unwrap_or("hydra");
    path.with_file_name(format!(".{name}.tmp-{}-{}", std::process::id(), now_unix()))
}

fn failed_command_result(
    command: &PanelInstallerCommandEnvelopeView,
    detail: &str,
) -> PanelInstallerCommandResult {
    PanelInstallerCommandResult {
        command_id: command.command_id.clone(),
        exit_code: 1,
        detail: Some(bounded_text(detail)),
        attestation: PanelInstallerCommandAttestation::default(),
    }
}

fn first_host_request_from_env() -> Result<PanelInstallerFirstHostSessionRequest> {
    let access_mode = match required_env("HYDRA_INSTALLER_ACCESS_MODE")?
        .to_ascii_lowercase()
        .as_str()
    {
        "domain_tls" => PanelAccessMode::DomainTls,
        "ip_http" => PanelAccessMode::IpHttp,
        "ip_self_signed_tls" => PanelAccessMode::IpSelfSignedTls,
        "reverse_proxy" => PanelAccessMode::ReverseProxy,
        _ => bail!(
            "HYDRA_INSTALLER_ACCESS_MODE must be domain_tls, ip_http, ip_self_signed_tls, or reverse_proxy"
        ),
    };
    let target_os = if cfg!(target_os = "linux") {
        PanelInstallerTargetOs::Linux
    } else if cfg!(windows) {
        PanelInstallerTargetOs::Windows
    } else {
        bail!("first-host installer does not support this operating system")
    };
    let target_arch = if cfg!(target_arch = "x86_64") {
        PanelInstallerTargetArch::X86_64
    } else if cfg!(target_arch = "aarch64") {
        PanelInstallerTargetArch::Aarch64
    } else {
        bail!("first-host installer does not support this CPU architecture")
    };
    let package_channel = match env::var("HYDRA_INSTALLER_PACKAGE_CHANNEL")
        .unwrap_or_else(|_| "stable".to_string())
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "stable" => PanelInstallerPackageChannel::Stable,
        "latest" => PanelInstallerPackageChannel::Latest,
        "pinned" => PanelInstallerPackageChannel::Pinned,
        _ => bail!("HYDRA_INSTALLER_PACKAGE_CHANNEL must be stable, latest, or pinned"),
    };
    let bind_port = optional_env("HYDRA_INSTALLER_BIND_PORT")
        .map(|value| {
            value
                .parse::<u16>()
                .context("HYDRA_INSTALLER_BIND_PORT is invalid")
        })
        .transpose()?;
    let panel_binary_url = required_env("HYDRA_INSTALLER_PANEL_BINARY_URL")?;
    let panel_binary_sha256 = required_env("HYDRA_INSTALLER_PANEL_BINARY_SHA256")?;
    let version = optional_env("HYDRA_INSTALLER_PANEL_BINARY_VERSION")
        .unwrap_or_else(|| "latest".to_string());
    let extension = if target_os == PanelInstallerTargetOs::Windows {
        ".exe"
    } else {
        ""
    };
    let os_name = if target_os == PanelInstallerTargetOs::Windows {
        "windows"
    } else {
        "linux"
    };
    let arch_name = if target_arch == PanelInstallerTargetArch::Aarch64 {
        "aarch64"
    } else {
        "x86_64"
    };

    Ok(PanelInstallerFirstHostSessionRequest {
        plan: PanelInstallPlanRequest {
            access_mode,
            domain: optional_env("HYDRA_INSTALLER_DOMAIN"),
            public_ip: optional_env("HYDRA_INSTALLER_PUBLIC_IP"),
            bind_host: optional_env("HYDRA_INSTALLER_BIND_HOST"),
            bind_port,
            acme_email: optional_env("HYDRA_INSTALLER_ACME_EMAIL"),
            firewall_allowlist: csv_env("HYDRA_INSTALLER_FIREWALL_ALLOWLIST"),
            trusted_proxy_cidrs: csv_env("HYDRA_INSTALLER_TRUSTED_PROXY_CIDRS"),
            confirm_public_http: env_flag("HYDRA_INSTALLER_CONFIRM_PUBLIC_HTTP"),
        },
        panel_binary: PanelInstallerReleaseArtifactRequest {
            name: format!("hydra-panel-{os_name}-{arch_name}{extension}"),
            artifact_kind: PanelInstallerArtifactKind::PanelBinary,
            target_os,
            target_arch,
            package_channel,
            version,
            url: panel_binary_url,
            sha256: panel_binary_sha256,
            signature_url: optional_env("HYDRA_INSTALLER_PANEL_BINARY_SIGNATURE_URL"),
            signing_key_fingerprint: optional_env("HYDRA_INSTALLER_SIGNING_KEY_FINGERPRINT"),
        },
        executor_contract_version: Some(EXECUTOR_CONTRACT_VERSION),
    })
}

fn local_result_request(
    session: &PanelInstallerExecutorSessionView,
    command_results: Vec<PanelInstallerCommandResult>,
) -> PanelInstallerSessionResultRequest {
    PanelInstallerSessionResultRequest {
        session_id: session.session_id.clone(),
        access_mode: session.plan.access_mode,
        expected_command_ids: session
            .command_envelopes
            .iter()
            .map(|command| command.command_id.clone())
            .collect(),
        expected_operation_ids: session
            .command_envelopes
            .iter()
            .flat_map(|command| {
                command
                    .operations
                    .iter()
                    .map(|operation| format!("{}/{}", command.command_id, operation.operation_id))
            })
            .collect(),
        command_results,
    }
}

fn required_env(name: &str) -> Result<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .with_context(|| format!("{name} is required"))
}

fn optional_env(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn csv_env(name: &str) -> Vec<String> {
    optional_env(name)
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn env_flag(name: &str) -> bool {
    env::var(name)
        .ok()
        .is_some_and(|value| matches!(value.trim(), "1" | "true" | "yes" | "on"))
}

fn validate_identifier(label: &str, value: &str, max_len: usize) -> Result<()> {
    ensure!(
        !value.is_empty() && value.len() <= max_len,
        "{label} length is invalid"
    );
    ensure!(
        value
            .chars()
            .all(|character| character.is_ascii_alphanumeric()
                || matches!(character, '-' | '_' | '.')),
        "{label} contains unsupported characters"
    );
    Ok(())
}

fn validate_bootstrap_password(value: &str) -> Result<()> {
    ensure!(
        (12..=128).contains(&value.len()),
        "bootstrap admin password must be 12-128 characters"
    );
    ensure!(
        value
            .chars()
            .all(|character| character.is_ascii_alphanumeric()),
        "bootstrap admin password must be alphanumeric for safe EnvironmentFile rendering"
    );
    Ok(())
}

fn generate_password() -> String {
    rand::rng()
        .sample_iter(&Alphanumeric)
        .take(32)
        .map(char::from)
        .collect()
}

fn safe_error(error: &anyhow::Error) -> String {
    bounded_text(&error.to_string().replace(['\r', '\n'], " "))
}

fn bounded_text(value: &str) -> String {
    value.chars().take(MAX_COMMAND_OUTPUT_BYTES).collect()
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_untrusted_redirects() {
        let previous = Url::parse("https://github.com/example/release").unwrap();
        let trusted = Url::parse("https://release-assets.githubusercontent.com/asset").unwrap();
        let untrusted = Url::parse("https://example.net/asset").unwrap();
        assert!(validate_redirect(&previous, &trusted).is_ok());
        assert!(validate_redirect(&previous, &untrusted).is_err());
    }

    #[test]
    fn bootstrap_password_is_environment_file_safe() {
        let password = generate_password();
        assert_eq!(password.len(), 32);
        assert!(validate_bootstrap_password(&password).is_ok());
        assert!(validate_bootstrap_password("contains space").is_err());
    }

    #[test]
    fn bind_port_parses_ipv4_and_ipv6_like_values_from_right() {
        assert_eq!(bind_port("0.0.0.0:2053").unwrap(), 2053);
        assert_eq!(bind_port("[::]:443").unwrap(), 443);
    }
}

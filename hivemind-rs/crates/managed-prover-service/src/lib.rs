use anyhow::{Context, Result};
use hivemind_auth::managed_proof::{
    ManagedProofAuthorizationBinding, ManagedProofAuthorizationVerifier,
};
use hivemind_config::{HivemindConfig, TrustedManagedDslWorkerRegistration};
use hivemind_managed_proof::{RISC0_MANAGED_GUEST_ID, RISC0_PROOF_SCHEME};
use hivemind_managed_prover_protocol::{
    ManagedProverRequest, ManagedProverResponse, RemoteManagedProofRequest, MAX_RESPONSE_JSON_BYTES,
};
use hivemind_proto::managedprover::managed_proof_provider_server::ManagedProofProvider;
use hivemind_proto::managedprover::{
    CancelManagedProofRequest, CancelManagedProofResponse, GetCapabilitiesRequest,
    GetCapabilitiesResponse, GetManagedProofRequest, GetManagedProofResponse,
    ManagedProofCapability, SubmitManagedProofRequest, SubmitManagedProofResponse,
};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::sync::{watch, Semaphore};
use tonic::transport::{Certificate, Identity, ServerTlsConfig};
use tonic::{Request, Response, Status};
use uuid::Uuid;

const STATE_PENDING: &str = "pending";
const STATE_RUNNING: &str = "running";
const STATE_SUCCEEDED: &str = "succeeded";
const STATE_FAILED: &str = "failed";
const STATE_CANCELLED: &str = "cancelled";
const MAX_RETAINED_JOBS: usize = 4096;

#[derive(Clone)]
pub struct ManagedProverService {
    state: Arc<ServiceState>,
}

struct ServiceState {
    verifier: ManagedProofAuthorizationVerifier,
    executable: String,
    timeout: Duration,
    queue_capacity: usize,
    semaphore: Arc<Semaphore>,
    state_dir: PathBuf,
    jobs: Mutex<HashMap<String, Job>>,
    /// Provider-local copy of the operator-approved Nodepool registration.
    /// It is used only to constrain proof admission; the provider never makes
    /// settlement or billing decisions.
    trusted_managed_dsl_capabilities: BTreeMap<String, TrustedManagedDslWorkerRegistration>,
}

#[derive(Clone)]
struct Job {
    persisted: PersistedJob,
    cancellation: watch::Sender<bool>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct PersistedJob {
    job_id: String,
    #[serde(default)]
    created_at_unix_ms: i64,
    binding: ManagedProofAuthorizationBinding,
    state: String,
    status_message: String,
    response_json: Option<Vec<u8>>,
    retryable: bool,
}

impl ManagedProverService {
    pub fn from_config(config: &HivemindConfig) -> Result<Self> {
        let public_key = config.managed_proof.authorization_public_key_pem.trim();
        if public_key.is_empty() {
            anyhow::bail!(
                "MANAGED_PROOF_AUTH_PUBLIC_KEY_PEM is required for the managed prover service"
            );
        }
        let executable = config.managed_proof.provider_executable.trim();
        if executable.is_empty() {
            anyhow::bail!(
                "MANAGED_PROVER_SERVICE_EXECUTABLE is required for the managed prover service"
            );
        }
        let state_dir = PathBuf::from(&config.managed_proof.provider_state_dir);
        std::fs::create_dir_all(&state_dir).with_context(|| {
            format!(
                "create managed prover state directory {}",
                state_dir.display()
            )
        })?;
        let verifier = ManagedProofAuthorizationVerifier::from_pem(public_key)?;
        let queue_capacity = config.managed_proof.provider_queue_capacity.max(1);
        let service = Self {
            state: Arc::new(ServiceState {
                verifier,
                executable: executable.to_string(),
                timeout: Duration::from_secs(config.executor.managed_prover_timeout_secs),
                queue_capacity,
                semaphore: Arc::new(Semaphore::new(queue_capacity)),
                state_dir,
                jobs: Mutex::new(HashMap::new()),
                trusted_managed_dsl_capabilities: config
                    .general_compute
                    .trusted_managed_dsl_worker_capabilities
                    .clone(),
            }),
        };
        service.load_jobs()?;
        Ok(service)
    }

    pub fn state_dir(&self) -> &Path {
        &self.state.state_dir
    }

    fn load_jobs(&self) -> Result<()> {
        let entries = std::fs::read_dir(&self.state.state_dir)?;
        let mut jobs = self
            .state
            .jobs
            .lock()
            .map_err(|_| anyhow::anyhow!("managed prover job registry is poisoned"))?;
        for entry in entries {
            let entry = entry?;
            if entry.path().extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let bytes = std::fs::read(entry.path())?;
            let mut persisted: PersistedJob = serde_json::from_slice(&bytes)
                .with_context(|| format!("decode managed prover job {}", entry.path().display()))?;
            if let Some(response_json) = persisted.response_json.as_ref() {
                if response_json.len() > MAX_RESPONSE_JSON_BYTES {
                    anyhow::bail!("managed prover job result exceeds the response bound");
                }
            }
            if matches!(persisted.state.as_str(), STATE_PENDING | STATE_RUNNING) {
                persisted.state = STATE_FAILED.into();
                persisted.status_message = "provider restarted; resubmit the proof request".into();
                persisted.retryable = true;
                self.persist_job(&persisted)?;
            }
            let (cancellation, _) = watch::channel(false);
            jobs.insert(
                persisted.job_id.clone(),
                Job {
                    persisted,
                    cancellation,
                },
            );
        }
        Ok(())
    }

    fn persist_job(&self, job: &PersistedJob) -> Result<()> {
        let target = self.state.state_dir.join(format!("{}.json", job.job_id));
        let temporary = self
            .state
            .state_dir
            .join(format!("{}.json.tmp", job.job_id));
        let bytes = serde_json::to_vec(job)?;
        std::fs::write(&temporary, bytes)?;
        #[cfg(windows)]
        if target.exists() {
            std::fs::remove_file(&target)?;
        }
        std::fs::rename(temporary, target)?;
        Ok(())
    }

    fn update_job(&self, job_id: &str, update: impl FnOnce(&mut PersistedJob)) -> Result<()> {
        let mut jobs = self
            .state
            .jobs
            .lock()
            .map_err(|_| anyhow::anyhow!("managed prover job registry is poisoned"))?;
        let job = jobs
            .get_mut(job_id)
            .ok_or_else(|| anyhow::anyhow!("managed prover job disappeared"))?;
        update(&mut job.persisted);
        self.persist_job(&job.persisted)
    }

    fn prune_terminal_jobs(&self, jobs: &mut HashMap<String, Job>) -> Result<()> {
        let terminal_count = jobs
            .values()
            .filter(|job| !matches!(job.persisted.state.as_str(), STATE_PENDING | STATE_RUNNING))
            .count();
        if terminal_count <= MAX_RETAINED_JOBS {
            return Ok(());
        }
        let remove_count = terminal_count - MAX_RETAINED_JOBS;
        let mut candidates = jobs
            .values()
            .filter(|job| !matches!(job.persisted.state.as_str(), STATE_PENDING | STATE_RUNNING))
            .map(|job| {
                (
                    job.persisted.created_at_unix_ms,
                    job.persisted.job_id.clone(),
                )
            })
            .collect::<Vec<_>>();
        candidates.sort_unstable();
        for (_, job_id) in candidates.into_iter().take(remove_count) {
            jobs.remove(&job_id);
            let path = self.state.state_dir.join(format!("{job_id}.json"));
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }
    fn spawn_job(&self, job_id: String, request: RemoteManagedProofRequest) {
        let service = self.clone();
        tokio::spawn(async move {
            if let Err(error) = service.run_job(job_id, request).await {
                tracing::warn!("managed prover job failed internally: {error:#}");
            }
        });
    }

    async fn run_job(&self, job_id: String, request: RemoteManagedProofRequest) -> Result<()> {
        self.update_job(&job_id, |job| {
            job.state = STATE_RUNNING.into();
            job.status_message = "proof generation in progress".into();
        })?;
        let cancellation = {
            let jobs = self
                .state
                .jobs
                .lock()
                .map_err(|_| anyhow::anyhow!("managed prover job registry is poisoned"))?;
            jobs.get(&job_id)
                .map(|job| job.cancellation.subscribe())
                .ok_or_else(|| anyhow::anyhow!("managed prover job disappeared"))?
        };
        let result = self.run_sidecar(&request, cancellation).await;
        match result {
            Ok(response) => {
                let response_json = response
                    .to_json_bytes()
                    .map_err(|error| anyhow::anyhow!(error))?;
                self.update_job(&job_id, |job| {
                    job.state = STATE_SUCCEEDED.into();
                    job.status_message = "proof generated".into();
                    job.response_json = Some(response_json);
                    job.retryable = false;
                })?;
            }
            Err(SidecarError::Cancelled) => {
                self.update_job(&job_id, |job| {
                    job.state = STATE_CANCELLED.into();
                    job.status_message = "proof generation cancelled".into();
                    job.retryable = true;
                })?;
            }
            Err(error) => {
                self.update_job(&job_id, |job| {
                    job.state = STATE_FAILED.into();
                    job.status_message = "managed proof generation failed".into();
                    job.retryable = error.retryable();
                })?;
            }
        }
        Ok(())
    }

    async fn run_sidecar(
        &self,
        request: &RemoteManagedProofRequest,
        mut cancellation: watch::Receiver<bool>,
    ) -> Result<ManagedProverResponse, SidecarError> {
        let permit = tokio::select! {
            permit = self.state.semaphore.clone().acquire_owned() =>
                permit.map_err(|_| SidecarError::Unavailable)?,
            _ = wait_for_cancel(&mut cancellation) => return Err(SidecarError::Cancelled),
        };
        let sidecar_request = ManagedProverRequest {
            protocol_version: request.protocol_version,
            task_id: request.proof_task_id.clone(),
            source: request.source.clone(),
            input: request.input.clone(),
            max_usage_units: request.max_usage_units,
        };
        let timeout = self.request_timeout(request)?;
        let request_json = sidecar_request
            .to_json_bytes()
            .map_err(|_| SidecarError::InvalidRequest)?;
        let mut child = Command::new(&self.state.executable);
        child
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut child = child.spawn().map_err(|_| SidecarError::Unavailable)?;
        let stdin = child.stdin.take().ok_or(SidecarError::Unavailable)?;
        let stdout = child.stdout.take().ok_or(SidecarError::Unavailable)?;
        let result = {
            let operation = async {
                let ((), output) =
                    tokio::try_join!(write_request(stdin, &request_json), read_stdout(stdout))
                        .map_err(|_| SidecarError::Failed)?;
                let status = child.wait().await.map_err(|_| SidecarError::Failed)?;
                if !status.success() {
                    return Err(SidecarError::Failed);
                }
                ManagedProverResponse::from_json_bytes(&output).map_err(|_| SidecarError::Failed)
            };
            tokio::pin!(operation);
            let timeout = tokio::time::sleep(timeout);
            tokio::pin!(timeout);
            tokio::select! {
                result = &mut operation => result,
                _ = &mut timeout => Err(SidecarError::Timeout),
                _ = wait_for_cancel(&mut cancellation) => Err(SidecarError::Cancelled),
            }
        };
        if result.is_err() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        drop(permit);
        result
    }

    fn request_timeout(
        &self,
        request: &RemoteManagedProofRequest,
    ) -> Result<Duration, SidecarError> {
        let now = chrono::Utc::now().timestamp_millis();
        let remaining_ms = request
            .deadline_unix_ms
            .checked_sub(now)
            .filter(|remaining| *remaining > 0)
            .ok_or(SidecarError::Timeout)?;
        let remaining_ms = u64::try_from(remaining_ms).map_err(|_| SidecarError::Timeout)?;
        Ok(self.state.timeout.min(Duration::from_millis(remaining_ms)))
    }

    fn server_tls_config(config: &HivemindConfig) -> Result<ServerTlsConfig> {
        let cert_path = config.managed_proof.provider_tls_server_cert_path.trim();
        let key_path = config.managed_proof.provider_tls_server_key_path.trim();
        let client_ca_path = config.managed_proof.provider_tls_client_ca_path.trim();
        if cert_path.is_empty() || key_path.is_empty() || client_ca_path.is_empty() {
            anyhow::bail!(
                "managed prover mTLS requires MANAGED_PROVER_TLS_SERVER_CERT_PATH, \
MANAGED_PROVER_TLS_SERVER_KEY_PATH, and MANAGED_PROVER_TLS_CLIENT_CA_PATH"
            );
        }
        let certificate = std::fs::read(cert_path)
            .with_context(|| format!("read managed prover server certificate {cert_path}"))?;
        let private_key = std::fs::read(key_path)
            .with_context(|| format!("read managed prover server key {key_path}"))?;
        let client_ca = std::fs::read(client_ca_path)
            .with_context(|| format!("read managed prover client CA {client_ca_path}"))?;
        Ok(ServerTlsConfig::new()
            .identity(Identity::from_pem(certificate, private_key))
            .client_ca_root(Certificate::from_pem(client_ca)))
    }

    fn authorize<T>(
        &self,
        request: &Request<T>,
        payload: &RemoteManagedProofRequest,
    ) -> Result<(), Box<Status>> {
        let token = authorization_token(request)?;
        self.state
            .verifier
            .decode_for_request(&token, payload)
            .map(|_| ())
            .map_err(|_| {
                Box::new(Status::permission_denied(
                    "managed proof authorization rejected",
                ))
            })
    }

    fn approved_managed_dsl_capability(&self, payload: &RemoteManagedProofRequest) -> bool {
        approved_managed_dsl_capability(
            self.state
                .trusted_managed_dsl_capabilities
                .get(&payload.worker_id),
            payload,
        )
    }

    fn authorize_binding<T>(
        &self,
        request: &Request<T>,
        binding: &ManagedProofAuthorizationBinding,
    ) -> Result<(), Box<Status>> {
        let token = authorization_token(request)?;
        let claims = self.state.verifier.decode(&token).map_err(|_| {
            Box::new(Status::permission_denied(
                "managed proof authorization rejected",
            ))
        })?;
        claims.binds_binding(binding).map_err(|_| {
            Box::new(Status::permission_denied(
                "managed proof authorization rejected",
            ))
        })
    }
    fn job_response(&self, job_id: &str) -> Result<PersistedJob, Box<Status>> {
        let jobs =
            self.state.jobs.lock().map_err(|_| {
                Box::new(Status::internal("managed prover job registry unavailable"))
            })?;
        jobs.get(job_id)
            .map(|job| job.persisted.clone())
            .ok_or_else(|| Box::new(Status::not_found("managed proof job not found")))
    }
}

#[tonic::async_trait]
impl ManagedProofProvider for ManagedProverService {
    async fn get_capabilities(
        &self,
        _request: Request<GetCapabilitiesRequest>,
    ) -> Result<Response<GetCapabilitiesResponse>, Status> {
        Ok(Response::new(GetCapabilitiesResponse {
            protocol_version:
                hivemind_managed_prover_protocol::REMOTE_MANAGED_PROOF_PROTOCOL_VERSION as u32,
            proof_scheme: RISC0_PROOF_SCHEME.into(),
            image_id: RISC0_MANAGED_GUEST_ID.to_vec(),
            capabilities: advertised_managed_dsl_capabilities(
                &self.state.trusted_managed_dsl_capabilities,
            ),
            max_source_bytes: hivemind_managed_prover_protocol::MAX_SOURCE_BYTES as u64,
            max_input_bytes: hivemind_managed_prover_protocol::MAX_INPUT_BYTES as u64,
            max_response_bytes: hivemind_managed_prover_protocol::MAX_RESPONSE_JSON_BYTES as u64,
            queue_capacity: self.state.queue_capacity as u32,
            cancellation_supported: true,
        }))
    }

    async fn submit_managed_proof(
        &self,
        request: Request<SubmitManagedProofRequest>,
    ) -> Result<Response<SubmitManagedProofResponse>, Status> {
        let payload = RemoteManagedProofRequest::from_json_bytes(&request.get_ref().request_json)
            .map_err(|_| Status::invalid_argument("managed proof request is invalid"))?;
        self.authorize(&request, &payload)
            .map_err(|status| *status)?;
        if !self.approved_managed_dsl_capability(&payload) {
            return Err(Status::failed_precondition(
                "managed proof capability is not operator-approved",
            ));
        }
        if payload.proof_scheme != RISC0_PROOF_SCHEME || payload.image_id != RISC0_MANAGED_GUEST_ID
        {
            return Err(Status::failed_precondition(
                "managed proof capability does not match the requested receipt",
            ));
        }
        let mut jobs = self
            .state
            .jobs
            .lock()
            .map_err(|_| Status::internal("managed prover job registry unavailable"))?;
        self.prune_terminal_jobs(&mut jobs)
            .map_err(|_| Status::internal("managed prover state unavailable"))?;
        for existing in jobs.values() {
            if existing.persisted.binding.idempotency_key == payload.idempotency_key {
                if existing.persisted.binding.request_digest != payload.request_digest {
                    return Err(Status::already_exists("managed proof idempotency conflict"));
                }
                return Ok(Response::new(SubmitManagedProofResponse {
                    job_id: existing.persisted.job_id.clone(),
                    state: existing.persisted.state.clone(),
                    status_message: existing.persisted.status_message.clone(),
                }));
            }
        }
        let active_jobs = jobs
            .values()
            .filter(|job| matches!(job.persisted.state.as_str(), STATE_PENDING | STATE_RUNNING))
            .count();
        if active_jobs >= self.state.queue_capacity {
            return Err(Status::resource_exhausted("managed prover queue is full"));
        }
        let job_id = Uuid::new_v4().to_string();
        let persisted = PersistedJob {
            job_id: job_id.clone(),
            created_at_unix_ms: chrono::Utc::now().timestamp_millis(),
            binding: ManagedProofAuthorizationBinding::from(&payload),
            state: STATE_PENDING.into(),
            status_message: "proof queued".into(),
            response_json: None,
            retryable: true,
        };
        self.persist_job(&persisted)
            .map_err(|_| Status::internal("managed prover state unavailable"))?;
        let (cancellation, _) = watch::channel(false);
        jobs.insert(
            job_id.clone(),
            Job {
                persisted,
                cancellation,
            },
        );
        drop(jobs);
        self.spawn_job(job_id.clone(), payload);
        Ok(Response::new(SubmitManagedProofResponse {
            job_id,
            state: STATE_PENDING.into(),
            status_message: "proof queued".into(),
        }))
    }

    async fn get_managed_proof(
        &self,
        request: Request<GetManagedProofRequest>,
    ) -> Result<Response<GetManagedProofResponse>, Status> {
        let persisted = self
            .job_response(&request.get_ref().job_id)
            .map_err(|status| *status)?;
        self.authorize_binding(&request, &persisted.binding)
            .map_err(|status| *status)?;
        Ok(Response::new(GetManagedProofResponse {
            job_id: persisted.job_id,
            state: persisted.state,
            status_message: persisted.status_message,
            response_json: persisted.response_json.unwrap_or_default(),
            retryable: persisted.retryable,
        }))
    }

    async fn cancel_managed_proof(
        &self,
        request: Request<CancelManagedProofRequest>,
    ) -> Result<Response<CancelManagedProofResponse>, Status> {
        let persisted = self
            .job_response(&request.get_ref().job_id)
            .map_err(|status| *status)?;
        self.authorize_binding(&request, &persisted.binding)
            .map_err(|status| *status)?;
        let (state, cancellation) = {
            let jobs = self
                .state
                .jobs
                .lock()
                .map_err(|_| Status::internal("managed prover job registry unavailable"))?;
            let job = jobs
                .get(&persisted.job_id)
                .ok_or_else(|| Status::not_found("managed proof job not found"))?;
            (job.persisted.state.clone(), job.cancellation.clone())
        };
        let status_message = if matches!(state.as_str(), STATE_PENDING | STATE_RUNNING) {
            let _ = cancellation.send(true);
            "proof cancellation requested"
        } else {
            persisted.status_message.as_str()
        };
        Ok(Response::new(CancelManagedProofResponse {
            job_id: persisted.job_id,
            state,
            status_message: status_message.into(),
        }))
    }
}

#[derive(Debug)]
enum SidecarError {
    InvalidRequest,
    Unavailable,
    Failed,
    Timeout,
    Cancelled,
}

impl SidecarError {
    fn retryable(&self) -> bool {
        matches!(self, Self::Unavailable | Self::Timeout | Self::Cancelled)
    }
}

fn authorization_token<T>(request: &Request<T>) -> Result<String, Box<Status>> {
    let value = request
        .metadata()
        .get("authorization")
        .ok_or_else(|| {
            Box::new(Status::unauthenticated(
                "managed proof authorization is required",
            ))
        })?
        .to_str()
        .map_err(|_| {
            Box::new(Status::unauthenticated(
                "managed proof authorization is invalid",
            ))
        })?;
    let mut parts = value.split_ascii_whitespace();
    let scheme = parts.next().unwrap_or_default();
    let token = parts
        .next()
        .filter(|token| !token.is_empty())
        .filter(|_| parts.next().is_none())
        .ok_or_else(|| {
            Box::new(Status::unauthenticated(
                "managed proof authorization is invalid",
            ))
        })?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return Err(Box::new(Status::unauthenticated(
            "managed proof authorization is invalid",
        )));
    }
    Ok(token.to_string())
}

async fn wait_for_cancel(cancellation: &mut watch::Receiver<bool>) {
    if *cancellation.borrow() {
        return;
    }
    let _ = cancellation.changed().await;
}

async fn write_request(
    mut stdin: tokio::process::ChildStdin,
    request: &[u8],
) -> std::io::Result<()> {
    stdin.write_all(request).await?;
    stdin.shutdown().await
}

async fn read_stdout<R>(mut stdout: R) -> std::io::Result<Vec<u8>>
where
    R: AsyncRead + Unpin,
{
    let limit = MAX_RESPONSE_JSON_BYTES + 1;
    let mut output = Vec::with_capacity(limit.min(64 * 1024));
    let mut buffer = [0_u8; 8192];
    while output.len() < limit {
        let remaining = limit - output.len();
        let read_len = remaining.min(buffer.len());
        let read = stdout.read(&mut buffer[..read_len]).await?;
        if read == 0 {
            break;
        }
        output.extend_from_slice(&buffer[..read]);
        if output.len() == limit {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "managed prover response exceeds the bound",
            ));
        }
    }
    Ok(output)
}

fn approved_managed_dsl_capability(
    registration: Option<&TrustedManagedDslWorkerRegistration>,
    payload: &RemoteManagedProofRequest,
) -> bool {
    let Some(registration) = registration else {
        return false;
    };
    if registration.owner.trim().is_empty() || registration.owner != payload.owner {
        return false;
    }
    registration.registrations.iter().any(|approved| {
        approved.validate().is_ok()
            && payload.max_usage_units <= approved.max_usage_units
            && match payload.runtime.as_str() {
                "managed-function-v0" => {
                    payload.backend_id.is_empty() && payload.semantics_manifest_sha256.is_empty()
                }
                "production_sandboxed_dsl" => {
                    payload.backend_id == approved.backend_id
                        && payload.semantics_manifest_sha256 == approved.semantics_manifest_sha256
                }
                _ => false,
            }
    })
}

fn advertised_managed_dsl_capabilities(
    registrations: &BTreeMap<String, TrustedManagedDslWorkerRegistration>,
) -> Vec<ManagedProofCapability> {
    let mut capabilities = Vec::new();
    let mut legacy_max_usage_units = 0;
    for registration in registrations.values() {
        for approved in &registration.registrations {
            if approved.validate().is_err() {
                continue;
            }
            legacy_max_usage_units = legacy_max_usage_units.max(approved.max_usage_units);
            let capability = ManagedProofCapability {
                runtime: "production_sandboxed_dsl".into(),
                backend_id: approved.backend_id.clone(),
                semantics_manifest_sha256: approved.semantics_manifest_sha256.clone(),
                max_usage_units: approved.max_usage_units,
            };
            if !capabilities.contains(&capability) {
                capabilities.push(capability);
            }
        }
    }
    if legacy_max_usage_units > 0 {
        capabilities.push(ManagedProofCapability {
            runtime: "managed-function-v0".into(),
            backend_id: String::new(),
            semantics_manifest_sha256: String::new(),
            max_usage_units: legacy_max_usage_units,
        });
    }
    capabilities
}

pub async fn serve(config: HivemindConfig) -> Result<()> {
    let service = ManagedProverService::from_config(&config)?;
    let addr = config
        .managed_proof
        .provider_service_addr
        .parse()
        .context("managed prover service address is invalid")?;
    let tls = ManagedProverService::server_tls_config(&config)?;
    tracing::info!("managed prover service listening on {addr}");
    let provider = hivemind_proto::ManagedProofProviderServer::new(service)
        .max_decoding_message_size(hivemind_proto::MANAGED_PROVER_RPC_MESSAGE_MAX_BYTES)
        .max_encoding_message_size(hivemind_proto::MANAGED_PROVER_RPC_MESSAGE_MAX_BYTES);
    tonic::transport::Server::builder()
        .tls_config(tls)
        .context("configure managed prover mTLS")?
        .add_service(provider)
        .serve(addr)
        .await
        .context("managed prover service stopped")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authorization_metadata_requires_bearer_token() {
        let request = Request::new(());
        assert!(authorization_token(&request).is_err());
        let mut request = Request::new(());
        request
            .metadata_mut()
            .insert("authorization", "Bearer test-token".parse().unwrap());
        assert_eq!(authorization_token(&request).unwrap(), "test-token");
    }

    #[test]
    fn configured_queue_capacity_controls_provider_concurrency() {
        let (_private_key, public_key) = hivemind_config::generate_worker_execution_test_key_pair();
        let state_dir =
            std::env::temp_dir().join(format!("hivemind-managed-prover-test-{}", Uuid::new_v4()));
        let mut config = HivemindConfig::default();
        config.managed_proof.authorization_public_key_pem = public_key;
        config.managed_proof.provider_executable = "test-prover".into();
        config.managed_proof.provider_state_dir = state_dir.to_string_lossy().into_owned();
        config.managed_proof.provider_queue_capacity = 3;

        let service = ManagedProverService::from_config(&config).unwrap();
        assert_eq!(service.state.queue_capacity, 3);
        assert_eq!(service.state.semaphore.available_permits(), 3);

        drop(service);
        let _ = std::fs::remove_dir_all(state_dir);
    }

    #[test]
    fn provider_capability_admission_binds_worker_owner_and_backend() {
        let registration = TrustedManagedDslWorkerRegistration {
            owner: "owner".into(),
            registrations: vec![
                general_compute_runtime::production::ManagedDslBackendRegistration {
                    backend_id: "dsl-default".into(),
                    runtime_version: "managed-function-v0".into(),
                    semantics_manifest_sha256:
                        general_compute_runtime::MANAGED_DSL_SEMANTICS_MANIFEST_SHA256.into(),
                    max_usage_units: 10,
                    max_output_bytes: 1024,
                },
            ],
        };
        let approved = remote_request(
            "owner",
            "production_sandboxed_dsl",
            "dsl-default",
            general_compute_runtime::MANAGED_DSL_SEMANTICS_MANIFEST_SHA256,
        );
        assert!(approved_managed_dsl_capability(
            Some(&registration),
            &approved
        ));

        let mut wrong_backend = approved.clone();
        wrong_backend.backend_id = "other-backend".into();
        assert!(!approved_managed_dsl_capability(
            Some(&registration),
            &wrong_backend
        ));

        let mut wrong_owner = approved;
        wrong_owner.owner = "other-owner".into();
        assert!(!approved_managed_dsl_capability(
            Some(&registration),
            &wrong_owner
        ));
    }

    #[test]
    fn provider_capabilities_never_advertise_wildcard_backend() {
        let registration = TrustedManagedDslWorkerRegistration {
            owner: "owner".into(),
            registrations: vec![
                general_compute_runtime::production::ManagedDslBackendRegistration {
                    backend_id: "dsl-default".into(),
                    runtime_version: "managed-function-v0".into(),
                    semantics_manifest_sha256:
                        general_compute_runtime::MANAGED_DSL_SEMANTICS_MANIFEST_SHA256.into(),
                    max_usage_units: 10,
                    max_output_bytes: 1024,
                },
            ],
        };
        let mut registrations = BTreeMap::new();
        registrations.insert("worker-1".into(), registration);

        let capabilities = advertised_managed_dsl_capabilities(&registrations);
        assert!(capabilities.iter().all(|capability| {
            capability.backend_id != "*" && capability.semantics_manifest_sha256 != "*"
        }));
        assert!(capabilities.iter().any(|capability| {
            capability.runtime == "production_sandboxed_dsl"
                && capability.backend_id == "dsl-default"
        }));
    }

    fn remote_request(
        owner: &str,
        runtime: &str,
        backend_id: &str,
        semantics_manifest_sha256: &str,
    ) -> RemoteManagedProofRequest {
        RemoteManagedProofRequest {
            protocol_version:
                hivemind_managed_prover_protocol::REMOTE_MANAGED_PROOF_PROTOCOL_VERSION,
            task_id: "task-1".into(),
            proof_task_id: "proof-task-1".into(),
            owner: owner.into(),
            worker_id: "worker-1".into(),
            execution_id: "execution-1".into(),
            attempt_id: "attempt-1".into(),
            idempotency_key: "idempotency-1".into(),
            request_digest:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            lease_generation: 1,
            runtime: runtime.into(),
            backend_id: backend_id.into(),
            semantics_manifest_sha256: semantics_manifest_sha256.into(),
            source: "return 1;".into(),
            input: "null".into(),
            max_usage_units: 10,
            proof_scheme: RISC0_PROOF_SCHEME.into(),
            image_id: RISC0_MANAGED_GUEST_ID,
            deadline_unix_ms: 4_000_000_000_000,
        }
    }
}

use std::future::pending;
use std::io;
use std::process::Stdio;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use hivemind_auth::managed_proof::MANAGED_PROOF_AUTH_TOKEN_MAX_BYTES;
use hivemind_config::HivemindConfig;
use hivemind_managed_proof::dsl_proof_task_id;
use hivemind_managed_proof::{RISC0_MANAGED_GUEST_ID, RISC0_PROOF_SCHEME};
use hivemind_managed_prover_protocol::{
    ManagedProverRequest, ManagedProverResponse, RemoteManagedProofRequest,
    MANAGED_PROVER_PROTOCOL_VERSION, MAX_RESPONSE_JSON_BYTES,
    REMOTE_MANAGED_PROOF_PROTOCOL_VERSION,
};
use hivemind_models::Task;
use hivemind_proto::{
    managedprover::{
        managed_proof_provider_client::ManagedProofProviderClient, CancelManagedProofRequest,
        GetCapabilitiesRequest, GetManagedProofRequest, SubmitManagedProofRequest,
    },
    ManagedProofEnvelope, MANAGED_PROOF_RPC_MESSAGE_MAX_BYTES,
};
use prost::Message;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{watch, OwnedSemaphorePermit, Semaphore};
use tonic::metadata::MetadataValue;
use tonic::transport::{Certificate, ClientTlsConfig, Endpoint, Identity};
use tonic::Request;

static PROCESS_PROVER_SEMAPHORE: OnceLock<Arc<Semaphore>> = OnceLock::new();

/// Public failures intentionally reveal no child-process detail or task contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ManagedProverError {
    #[error("managed prover queue is full")]
    QueueFull,
    #[error("managed proof generation failed")]
    Failed,
}

/// Context that Nodepool binds to one managed-proof attempt. The bearer token
/// is kept only in memory while the Worker forwards the request to the provider.
#[derive(Clone, Debug)]
pub struct ManagedProofTaskContext {
    pub owner: String,
    pub worker_id: String,
    pub execution_id: String,
    pub attempt_id: String,
    pub idempotency_key: String,
    pub request_digest: String,
    pub lease_generation: i64,
    pub authorization_token: String,
    pub deadline_unix_ms: i64,
}

#[derive(Clone, Debug)]
struct RemoteProviderConfig {
    endpoint: String,
    ca_path: String,
    client_cert_path: String,
    client_key_path: String,
}

/// Bounded adapter for the isolated managed-prover sidecar or remote provider.
///
/// The executable is treated as a single program path: no shell or implicit
/// argument splitting is involved. The process-wide semaphore ensures a worker
/// process never runs more than one prover child at a time.
pub struct ManagedProverExecutor {
    executable: String,
    timeout: Duration,
    semaphore: Arc<Semaphore>,
    remote: Option<RemoteProviderConfig>,
}

impl ManagedProverExecutor {
    pub fn new(config: &HivemindConfig) -> Self {
        let endpoint = config.managed_proof.provider_endpoint.trim();
        Self {
            executable: config.executor.managed_prover_executable.clone(),
            timeout: Duration::from_secs(config.executor.managed_prover_timeout_secs),
            semaphore: process_prover_semaphore(),
            remote: (!endpoint.is_empty()).then(|| RemoteProviderConfig {
                endpoint: endpoint.to_string(),
                ca_path: config.managed_proof.provider_tls_ca_path.clone(),
                client_cert_path: config.managed_proof.provider_tls_client_cert_path.clone(),
                client_key_path: config.managed_proof.provider_tls_client_key_path.clone(),
            }),
        }
    }

    /// Legacy/local-provider entry point retained for tests and in-process callers.
    pub async fn prove(
        &self,
        task: &Task,
        cancellation: watch::Receiver<bool>,
    ) -> Result<ManagedProofEnvelope, ManagedProverError> {
        self.prove_with_context(task, cancellation, None).await
    }

    /// Prove a managed task through the explicitly configured provider.
    ///
    /// An explicitly configured remote provider is authoritative: failures do
    /// not fall back to a local executable, because doing so would bypass the
    /// operator-approved capability and task authorization boundary.
    pub async fn prove_with_context(
        &self,
        task: &Task,
        cancellation: watch::Receiver<bool>,
        context: Option<ManagedProofTaskContext>,
    ) -> Result<ManagedProofEnvelope, ManagedProverError> {
        if let Some(remote) = &self.remote {
            let context = context.ok_or(ManagedProverError::Failed)?;
            return self.prove_remote(remote, task, context, cancellation).await;
        }
        self.prove_local(task, cancellation).await
    }

    async fn prove_local(
        &self,
        task: &Task,
        mut cancellation: watch::Receiver<bool>,
    ) -> Result<ManagedProofEnvelope, ManagedProverError> {
        let request = request_for_task(task)?;
        let request_json = request
            .to_json_bytes()
            .map_err(|_| ManagedProverError::Failed)?;

        if self.executable.trim().is_empty() || is_cancelled(&cancellation) {
            return Err(ManagedProverError::Failed);
        }

        let permit = self.try_acquire_prover_slot()?;
        if is_cancelled(&cancellation) {
            return Err(ManagedProverError::Failed);
        }

        let mut command = Command::new(&self.executable);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let child = command.spawn().map_err(|_| ManagedProverError::Failed)?;
        let mut child = ChildCleanupGuard::new(child, permit);

        let Some(stdin) = child.child_mut().and_then(|child| child.stdin.take()) else {
            child.terminate_and_reap().await;
            return Err(ManagedProverError::Failed);
        };
        let Some(stdout) = child.child_mut().and_then(|child| child.stdout.take()) else {
            child.terminate_and_reap().await;
            return Err(ManagedProverError::Failed);
        };

        let completion = {
            let operation = async {
                let ((), output) = tokio::try_join!(
                    write_request(stdin, &request_json),
                    read_stdout_capped(stdout),
                )
                .map_err(|_| ManagedProverError::Failed)?;

                let status = child
                    .child_mut()
                    .ok_or(ManagedProverError::Failed)?
                    .wait()
                    .await
                    .map_err(|_| ManagedProverError::Failed)?;
                if !status.success() {
                    return Err(ManagedProverError::Failed);
                }

                Ok(output)
            };
            tokio::pin!(operation);
            let timeout = tokio::time::sleep(self.timeout);
            tokio::pin!(timeout);

            tokio::select! {
                result = &mut operation => ProverCompletion::Finished(result),
                _ = &mut timeout => ProverCompletion::TimedOut,
                _ = wait_for_cancellation(&mut cancellation) => ProverCompletion::Cancelled,
            }
        };

        let output = match completion {
            ProverCompletion::Finished(Ok(output)) => {
                child.disarm_after_reap();
                output
            }
            ProverCompletion::Finished(Err(error)) => {
                child.terminate_and_reap().await;
                return Err(error);
            }
            ProverCompletion::TimedOut | ProverCompletion::Cancelled => {
                child.terminate_and_reap().await;
                return Err(ManagedProverError::Failed);
            }
        };

        let response = ManagedProverResponse::from_json_bytes(&output)
            .map_err(|_| ManagedProverError::Failed)?;
        envelope_from_response(response)
    }

    async fn prove_remote(
        &self,
        remote: &RemoteProviderConfig,
        task: &Task,
        context: ManagedProofTaskContext,
        mut cancellation: watch::Receiver<bool>,
    ) -> Result<ManagedProofEnvelope, ManagedProverError> {
        if is_cancelled(&cancellation) {
            return Err(ManagedProverError::Failed);
        }
        let request = remote_request_for_task(task, &context)?;
        let request_json = request
            .to_json_bytes()
            .map_err(|_| ManagedProverError::Failed)?;
        let mut client =
            connect_remote_provider(remote, self.timeout, request.deadline_unix_ms).await?;

        let capabilities = rpc_with_deadline(
            request.deadline_unix_ms,
            client.get_capabilities(Request::new(GetCapabilitiesRequest {})),
        )
        .await
        .map_err(|_| ManagedProverError::Failed)?
        .map_err(|_| ManagedProverError::Failed)?
        .into_inner();
        validate_capabilities(&capabilities, &request)?;

        let mut submit = Request::new(SubmitManagedProofRequest { request_json });
        add_authorization(&mut submit, &context.authorization_token)?;
        let submitted = rpc_with_deadline(
            request.deadline_unix_ms,
            client.submit_managed_proof(submit),
        )
        .await
        .map_err(|_| ManagedProverError::Failed)?
        .map_err(|status| {
            if status.code() == tonic::Code::ResourceExhausted {
                ManagedProverError::QueueFull
            } else {
                ManagedProverError::Failed
            }
        })?
        .into_inner();
        validate_job_id(&submitted.job_id)?;

        let mut state = submitted.state;
        loop {
            if is_cancelled(&cancellation) {
                let _ = cancel_remote_job(
                    &mut client,
                    &submitted.job_id,
                    &context.authorization_token,
                    request.deadline_unix_ms,
                )
                .await;
                return Err(ManagedProverError::Failed);
            }
            if state == "failed" || state == "cancelled" {
                return Err(ManagedProverError::Failed);
            }

            let mut get = Request::new(GetManagedProofRequest {
                job_id: submitted.job_id.clone(),
            });
            add_authorization(&mut get, &context.authorization_token)?;
            let response =
                rpc_with_deadline(request.deadline_unix_ms, client.get_managed_proof(get))
                    .await
                    .map_err(|_| ManagedProverError::Failed)?
                    .map_err(|_| ManagedProverError::Failed)?
                    .into_inner();
            validate_job_id(&response.job_id)?;
            if response.job_id != submitted.job_id {
                return Err(ManagedProverError::Failed);
            }
            state = response.state;
            match state.as_str() {
                "succeeded" => {
                    let proof = ManagedProverResponse::from_json_bytes(&response.response_json)
                        .map_err(|_| ManagedProverError::Failed)?;
                    return envelope_from_response(proof);
                }
                "failed" | "cancelled" => return Err(ManagedProverError::Failed),
                "pending" | "running" => {}
                _ => return Err(ManagedProverError::Failed),
            }

            let sleep_for =
                remaining_until(request.deadline_unix_ms)?.min(Duration::from_millis(250));
            tokio::select! {
                _ = tokio::time::sleep(sleep_for) => {}
                _ = wait_for_cancellation(&mut cancellation) => {
                    let _ = cancel_remote_job(
                        &mut client,
                        &submitted.job_id,
                        &context.authorization_token,
                        request.deadline_unix_ms,
                    ).await;
                    return Err(ManagedProverError::Failed);
                }
            }
        }
    }

    #[cfg(test)]
    fn with_parts(executable: String, timeout: Duration, semaphore: Arc<Semaphore>) -> Self {
        Self {
            executable,
            timeout,
            semaphore,
            remote: None,
        }
    }

    fn try_acquire_prover_slot(&self) -> Result<OwnedSemaphorePermit, ManagedProverError> {
        self.semaphore
            .clone()
            .try_acquire_owned()
            .map_err(|_| ManagedProverError::QueueFull)
    }
}

async fn connect_remote_provider(
    remote: &RemoteProviderConfig,
    timeout: Duration,
    deadline_unix_ms: i64,
) -> Result<ManagedProofProviderClient<tonic::transport::Channel>, ManagedProverError> {
    if !remote.endpoint.starts_with("https://") {
        return Err(ManagedProverError::Failed);
    }
    if remote.ca_path.trim().is_empty()
        || remote.client_cert_path.trim().is_empty()
        || remote.client_key_path.trim().is_empty()
    {
        return Err(ManagedProverError::Failed);
    }
    let ca = std::fs::read(&remote.ca_path).map_err(|_| ManagedProverError::Failed)?;
    let certificate =
        std::fs::read(&remote.client_cert_path).map_err(|_| ManagedProverError::Failed)?;
    let private_key =
        std::fs::read(&remote.client_key_path).map_err(|_| ManagedProverError::Failed)?;
    let tls = ClientTlsConfig::new()
        .ca_certificate(Certificate::from_pem(ca))
        .identity(Identity::from_pem(certificate, private_key));
    let connect_timeout = timeout.min(remaining_until(deadline_unix_ms)?);
    let endpoint = Endpoint::from_shared(remote.endpoint.clone())
        .map_err(|_| ManagedProverError::Failed)?
        .connect_timeout(connect_timeout)
        .timeout(connect_timeout)
        .tls_config(tls)
        .map_err(|_| ManagedProverError::Failed)?;
    ManagedProofProviderClient::connect(endpoint)
        .await
        .map_err(|_| ManagedProverError::Failed)
}

fn add_authorization<T>(request: &mut Request<T>, token: &str) -> Result<(), ManagedProverError> {
    if token.trim().is_empty() || token.len() > MANAGED_PROOF_AUTH_TOKEN_MAX_BYTES {
        return Err(ManagedProverError::Failed);
    }
    let value = MetadataValue::try_from(format!("Bearer {token}"))
        .map_err(|_| ManagedProverError::Failed)?;
    request.metadata_mut().insert("authorization", value);
    Ok(())
}

fn validate_capabilities(
    response: &hivemind_proto::managedprover::GetCapabilitiesResponse,
    request: &RemoteManagedProofRequest,
) -> Result<(), ManagedProverError> {
    if response.protocol_version != REMOTE_MANAGED_PROOF_PROTOCOL_VERSION as u32
        || response.proof_scheme != RISC0_PROOF_SCHEME
        || response.image_id != RISC0_MANAGED_GUEST_ID
        || response.max_source_bytes < request.source.len() as u64
        || response.max_input_bytes < request.input.len() as u64
        || response.max_response_bytes < MAX_RESPONSE_JSON_BYTES as u64
        || !response.cancellation_supported
    {
        return Err(ManagedProverError::Failed);
    }
    let matching = response.capabilities.iter().any(|capability| {
        capability.runtime == request.runtime
            && capability.max_usage_units >= request.max_usage_units
            && if request.runtime == "production_sandboxed_dsl" {
                (capability.backend_id == "*" || capability.backend_id == request.backend_id)
                    && (capability.semantics_manifest_sha256 == "*"
                        || capability.semantics_manifest_sha256
                            == request.semantics_manifest_sha256)
            } else {
                capability.backend_id.is_empty() && capability.semantics_manifest_sha256.is_empty()
            }
    });
    matching.then_some(()).ok_or(ManagedProverError::Failed)
}

fn validate_job_id(job_id: &str) -> Result<(), ManagedProverError> {
    if job_id.trim().is_empty() || job_id.len() > 255 {
        return Err(ManagedProverError::Failed);
    }
    Ok(())
}

async fn cancel_remote_job(
    client: &mut ManagedProofProviderClient<tonic::transport::Channel>,
    job_id: &str,
    token: &str,
    deadline_unix_ms: i64,
) -> Result<(), ManagedProverError> {
    let mut request = Request::new(CancelManagedProofRequest {
        job_id: job_id.to_string(),
    });
    add_authorization(&mut request, token)?;
    rpc_with_deadline(deadline_unix_ms, client.cancel_managed_proof(request))
        .await
        .map_err(|_| ManagedProverError::Failed)?
        .map_err(|_| ManagedProverError::Failed)?;
    Ok(())
}

async fn rpc_with_deadline<T, F>(
    deadline_unix_ms: i64,
    future: F,
) -> Result<Result<T, tonic::Status>, ManagedProverError>
where
    F: std::future::Future<Output = Result<T, tonic::Status>>,
{
    let timeout = remaining_until(deadline_unix_ms)?;
    tokio::time::timeout(timeout, future)
        .await
        .map_err(|_| ManagedProverError::Failed)
}

enum ProverCompletion {
    Finished(Result<Vec<u8>, ManagedProverError>),
    TimedOut,
    Cancelled,
}

/// Owns a spawned sidecar and keeps its permit until the process is reaped.
///
/// An aborted proof future drops this guard. Its Drop implementation starts
/// termination synchronously, then moves the child and permit into a native
/// reaper thread that is independent of Tokio runtime shutdown.
struct ChildCleanupGuard {
    child: Option<Child>,
    permit: Option<OwnedSemaphorePermit>,
}

impl ChildCleanupGuard {
    fn new(child: Child, permit: OwnedSemaphorePermit) -> Self {
        Self {
            child: Some(child),
            permit: Some(permit),
        }
    }

    fn child_mut(&mut self) -> Option<&mut Child> {
        self.child.as_mut()
    }

    async fn terminate_and_reap(&mut self) {
        if let Some(child) = self.child.as_mut() {
            terminate_child(child).await;
        }
        self.child.take();
        self.permit.take();
    }

    fn disarm_after_reap(&mut self) {
        self.child.take();
        self.permit.take();
    }
}

impl Drop for ChildCleanupGuard {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            self.permit.take();
            return;
        };
        let permit = self.permit.take();

        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        let _ = child.start_kill();

        let cleanup = Arc::new(Mutex::new(Some((child, permit))));
        let reaper_cleanup = Arc::clone(&cleanup);
        let spawned = std::thread::Builder::new()
            .name("managed-prover-reaper".into())
            .spawn(move || {
                if let Some((child, permit)) = take_cleanup_state(&reaper_cleanup) {
                    reap_child_blocking(child, permit);
                }
            });

        if spawned.is_err() {
            if let Some((child, permit)) = take_cleanup_state(&cleanup) {
                reap_child_blocking(child, permit);
            }
        }
    }
}

fn take_cleanup_state(
    cleanup: &Mutex<Option<(Child, Option<OwnedSemaphorePermit>)>>,
) -> Option<(Child, Option<OwnedSemaphorePermit>)> {
    let mut cleanup = match cleanup.lock() {
        Ok(cleanup) => cleanup,
        Err(poisoned) => poisoned.into_inner(),
    };
    cleanup.take()
}

fn reap_child_blocking(mut child: Child, permit: Option<OwnedSemaphorePermit>) {
    let _ = child.start_kill();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) | Err(_) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    drop(permit);
}

fn process_prover_semaphore() -> Arc<Semaphore> {
    PROCESS_PROVER_SEMAPHORE
        .get_or_init(|| Arc::new(Semaphore::new(1)))
        .clone()
}

fn remote_request_for_task(
    task: &Task,
    context: &ManagedProofTaskContext,
) -> Result<RemoteManagedProofRequest, ManagedProverError> {
    if context.owner != task.owner
        || task.worker_id.as_deref() != Some(context.worker_id.as_str())
        || context.authorization_token.trim().is_empty()
        || context.authorization_token.len() > MANAGED_PROOF_AUTH_TOKEN_MAX_BYTES
    {
        return Err(ManagedProverError::Failed);
    }
    let sidecar_request = request_for_task(task)?;
    let request = RemoteManagedProofRequest {
        protocol_version: REMOTE_MANAGED_PROOF_PROTOCOL_VERSION,
        task_id: task.task_id.clone(),
        proof_task_id: sidecar_request.task_id,
        owner: context.owner.clone(),
        worker_id: context.worker_id.clone(),
        execution_id: context.execution_id.clone(),
        attempt_id: context.attempt_id.clone(),
        idempotency_key: context.idempotency_key.clone(),
        request_digest: String::new(),
        lease_generation: context.lease_generation,
        runtime: task.runtime.clone().unwrap_or_default(),
        backend_id: task.managed_dsl_backend_id.clone().unwrap_or_default(),
        semantics_manifest_sha256: task
            .managed_dsl_semantics_manifest_sha256
            .clone()
            .unwrap_or_default(),
        source: sidecar_request.source,
        input: sidecar_request.input,
        max_usage_units: sidecar_request.max_usage_units,
        proof_scheme: RISC0_PROOF_SCHEME.into(),
        image_id: RISC0_MANAGED_GUEST_ID,
        deadline_unix_ms: context.deadline_unix_ms,
    };
    let request = request
        .with_computed_digest()
        .map_err(|_| ManagedProverError::Failed)?;
    if context.request_digest != request.request_digest {
        return Err(ManagedProverError::Failed);
    }
    Ok(request)
}

fn remaining_until(deadline_unix_ms: i64) -> Result<Duration, ManagedProverError> {
    remaining_until_at(deadline_unix_ms, chrono::Utc::now().timestamp_millis())
}

fn remaining_until_at(
    deadline_unix_ms: i64,
    now_unix_ms: i64,
) -> Result<Duration, ManagedProverError> {
    let remaining_ms = deadline_unix_ms
        .checked_sub(now_unix_ms)
        .filter(|remaining| *remaining > 0)
        .ok_or(ManagedProverError::Failed)?;
    let remaining_ms = u64::try_from(remaining_ms).map_err(|_| ManagedProverError::Failed)?;
    Ok(Duration::from_millis(remaining_ms))
}

fn request_for_task(task: &Task) -> Result<ManagedProverRequest, ManagedProverError> {
    if task.max_cpt <= 0 {
        return Err(ManagedProverError::Failed);
    }

    let task_id = if task.runtime.as_deref() == Some("production_sandboxed_dsl") {
        let backend_id = task
            .managed_dsl_backend_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or(ManagedProverError::Failed)?;
        let semantics_digest = task
            .managed_dsl_semantics_manifest_sha256
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or(ManagedProverError::Failed)?;
        dsl_proof_task_id(
            &task.task_id,
            "production_sandboxed_dsl",
            backend_id,
            semantics_digest,
        )
    } else {
        task.task_id.clone()
    };
    let request = ManagedProverRequest {
        protocol_version: MANAGED_PROVER_PROTOCOL_VERSION,
        task_id,
        source: task.task_source.clone().unwrap_or_default(),
        input: task.torrent_source.clone().unwrap_or_default(),
        max_usage_units: task.max_cpt as u64,
    };
    request.validate().map_err(|_| ManagedProverError::Failed)?;
    Ok(request)
}

fn envelope_from_response(
    response: ManagedProverResponse,
) -> Result<ManagedProofEnvelope, ManagedProverError> {
    response
        .validate()
        .map_err(|_| ManagedProverError::Failed)?;

    let envelope = ManagedProofEnvelope {
        proof_scheme: response.proof_scheme,
        image_id: response.image_id.to_vec(),
        journal: response.journal,
        receipt_json: response.receipt_json.into_bytes(),
    };
    ensure_envelope_fits_rpc_boundary(&envelope)?;
    Ok(envelope)
}

fn ensure_envelope_fits_rpc_boundary(
    envelope: &ManagedProofEnvelope,
) -> Result<(), ManagedProverError> {
    if envelope.encoded_len() > MANAGED_PROOF_RPC_MESSAGE_MAX_BYTES {
        return Err(ManagedProverError::Failed);
    }
    Ok(())
}

async fn write_request(mut stdin: ChildStdin, request: &[u8]) -> io::Result<()> {
    stdin.write_all(request).await?;
    stdin.shutdown().await
}

async fn read_stdout_capped<R>(mut stdout: R) -> io::Result<Vec<u8>>
where
    R: AsyncRead + Unpin,
{
    const OUTPUT_LIMIT: usize = MAX_RESPONSE_JSON_BYTES + 1;

    let mut output = Vec::with_capacity(OUTPUT_LIMIT);
    let mut buffer = [0_u8; 8192];
    while output.len() < OUTPUT_LIMIT {
        let remaining = OUTPUT_LIMIT - output.len();
        let chunk_len = remaining.min(buffer.len());
        let read = stdout.read(&mut buffer[..chunk_len]).await?;
        if read == 0 {
            break;
        }
        output.extend_from_slice(&buffer[..read]);
        if output.len() == OUTPUT_LIMIT {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "managed prover response exceeds the response size limit",
            ));
        }
    }
    Ok(output)
}

fn is_cancelled(cancellation: &watch::Receiver<bool>) -> bool {
    *cancellation.borrow()
}

async fn wait_for_cancellation(cancellation: &mut watch::Receiver<bool>) {
    loop {
        if *cancellation.borrow_and_update() {
            return;
        }
        if cancellation.changed().await.is_err() {
            pending::<()>().await;
        }
    }
}

async fn terminate_child(child: &mut Child) {
    if !matches!(child.try_wait(), Ok(Some(_))) {
        let _ = child.start_kill();
    }
    let _ = child.wait().await;
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io;
    use std::sync::Arc;
    use std::time::Duration;

    use chrono::Utc;
    use hivemind_config::HivemindConfig;
    use hivemind_managed_prover_protocol::{
        ManagedProverResponse, MANAGED_PROVER_PROTOCOL_VERSION, MAX_INPUT_BYTES,
        MAX_RESPONSE_JSON_BYTES,
    };
    use hivemind_models::{Task, TaskStatus};
    use hivemind_proto::{ManagedProofEnvelope, MANAGED_PROOF_RPC_MESSAGE_MAX_BYTES};
    use tempfile::TempDir;
    use tokio::io::AsyncWriteExt;
    use tokio::sync::{watch, Semaphore};
    use uuid::Uuid;

    use super::{
        envelope_from_response, read_stdout_capped, remaining_until_at, request_for_task,
        ManagedProverError, ManagedProverExecutor,
    };

    const VALID_RESPONSE: &str = r#"{"protocol_version":1,"proof_scheme":"test-proof","image_id":[1,2,3,4,5,6,7,8],"journal":[4,5,6],"receipt_json":"{\"seal\":\"ok\"}"}"#;

    #[test]
    fn absolute_deadline_remaining_time_is_checked_at_call_time() {
        assert_eq!(remaining_until_at(2_000, 1_000), Ok(Duration::from_secs(1)));
        assert_eq!(
            remaining_until_at(2_000, 2_000),
            Err(ManagedProverError::Failed)
        );
        assert_eq!(
            remaining_until_at(2_000, 2_001),
            Err(ManagedProverError::Failed)
        );
        assert_eq!(
            remaining_until_at(i64::MAX, i64::MIN),
            Err(ManagedProverError::Failed)
        );
    }

    #[test]
    fn builds_a_validated_request_from_the_task_contract() {
        let task = test_task();

        let request = request_for_task(&task).expect("task produces a valid prover request");

        assert_eq!(request.protocol_version, MANAGED_PROVER_PROTOCOL_VERSION);
        assert_eq!(request.task_id, task.task_id);
        assert_eq!(
            request.source,
            task.task_source.expect("test task has source")
        );
        assert_eq!(
            request.input,
            task.torrent_source.expect("test task has input")
        );
        assert_eq!(request.max_usage_units, task.max_cpt as u64);
    }

    #[test]
    fn production_dsl_request_authenticates_task_identity_in_proved_task_id() {
        let mut task = test_task();
        task.runtime = Some("production_sandboxed_dsl".into());
        task.managed_dsl_backend_id = Some("managed-default".into());
        task.managed_dsl_semantics_manifest_sha256 =
            Some(general_compute_runtime::MANAGED_DSL_SEMANTICS_MANIFEST_SHA256.into());

        let request = request_for_task(&task).expect("production DSL task produces a request");

        assert_eq!(
            request.task_id,
            hivemind_managed_proof::dsl_proof_task_id(
                &task.task_id,
                "production_sandboxed_dsl",
                "managed-default",
                general_compute_runtime::MANAGED_DSL_SEMANTICS_MANIFEST_SHA256,
            )
        );
        assert_ne!(request.task_id, task.task_id);
    }

    #[test]
    fn constructor_reads_the_sidecar_configuration() {
        let executor =
            ManagedProverExecutor::new(&config_with_executable("configured-prover".into()));

        assert_eq!(executor.executable, "configured-prover");
        assert_eq!(executor.timeout, Duration::from_secs(5));
    }

    #[test]
    fn invalid_task_contract_fails_without_exposing_source_or_input() {
        let mut task = test_task();
        task.task_source = Some("source-that-must-not-leak".into());
        task.torrent_source = Some("input-that-must-not-leak".into());

        let error = request_for_task(&task).expect_err("non-JSON input is rejected");

        assert_eq!(error, ManagedProverError::Failed);
        let public_error = error.to_string();
        assert!(!public_error.contains("source-that-must-not-leak"));
        assert!(!public_error.contains("input-that-must-not-leak"));

        task.torrent_source = Some("{}".into());
        task.max_cpt = 0;
        assert_eq!(
            request_for_task(&task),
            Err(ManagedProverError::Failed),
            "a non-positive budget fails closed"
        );
    }

    #[test]
    fn valid_prover_response_becomes_a_proof_envelope() {
        let response = ManagedProverResponse {
            protocol_version: MANAGED_PROVER_PROTOCOL_VERSION,
            proof_scheme: "test-proof".into(),
            image_id: [1, 2, 3, 4, 5, 6, 7, 8],
            journal: vec![4, 5, 6],
            receipt_json: r#"{"seal":"ok"}"#.into(),
        };

        let envelope = envelope_from_response(response).expect("valid response converts");

        assert_eq!(envelope.proof_scheme, "test-proof");
        assert_eq!(envelope.image_id, vec![1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(envelope.journal, vec![4, 5, 6]);
        assert_eq!(envelope.receipt_json, br#"{"seal":"ok"}"#);
    }

    #[test]
    fn oversized_proof_envelope_fails_closed() {
        let envelope = ManagedProofEnvelope {
            receipt_json: vec![0; MANAGED_PROOF_RPC_MESSAGE_MAX_BYTES],
            ..ManagedProofEnvelope::default()
        };

        assert_eq!(
            super::ensure_envelope_fits_rpc_boundary(&envelope),
            Err(ManagedProverError::Failed)
        );
    }

    #[tokio::test]
    async fn stdout_reader_stops_after_the_protocol_cap_plus_one_byte() {
        let (mut writer, reader) = tokio::io::duplex(4096);
        let oversized = vec![b'x'; MAX_RESPONSE_JSON_BYTES + 64];
        let producer = tokio::spawn(async move {
            let _ = writer.write_all(&oversized).await;
        });

        let error = read_stdout_capped(reader)
            .await
            .expect_err("cap-plus-one stdout byte is rejected immediately");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        producer
            .await
            .expect("producer finishes after reader closes");
    }

    #[tokio::test]
    async fn direct_fake_sidecar_response_is_parsed_and_converted() {
        let temp = TempDir::new().expect("temporary fake sidecar directory");
        let executable = fake_sidecar(&temp, "success", VALID_RESPONSE, 0);
        let executor = test_executor(executable);
        let (_, cancellation) = watch::channel(false);

        let envelope = executor
            .prove(&test_task(), cancellation)
            .await
            .expect("fake sidecar response is accepted");

        assert_eq!(envelope.proof_scheme, "test-proof");
        assert_eq!(envelope.image_id, vec![1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(envelope.journal, vec![4, 5, 6]);
        assert_eq!(envelope.receipt_json, br#"{"seal":"ok"}"#);
    }

    #[tokio::test]
    async fn malformed_or_nonzero_sidecars_fail_with_the_generic_error() {
        let temp = TempDir::new().expect("temporary fake sidecar directory");
        let (_, cancellation) = watch::channel(false);
        let malformed = test_executor(fake_sidecar(&temp, "malformed", "not-json", 0));

        assert_eq!(
            malformed.prove(&test_task(), cancellation).await,
            Err(ManagedProverError::Failed)
        );

        let (_, cancellation) = watch::channel(false);
        let nonzero = test_executor(fake_sidecar(&temp, "nonzero", VALID_RESPONSE, 7));

        assert_eq!(
            nonzero.prove(&test_task(), cancellation).await,
            Err(ManagedProverError::Failed)
        );
    }

    #[tokio::test]
    async fn unavailable_or_cancelled_sidecars_fail_with_the_generic_error() {
        let unavailable = test_executor("definitely-not-an-installed-managed-prover".into());
        let (_, cancellation) = watch::channel(false);

        assert_eq!(
            unavailable.prove(&test_task(), cancellation).await,
            Err(ManagedProverError::Failed)
        );

        let executor = test_executor("definitely-not-reached-after-cancellation".into());
        let (cancellation_tx, cancellation) = watch::channel(true);
        let _keep_sender_alive = cancellation_tx;

        assert_eq!(
            executor.prove(&test_task(), cancellation).await,
            Err(ManagedProverError::Failed)
        );
    }

    #[tokio::test]
    async fn oversized_sidecar_stdout_is_killed_reaped_and_releases_the_prover_slot() {
        let temp = TempDir::new().expect("temporary fake sidecar directory");
        let payload = temp.path().join("oversized-stdout");
        fs::write(&payload, vec![b'x'; MAX_RESPONSE_JSON_BYTES + 64])
            .expect("oversized fake stdout is written");
        let semaphore = Arc::new(Semaphore::new(1));
        let executor = ManagedProverExecutor::with_parts(
            oversized_sidecar(&temp, "oversized", &payload),
            Duration::from_secs(5),
            Arc::clone(&semaphore),
        );
        let (_, cancellation) = watch::channel(false);

        let result = tokio::time::timeout(
            Duration::from_secs(2),
            executor.prove(&test_task(), cancellation),
        )
        .await
        .expect("oversized stdout is killed and reaped promptly");

        assert_eq!(result, Err(ManagedProverError::Failed));
        assert_slot_is_reusable(&temp, semaphore).await;
    }

    #[tokio::test]
    async fn oversized_stdout_stops_a_sidecar_that_refuses_to_read_a_large_request() {
        let temp = TempDir::new().expect("temporary fake sidecar directory");
        let payload = temp.path().join("oversized-nonreading-stdout");
        fs::write(&payload, vec![b'x'; MAX_RESPONSE_JSON_BYTES + 64])
            .expect("oversized fake stdout is written");
        let semaphore = Arc::new(Semaphore::new(1));
        let executor = ManagedProverExecutor::with_parts(
            blocking_oversized_sidecar(&temp, "oversized-nonreading", &payload),
            Duration::from_secs(5),
            Arc::clone(&semaphore),
        );
        let (cancellation_tx, cancellation) = watch::channel(false);
        let task = task_with_maximum_input();
        let proving = executor.prove(&task, cancellation);
        tokio::pin!(proving);

        let result = tokio::select! {
            result = &mut proving => result,
            _ = tokio::time::sleep(Duration::from_millis(500)) => {
                cancellation_tx
                    .send(true)
                    .expect("blocked sidecar still listens for cancellation");
                let cleanup = tokio::time::timeout(Duration::from_secs(2), &mut proving)
                    .await
                    .expect("cancellation cleans up the blocked sidecar");
                assert_eq!(cleanup, Err(ManagedProverError::Failed));
                panic!("oversized stdout waited for a blocked stdin write");
            }
        };

        assert_eq!(result, Err(ManagedProverError::Failed));
        assert_slot_is_reusable(&temp, semaphore).await;
    }

    #[tokio::test]
    async fn post_spawn_cancellation_kills_reaps_and_releases_the_prover_slot() {
        let temp = TempDir::new().expect("temporary fake sidecar directory");
        let marker = temp.path().join("cancelled-sidecar-started");
        let semaphore = Arc::new(Semaphore::new(1));
        let blocking = Arc::new(ManagedProverExecutor::with_parts(
            blocking_sidecar(&temp, "cancelled", &marker),
            Duration::from_secs(5),
            Arc::clone(&semaphore),
        ));
        let task = test_task();
        let (cancellation_tx, cancellation) = watch::channel(false);
        let proving = tokio::spawn({
            let blocking = Arc::clone(&blocking);
            async move { blocking.prove(&task, cancellation).await }
        });

        wait_for_marker(&marker).await;
        cancellation_tx
            .send(true)
            .expect("sidecar still listens for cancellation");
        let result = tokio::time::timeout(Duration::from_secs(2), proving)
            .await
            .expect("cancellation kills and reaps the sidecar promptly")
            .expect("proving task joins");

        assert_eq!(result, Err(ManagedProverError::Failed));
        #[cfg(unix)]
        assert_marker_pid_is_dead(&marker);
        assert_slot_is_reusable(&temp, semaphore).await;
    }

    #[tokio::test]
    async fn aborted_prove_future_kills_reaps_and_holds_its_permit_until_cleanup() {
        let temp = TempDir::new().expect("temporary fake sidecar directory");
        let started = temp.path().join("aborted-sidecar-started");
        let heartbeat = temp.path().join("aborted-sidecar-heartbeat");
        let stop = temp.path().join("aborted-sidecar-stop");
        let semaphore = Arc::new(Semaphore::new(1));
        let executor = Arc::new(ManagedProverExecutor::with_parts(
            abortable_blocking_sidecar(&temp, "aborted", &started, &heartbeat, &stop),
            Duration::from_secs(5),
            Arc::clone(&semaphore),
        ));
        let task = test_task();
        let (_, cancellation) = watch::channel(false);
        let proving = tokio::spawn({
            let executor = Arc::clone(&executor);
            async move { executor.prove(&task, cancellation).await }
        });

        wait_for_marker(&started).await;
        wait_for_marker(&heartbeat).await;
        proving.abort();
        let _ = proving.await;

        tokio::time::sleep(Duration::from_millis(300)).await;
        let before = fs::read_to_string(&heartbeat).expect("sidecar heartbeat is readable");
        tokio::time::sleep(Duration::from_millis(100)).await;
        let after = fs::read_to_string(&heartbeat).expect("sidecar heartbeat remains readable");
        fs::write(&stop, "stop").expect("test cleanup stop marker is written");

        assert_eq!(
            before, after,
            "aborting prove must kill the child instead of leaving it running"
        );
        wait_for_slot_to_be_reusable(&temp, semaphore).await;
    }

    #[tokio::test]
    async fn post_spawn_timeout_kills_reaps_and_releases_the_prover_slot() {
        let temp = TempDir::new().expect("temporary fake sidecar directory");
        let marker = temp.path().join("timed-out-sidecar-started");
        let semaphore = Arc::new(Semaphore::new(1));
        let blocking = Arc::new(ManagedProverExecutor::with_parts(
            blocking_sidecar(&temp, "timed-out", &marker),
            Duration::from_millis(100),
            Arc::clone(&semaphore),
        ));
        let task = test_task();
        let (_, cancellation) = watch::channel(false);
        let proving = tokio::spawn({
            let blocking = Arc::clone(&blocking);
            async move { blocking.prove(&task, cancellation).await }
        });

        wait_for_marker(&marker).await;
        let result = tokio::time::timeout(Duration::from_secs(2), proving)
            .await
            .expect("timeout kills and reaps the sidecar promptly")
            .expect("proving task joins");

        assert_eq!(result, Err(ManagedProverError::Failed));
        #[cfg(unix)]
        assert_marker_pid_is_dead(&marker);
        assert_slot_is_reusable(&temp, semaphore).await;
    }

    #[tokio::test]
    async fn a_busy_process_local_prover_slot_fails_fast_with_queue_full() {
        let executor = ManagedProverExecutor::with_parts(
            "not-reached-because-the-slot-is-busy".into(),
            Duration::from_secs(1),
            Arc::new(Semaphore::new(0)),
        );
        let (_, cancellation) = watch::channel(false);

        assert_eq!(
            executor.prove(&test_task(), cancellation).await,
            Err(ManagedProverError::QueueFull)
        );
    }

    fn config_with_executable(executable: String) -> HivemindConfig {
        let mut config = HivemindConfig::default();
        config.executor.managed_prover_executable = executable;
        config.executor.managed_prover_timeout_secs = 5;
        config
    }

    fn test_executor(executable: String) -> ManagedProverExecutor {
        ManagedProverExecutor::with_parts(
            executable,
            Duration::from_secs(5),
            Arc::new(Semaphore::new(1)),
        )
    }

    fn test_task() -> Task {
        let now = Utc::now();
        Task {
            id: Uuid::new_v4(),
            task_id: "managed-prover-test".into(),
            owner: "worker-test".into(),
            worker_id: None,
            worker_ip: None,
            status: TaskStatus::Pending,
            status_message: None,
            output: None,
            result_torrent: None,
            torrent_source: Some(r#"{"value":42}"#.into()),
            runtime: Some("managed-function-v0".into()),
            task_source: Some("return input;".into()),
            general_compute_manifest_json: None,
            managed_dsl_backend_id: None,
            managed_dsl_semantics_manifest_sha256: None,
            expected_btih: None,
            cpu_usage: 0.0,
            memory_usage: 0.0,
            gpu_usage: 0.0,
            gpu_memory_usage: 0.0,
            req_cpu_score: 1,
            req_gpu_score: 0,
            req_memory_gb: 1,
            req_gpu_memory_gb: 0,
            req_storage_gb: 1,
            host_count: 1,
            max_cpt: 100,
            billing_settled: false,
            billed_amount: 0,
            managed_executed_ops: 0,
            managed_output_bytes: 0,
            managed_receipt_json: None,
            retry_count: 0,
            max_retries: 3,
            deadline: None,
            deterministic: true,
            side_effects: false,
            priority: 0,
            cpu_time_ms: 0,
            wall_time_ms: 0,
            peak_memory_mb: 0,
            download_bytes: 0,
            cache_hits: 0,
            created_at: now,
            last_update: now,
            completed_at: None,
        }
    }

    fn task_with_maximum_input() -> Task {
        let mut task = test_task();
        task.torrent_source = Some(format!(r#""{}""#, "x".repeat(MAX_INPUT_BYTES - 2)));
        task
    }

    async fn assert_slot_is_reusable(temp: &TempDir, semaphore: Arc<Semaphore>) {
        let reusable = ManagedProverExecutor::with_parts(
            fake_sidecar(temp, "reusable", VALID_RESPONSE, 0),
            Duration::from_secs(2),
            semaphore,
        );
        let (_, cancellation) = watch::channel(false);

        assert!(
            reusable.prove(&test_task(), cancellation).await.is_ok(),
            "a killed sidecar must release its permit for the next proof"
        );
    }

    async fn wait_for_slot_to_be_reusable(temp: &TempDir, semaphore: Arc<Semaphore>) {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let reusable = ManagedProverExecutor::with_parts(
                    fake_sidecar(temp, "reusable-after-abort", VALID_RESPONSE, 0),
                    Duration::from_secs(2),
                    Arc::clone(&semaphore),
                );
                let (_, cancellation) = watch::channel(false);
                match reusable.prove(&test_task(), cancellation).await {
                    Ok(_) => break,
                    Err(ManagedProverError::QueueFull) => {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                    Err(error) => panic!("unexpected reusable prover error: {error}"),
                }
            }
        })
        .await
        .expect("aborted proof eventually kills/reaps its child and releases the permit");
    }

    async fn wait_for_marker(marker: &std::path::Path) {
        tokio::time::timeout(Duration::from_secs(2), async {
            while !marker.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("fake sidecar reaches its post-stdin blocking point");
    }

    #[cfg(unix)]
    fn assert_marker_pid_is_dead(marker: &std::path::Path) {
        let pid = fs::read_to_string(marker).expect("fake sidecar writes its pid");
        let status = std::process::Command::new("kill")
            .args(["-0", pid.trim()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("kill utility is available");

        assert!(!status.success(), "sidecar process must no longer be alive");
    }

    #[cfg(unix)]
    fn fake_sidecar(temp: &TempDir, name: &str, stdout: &str, exit_code: i32) -> String {
        use std::os::unix::fs::PermissionsExt;

        let path = temp.path().join(format!("{name}.sh"));
        let escaped_stdout = stdout.replace('\'', "'\\''");
        fs::write(
            &path,
            format!(
                "#!/bin/sh\ncat >/dev/null\nprintf '%s\\n' '{escaped_stdout}'\nexit {exit_code}\n"
            ),
        )
        .expect("fake sidecar script is written");
        let mut permissions = fs::metadata(&path)
            .expect("fake sidecar metadata")
            .permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&path, permissions).expect("fake sidecar is executable");
        path.to_string_lossy().into_owned()
    }

    #[cfg(unix)]
    fn blocking_sidecar(temp: &TempDir, name: &str, marker: &std::path::Path) -> String {
        use std::os::unix::fs::PermissionsExt;

        let path = temp.path().join(format!("{name}.sh"));
        let marker = marker.to_string_lossy().replace('\'', "'\\''");
        fs::write(
            &path,
            format!(
                "#!/bin/sh\ncat >/dev/null\nprintf '%s\\n' \"$$\" > '{marker}'\nwhile :; do :; done\n"
            ),
        )
        .expect("blocking fake sidecar script is written");
        let mut permissions = fs::metadata(&path)
            .expect("blocking fake sidecar metadata")
            .permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&path, permissions).expect("blocking fake sidecar is executable");
        path.to_string_lossy().into_owned()
    }

    #[cfg(unix)]
    fn abortable_blocking_sidecar(
        temp: &TempDir,
        name: &str,
        started: &std::path::Path,
        heartbeat: &std::path::Path,
        stop: &std::path::Path,
    ) -> String {
        use std::os::unix::fs::PermissionsExt;

        let path = temp.path().join(format!("{name}.sh"));
        let started = started.to_string_lossy().replace('\'', "'\\''");
        let heartbeat = heartbeat.to_string_lossy().replace('\'', "'\\''");
        let stop = stop.to_string_lossy().replace('\'', "'\\''");
        fs::write(
            &path,
            format!(
                "#!/bin/sh\ncat >/dev/null\nprintf '%s\\n' started > '{started}'\ncount=0\nwhile :; do\n  count=$((count + 1))\n  printf '%s\\n' \"$count\" > '{heartbeat}'\n  [ -f '{stop}' ] && exit 0\ndone\n"
            ),
        )
        .expect("abortable fake sidecar script is written");
        let mut permissions = fs::metadata(&path)
            .expect("abortable fake sidecar metadata")
            .permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&path, permissions).expect("abortable fake sidecar is executable");
        path.to_string_lossy().into_owned()
    }

    #[cfg(unix)]
    fn oversized_sidecar(temp: &TempDir, name: &str, payload: &std::path::Path) -> String {
        use std::os::unix::fs::PermissionsExt;

        let path = temp.path().join(format!("{name}.sh"));
        let payload = payload.to_string_lossy().replace('\'', "'\\''");
        fs::write(&path, format!("#!/bin/sh\ncat '{payload}'\n"))
            .expect("oversized fake sidecar script is written");
        let mut permissions = fs::metadata(&path)
            .expect("oversized fake sidecar metadata")
            .permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&path, permissions).expect("oversized fake sidecar is executable");
        path.to_string_lossy().into_owned()
    }

    #[cfg(unix)]
    fn blocking_oversized_sidecar(temp: &TempDir, name: &str, payload: &std::path::Path) -> String {
        use std::os::unix::fs::PermissionsExt;

        let path = temp.path().join(format!("{name}.sh"));
        let payload = payload.to_string_lossy().replace('\'', "'\\''");
        fs::write(
            &path,
            format!("#!/bin/sh\ncat '{payload}'\nwhile :; do :; done\n"),
        )
        .expect("blocking oversized fake sidecar script is written");
        let mut permissions = fs::metadata(&path)
            .expect("blocking oversized fake sidecar metadata")
            .permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&path, permissions)
            .expect("blocking oversized fake sidecar is executable");
        path.to_string_lossy().into_owned()
    }

    #[cfg(windows)]
    fn fake_sidecar(temp: &TempDir, name: &str, stdout: &str, exit_code: i32) -> String {
        let path = temp.path().join(format!("{name}.cmd"));
        fs::write(
            &path,
            format!("@echo off\r\nfindstr \"^\" > nul\r\necho {stdout}\r\nexit /b {exit_code}\r\n"),
        )
        .expect("fake sidecar script is written");
        path.to_string_lossy().into_owned()
    }

    #[cfg(windows)]
    fn blocking_sidecar(temp: &TempDir, name: &str, marker: &std::path::Path) -> String {
        let path = temp.path().join(format!("{name}.cmd"));
        fs::write(
            &path,
            format!(
                "@echo off\r\nfindstr \"^\" > nul\r\necho started > \"{}\"\r\n:loop\r\ngoto loop\r\n",
                marker.display()
            ),
        )
        .expect("blocking fake sidecar script is written");
        path.to_string_lossy().into_owned()
    }

    #[cfg(windows)]
    fn abortable_blocking_sidecar(
        temp: &TempDir,
        name: &str,
        started: &std::path::Path,
        heartbeat: &std::path::Path,
        stop: &std::path::Path,
    ) -> String {
        let path = temp.path().join(format!("{name}.cmd"));
        fs::write(
            &path,
            format!(
                "@echo off\r\nfindstr \"^\" > nul\r\necho started > \"{}\"\r\nset count=0\r\n:loop\r\nset /a count=%count%+1 > nul\r\necho %count% > \"{}\"\r\nif exist \"{}\" exit /b 0\r\ngoto loop\r\n",
                started.display(),
                heartbeat.display(),
                stop.display()
            ),
        )
        .expect("abortable fake sidecar script is written");
        path.to_string_lossy().into_owned()
    }

    #[cfg(windows)]
    fn oversized_sidecar(temp: &TempDir, name: &str, payload: &std::path::Path) -> String {
        let path = temp.path().join(format!("{name}.cmd"));
        fs::write(
            &path,
            format!("@echo off\r\ntype \"{}\"\r\n", payload.display()),
        )
        .expect("oversized fake sidecar script is written");
        path.to_string_lossy().into_owned()
    }

    #[cfg(windows)]
    fn blocking_oversized_sidecar(temp: &TempDir, name: &str, payload: &std::path::Path) -> String {
        let path = temp.path().join(format!("{name}.cmd"));
        fs::write(
            &path,
            format!(
                "@echo off\r\ntype \"{}\"\r\n:loop\r\ngoto loop\r\n",
                payload.display()
            ),
        )
        .expect("blocking oversized fake sidecar script is written");
        path.to_string_lossy().into_owned()
    }
}

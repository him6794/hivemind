#![allow(clippy::result_large_err, reason = "gRPC uses tonic::Status")]

//! Bounded owner-hosted artifact reads, shared by CLI/Master/Worker hosts.
//! This crate never starts a listener or discovers peers by itself. The host
//! chooses an overlay listener (or a loopback userspace-overlay forward) and a
//! trusted Nodepool channel. Remote requests cannot choose a path or authority.

use anyhow::{bail, ensure, Result};
use chrono::Utc;
use general_compute_runtime::{
    hcode_artifacts::{CHUNK_BYTES, MAX_ARTIFACTS, MAX_FILE_BYTES, MAX_TOTAL_BYTES},
    sha256_digest, ArtifactManifest, ArtifactRole,
};
use hivemind_auth::{
    artifact_probe::ArtifactProbeTokenVerifier,
    artifact_read::{ArtifactReadScope, ArtifactReadTokenVerifier},
};
use hivemind_proto::{
    hcode_artifact_peer_service_server::{
        HcodeArtifactPeerService, HcodeArtifactPeerServiceServer,
    },
    hcode_artifact_probe_service_client::HcodeArtifactProbeServiceClient,
    hcode_artifact_read_lease_service_client::HcodeArtifactReadLeaseServiceClient,
    HcodeArtifactChunkDownloadResponse, HcodePeerProbeRequest, HcodePeerReadAuthorization,
    HcodePeerReadRequest, HcodeSourceRegistration,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock},
    time::Duration,
};
use tokio::sync::{Semaphore, SemaphorePermit};
use tonic::{transport::Channel, Request, Response, Status};
use uuid::Uuid;

pub const PEER_READ_REQUEST_MAX_BYTES: usize = 8 * 1024;
pub const PEER_READ_RESPONSE_MAX_BYTES: usize = CHUNK_BYTES + 1024;
const LIVE_VALIDATION_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_CONCURRENT_READS: usize = 8;

/// Owns the exact bytes that were hashed, avoiding reopen/path/symlink races.
/// Construction verifies the whole file and every chunk before serving anything.
/// Hashing is synchronous; hosts should prepare large sources off the async
/// executor. Registration renewal shares these immutable bytes without reloading.
#[derive(Clone)]
pub struct PinnedSource {
    registration: HcodeSourceRegistration,
    source_id: Uuid,
    manifest: Arc<ArtifactManifest>,
    bytes: Arc<Vec<u8>>,
}

fn canonical_id(value: &str) -> Result<Uuid> {
    let id = Uuid::parse_str(value)?;
    ensure!(
        !id.is_nil() && id.to_string() == value,
        "invalid source identity"
    );
    Ok(id)
}

impl PinnedSource {
    pub fn new(
        registration: HcodeSourceRegistration,
        manifest: ArtifactManifest,
        bytes: Vec<u8>,
    ) -> Result<Self> {
        let source_id = canonical_id(&registration.source_id)?;
        canonical_id(&registration.registration_id)?;
        ensure!(
            !registration.task_id.is_empty()
                && registration.task_id.len() <= 255
                && registration.task_id != "."
                && !registration.task_id.contains("..")
                && registration
                    .task_id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)),
            "invalid source task identity"
        );
        ensure!(
            manifest.size_bytes >= 10
                && manifest.size_bytes <= MAX_FILE_BYTES
                && bytes.len() as u64 == manifest.size_bytes
                && manifest.role == ArtifactRole::Input
                && manifest.mime_type == "application/x-safetensors"
                && manifest.inline_bytes.is_none()
                && !manifest.chunks.is_empty()
                && manifest.chunks.len() as u64 <= MAX_FILE_BYTES / CHUNK_BYTES as u64
                && manifest
                    .chunks
                    .iter()
                    .all(|chunk| chunk.size_bytes <= CHUNK_BYTES as u64),
            "source exceeds hcode artifact limits"
        );
        manifest.validate().map_err(anyhow::Error::msg)?;
        ensure!(
            registration.artifact_id == manifest.artifact_id
                && registration.artifact_sha256 == manifest.sha256
                && u64::try_from(registration.artifact_size_bytes).ok()
                    == Some(manifest.size_bytes)
                && sha256_digest(&bytes) == manifest.sha256,
            "source content does not match registration"
        );
        for chunk in &manifest.chunks {
            let start = usize::try_from(chunk.offset)?;
            let end = usize::try_from(
                chunk
                    .offset
                    .checked_add(chunk.size_bytes)
                    .ok_or_else(|| anyhow::anyhow!("chunk range overflows"))?,
            )?;
            let content = bytes
                .get(start..end)
                .ok_or_else(|| anyhow::anyhow!("invalid chunk range"))?;
            ensure!(
                sha256_digest(content) == chunk.sha256,
                "source chunk digest mismatch"
            );
        }
        Ok(Self {
            registration,
            source_id,
            manifest: Arc::new(manifest),
            bytes: Arc::new(bytes),
        })
    }

    fn matches(&self, scope: &ArtifactReadScope) -> bool {
        self.registration.task_id == scope.task_id
            && self.manifest.artifact_id == scope.artifact_id
            && self.manifest.sha256 == scope.artifact_sha256
            && self.manifest.size_bytes == scope.artifact_size_bytes
            && self.manifest.chunks.iter().any(|chunk| {
                chunk.offset == scope.offset
                    && chunk.size_bytes == scope.chunk_size_bytes
                    && chunk.sha256 == scope.chunk_sha256
            })
    }
}

pub struct GrpcArtifactPeer {
    sources: RwLock<BTreeMap<Uuid, PinnedSource>>,
    verifier: ArtifactReadTokenVerifier,
    probe_verifier: ArtifactProbeTokenVerifier,
    nodepool: HcodeArtifactReadLeaseServiceClient<Channel>,
    probe_nodepool: HcodeArtifactProbeServiceClient<Channel>,
    concurrent: Arc<Semaphore>,
}

impl GrpcArtifactPeer {
    pub fn new(public_key: &str, nodepool: Channel, sources: Vec<PinnedSource>) -> Result<Self> {
        ensure!(
            !sources.is_empty() && sources.len() <= MAX_ARTIFACTS,
            "invalid source count"
        );
        let mut total = 0_u64;
        let mut saved = BTreeMap::new();
        for source in sources {
            total = total
                .checked_add(source.manifest.size_bytes)
                .ok_or_else(|| anyhow::anyhow!("source byte total overflows"))?;
            ensure!(total <= MAX_TOTAL_BYTES, "source byte limit exceeded");
            if saved.insert(source.source_id, source).is_some() {
                bail!("duplicate source identity");
            }
        }
        Ok(Self {
            sources: RwLock::new(saved),
            verifier: ArtifactReadTokenVerifier::from_pem(public_key)?,
            probe_verifier: ArtifactProbeTokenVerifier::from_pem(public_key)?,
            nodepool: HcodeArtifactReadLeaseServiceClient::new(nodepool.clone())
                .max_encoding_message_size(PEER_READ_REQUEST_MAX_BYTES)
                .max_decoding_message_size(32 * 1024),
            probe_nodepool: HcodeArtifactProbeServiceClient::new(nodepool)
                .max_encoding_message_size(PEER_READ_REQUEST_MAX_BYTES)
                .max_decoding_message_size(32 * 1024),
            concurrent: Arc::new(Semaphore::new(MAX_CONCURRENT_READS)),
        })
    }

    /// Host-only renewal after an owner-authenticated Nodepool registration.
    /// This is not an RPC and cannot add files or change immutable content.
    pub fn renew_source(&self, registration: HcodeSourceRegistration) -> Result<()> {
        let id = canonical_id(&registration.source_id)?;
        canonical_id(&registration.registration_id)?;
        let mut sources = self
            .sources
            .write()
            .map_err(|_| anyhow::anyhow!("source registry is unavailable"))?;
        let source = sources
            .get_mut(&id)
            .ok_or_else(|| anyhow::anyhow!("source is not hosted here"))?;
        ensure!(
            registration.task_id == source.registration.task_id
                && registration.artifact_id == source.manifest.artifact_id
                && registration.artifact_sha256 == source.manifest.sha256
                && u64::try_from(registration.artifact_size_bytes).ok()
                    == Some(source.manifest.size_bytes)
                && registration.expires_at_unix_ms > Utc::now().timestamp_millis(),
            "renewal changes source content or is expired"
        );
        source.registration = registration;
        Ok(())
    }

    /// Apply wire bounds even when the host uses its own listener/forwarder.
    pub fn into_server(self) -> HcodeArtifactPeerServiceServer<Self> {
        Arc::new(self).server()
    }

    /// Retain an Arc in the host to renew bindings while the listener is running.
    pub fn server(self: Arc<Self>) -> HcodeArtifactPeerServiceServer<Self> {
        HcodeArtifactPeerServiceServer::from_arc(self)
            .max_decoding_message_size(PEER_READ_REQUEST_MAX_BYTES)
            .max_encoding_message_size(PEER_READ_RESPONSE_MAX_BYTES)
    }

    fn permit(&self) -> Result<SemaphorePermit<'_>, Status> {
        self.concurrent
            .try_acquire()
            .map_err(|_| Status::resource_exhausted("source read limit reached"))
    }

    /// Returns a clone of immutable bytes only if the requested scope is hosted
    /// and the local registration is still unexpired. No lock is held across awaits.
    fn live_source(&self, id: Uuid, scope: &ArtifactReadScope) -> Result<PinnedSource, Status> {
        let source = self
            .sources
            .read()
            .map_err(|_| Status::internal("source registry is unavailable"))?
            .get(&id)
            .filter(|source| source.matches(scope))
            .cloned()
            .ok_or_else(|| Status::permission_denied("source scope is unavailable"))?;
        if source.registration.expires_at_unix_ms <= Utc::now().timestamp_millis() {
            return Err(Status::permission_denied("source registration has expired"));
        }
        Ok(source)
    }
}

fn canonical_source(value: &str) -> Result<Uuid, Status> {
    canonical_id(value).map_err(|_| Status::invalid_argument("invalid source ID"))
}

/// Every Nodepool grant must repeat the source, registration, identity, chunk
/// digest and expiry fences. A signed token alone or an earlier grant is not enough.
fn check_grant(
    grant: &HcodePeerReadAuthorization,
    source: &PinnedSource,
    source_id: &str,
    scope: &ArtifactReadScope,
    token_exp: u64,
) -> Result<(), Status> {
    let expected_expiry = i64::try_from(token_exp.saturating_mul(1000)).unwrap_or(i64::MAX);
    let now = Utc::now().timestamp_millis();
    if grant.source_id != source_id
        || grant.registration_id != source.registration.registration_id
        || grant.task_id != scope.task_id
        || grant.artifact_id != scope.artifact_id
        || grant.offset != scope.offset
        || grant.expires_at_unix_ms <= now
        || grant.expires_at_unix_ms > expected_expiry
        || grant.expires_at_unix_ms > source.registration.expires_at_unix_ms
        || grant.chunk.as_ref().is_none_or(|chunk| {
            chunk.artifact_sha256 != scope.artifact_sha256
                || u64::try_from(chunk.artifact_size_bytes).ok() != Some(scope.artifact_size_bytes)
                || chunk.chunk_sha256 != scope.chunk_sha256
                || u64::try_from(chunk.chunk_size_bytes).ok() != Some(scope.chunk_size_bytes)
        })
    {
        return Err(Status::permission_denied("live source scope changed"));
    }
    Ok(())
}

fn chunk_bytes(source: &PinnedSource, scope: &ArtifactReadScope) -> Result<Vec<u8>, Status> {
    let start = usize::try_from(scope.offset).map_err(|_| Status::out_of_range("invalid range"))?;
    let end = scope
        .offset
        .checked_add(scope.chunk_size_bytes)
        .and_then(|end| usize::try_from(end).ok())
        .ok_or_else(|| Status::out_of_range("invalid range"))?;
    source
        .bytes
        .get(start..end)
        .map(<[u8]>::to_vec)
        .ok_or_else(|| Status::out_of_range("invalid range"))
}

#[tonic::async_trait]
impl HcodeArtifactPeerService for GrpcArtifactPeer {
    async fn read_chunk(
        &self,
        request: Request<HcodePeerReadRequest>,
    ) -> Result<Response<HcodeArtifactChunkDownloadResponse>, Status> {
        let _permit = self.permit()?;
        let request = request.into_inner();
        let id = canonical_source(&request.source_id)?;
        let read = self
            .verifier
            .verify(&request.token, id)
            .map_err(|_| Status::unauthenticated("invalid artifact-read token"))?;
        let source = self.live_source(id, &read.scope)?;
        let mut authority = self.nodepool.clone();
        // No success cache and no offline fallback, including on timeout or a
        // dropped Nodepool connection. Only the read capability leaves this peer.
        let grant = tokio::time::timeout(
            LIVE_VALIDATION_TIMEOUT,
            authority.validate_read_lease(Request::new(request.clone())),
        )
        .await
        .map_err(|_| Status::unavailable("Nodepool live validation timed out"))??
        .into_inner();
        check_grant(&grant, &source, &request.source_id, &read.scope, read.exp)?;
        // Recheck expiry after the network await, before copying any bytes.
        self.verifier
            .verify(&request.token, id)
            .map_err(|_| Status::unauthenticated("artifact-read token expired"))?;
        Ok(Response::new(HcodeArtifactChunkDownloadResponse {
            bytes: chunk_bytes(&source, &read.scope)?,
        }))
    }

    /// Pre-dispatch sampling. Uses its own purpose and Nodepool RPC, so a probe
    /// credential cannot authorize an execution read and vice versa.
    async fn probe_chunk(
        &self,
        request: Request<HcodePeerProbeRequest>,
    ) -> Result<Response<HcodeArtifactChunkDownloadResponse>, Status> {
        let _permit = self.permit()?;
        let request = request.into_inner();
        let id = canonical_source(&request.source_id)?;
        let probe = self
            .probe_verifier
            .verify(&request.token, id)
            .map_err(|_| Status::unauthenticated("invalid artifact-probe token"))?;
        let source = self.live_source(id, &probe.scope)?;
        let mut authority = self.probe_nodepool.clone();
        let grant = tokio::time::timeout(
            LIVE_VALIDATION_TIMEOUT,
            authority.validate_probe_lease(Request::new(request.clone())),
        )
        .await
        .map_err(|_| Status::unavailable("Nodepool live validation timed out"))??
        .into_inner();
        check_grant(&grant, &source, &request.source_id, &probe.scope, probe.exp)?;
        self.probe_verifier
            .verify(&request.token, id)
            .map_err(|_| Status::unauthenticated("artifact-probe token expired"))?;
        Ok(Response::new(HcodeArtifactChunkDownloadResponse {
            bytes: chunk_bytes(&source, &probe.scope)?,
        }))
    }
}

#[cfg(test)]
mod tests;

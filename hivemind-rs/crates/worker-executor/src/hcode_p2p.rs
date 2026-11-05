//! Direct, verified chunk download from probed overlay sources.
//!
//! Nodepool discovers sources and authorizes each read lease; the source peer
//! returns the bytes. The Worker checks every chunk and then the complete
//! artifact. The control-plane trait cannot return chunk content, so this path
//! has no Nodepool byte fallback: a failed source is replaced only by another
//! authorized source, and a chunk without a verified source fails the attempt.

use std::net::Ipv4Addr;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use general_compute_runtime::hcode_artifacts::{CHUNK_BYTES, MAX_FILE_BYTES};
use general_compute_runtime::{sha256_digest, ArtifactChunk, ArtifactManifest};
use hivemind_proto::{
    hcode_artifact_peer_service_client::HcodeArtifactPeerServiceClient,
    hcode_artifact_read_lease_service_client::HcodeArtifactReadLeaseServiceClient,
    HcodeArtifactChunkDownload, HcodeArtifactReadLease, HcodePeerReadRequest,
    HcodeSourceAvailability, HcodeSourceRegistration, IssueHcodeReadLeaseRequest,
};
use tonic::transport::{Channel, Endpoint};

use crate::nodepool_client::nodepool_endpoint;

/// Fixed overlay-only artifact service port, as registered by Nodepool.
pub const HCODE_SOURCE_PORT: u32 = 50054;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const CONTROL_TIMEOUT: Duration = Duration::from_secs(10);
const READ_TIMEOUT: Duration = Duration::from_secs(30);
/// Nodepool returns at most eight sources for one artifact. Never try more.
const MAX_SOURCES_PER_CHUNK: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum P2pDownloadError {
    /// The manifest does not describe a contiguous, bounded artifact.
    InvalidManifest(&'static str),
    /// Nodepool reported no source whose registration and observation are live.
    NoLiveSource { offset: u64 },
    /// Every authorized candidate was tried; none returned the expected bytes.
    SourcesExhausted { offset: u64, attempted: usize },
    /// A Nodepool control call failed. No byte request was made.
    ControlPlane(String),
    /// Every chunk matched, but the assembled artifact did not.
    ArtifactDigestMismatch,
    /// The whole-artifact hashing task did not complete.
    HashingFailed,
}

impl std::fmt::Display for P2pDownloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidManifest(reason) => write!(f, "invalid artifact manifest: {reason}"),
            Self::NoLiveSource { offset } => {
                write!(f, "no live source for the chunk at offset {offset}")
            }
            Self::SourcesExhausted { offset, attempted } => write!(
                f,
                "{attempted} authorized source(s) failed for the chunk at offset {offset}"
            ),
            Self::ControlPlane(reason) => write!(f, "source control call failed: {reason}"),
            Self::ArtifactDigestMismatch => {
                f.write_str("assembled artifact does not match its manifest digest")
            }
            Self::HashingFailed => f.write_str("artifact digest verification did not complete"),
        }
    }
}

impl std::error::Error for P2pDownloadError {}

/// Nodepool's discovery and lease authority. It returns metadata and
/// capabilities only; chunk bytes never come back through this trait.
#[tonic::async_trait]
pub trait P2pControlPlane: Send + Sync {
    async fn list_sources(
        &self,
        request: HcodeArtifactChunkDownload,
    ) -> Result<Vec<HcodeSourceAvailability>, String>;

    async fn issue_lease(
        &self,
        request: IssueHcodeReadLeaseRequest,
    ) -> Result<HcodeArtifactReadLease, String>;
}

/// The data plane of one source. The implementation must send only the
/// purpose-separated read token from the lease, never the execution token.
#[tonic::async_trait]
pub trait P2pPeerReader: Send + Sync {
    async fn read_chunk(
        &self,
        overlay_ipv4: &str,
        port: u32,
        source_id: &str,
        read_token: &str,
    ) -> Result<Vec<u8>, String>;
}

/// Production control plane. Every call goes to Nodepool; no successful
/// discovery or lease is cached.
pub struct NodepoolP2pControl {
    endpoint: Arc<Mutex<String>>,
    require_external_overlay: bool,
}

impl NodepoolP2pControl {
    #[must_use]
    pub fn new_shared(endpoint: Arc<Mutex<String>>, require_external_overlay: bool) -> Self {
        Self {
            endpoint,
            require_external_overlay,
        }
    }

    async fn client(&self) -> Result<HcodeArtifactReadLeaseServiceClient<Channel>, String> {
        let configured = self
            .endpoint
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let endpoint = if self.require_external_overlay {
            hivemind_client_runtime::external_overlay_endpoint(
                hivemind_client_runtime::ClientRole::Worker,
                &configured,
            )
            .await
            .map_err(|error| error.to_string())?
        } else {
            configured
        };
        if endpoint.trim().is_empty() {
            return Err("Nodepool endpoint is not ready".into());
        }
        let channel = Endpoint::from_shared(nodepool_endpoint(&endpoint))
            .map_err(|error| error.to_string())?
            .connect_timeout(CONNECT_TIMEOUT)
            .connect()
            .await
            .map_err(|error| error.to_string())?;
        Ok(HcodeArtifactReadLeaseServiceClient::new(channel))
    }
}

#[tonic::async_trait]
impl P2pControlPlane for NodepoolP2pControl {
    async fn list_sources(
        &self,
        request: HcodeArtifactChunkDownload,
    ) -> Result<Vec<HcodeSourceAvailability>, String> {
        let response = tokio::time::timeout(CONTROL_TIMEOUT, async {
            let mut client = self.client().await?;
            client
                .list_read_sources(request)
                .await
                .map_err(|status| format!("{:?}", status.code()))
        })
        .await
        .map_err(|_| "source discovery timed out".to_owned())??;
        Ok(response.into_inner().sources)
    }

    async fn issue_lease(
        &self,
        request: IssueHcodeReadLeaseRequest,
    ) -> Result<HcodeArtifactReadLease, String> {
        let response = tokio::time::timeout(CONTROL_TIMEOUT, async {
            let mut client = self.client().await?;
            client
                .issue_read_lease(request)
                .await
                .map_err(|status| format!("{:?}", status.code()))
        })
        .await
        .map_err(|_| "read lease request timed out".to_owned())??;
        Ok(response.into_inner())
    }
}

/// Turns a validated overlay `ip:port` target into the address this process
/// dials. On Windows the embedded userspace VPN has no OS route to the overlay,
/// so its outbound TCP goes through a local bridge. Elsewhere the target is used.
#[tonic::async_trait]
pub trait OverlayDial: Send + Sync {
    async fn dial_address(&self, overlay_target: &str) -> Result<String, String>;
}

/// Worker dialer. It asks the process's VPN session for a bridge to the target.
pub struct UserspaceOverlayDial;

#[tonic::async_trait]
impl OverlayDial for UserspaceOverlayDial {
    async fn dial_address(&self, overlay_target: &str) -> Result<String, String> {
        hivemind_client_runtime::userspace_tcp_bridge(
            hivemind_client_runtime::ClientRole::Worker,
            overlay_target,
        )
        .await
        .map_err(|error| error.to_string())
    }
}

/// Production overlay reader. The address comes only from a Nodepool lease and
/// must be a canonical overlay IPv4 on the fixed artifact port. Only a validated
/// target reaches the dialer.
pub struct OverlayPeerReader {
    dial: Arc<dyn OverlayDial>,
}

impl OverlayPeerReader {
    #[must_use]
    pub fn new(dial: Arc<dyn OverlayDial>) -> Self {
        Self { dial }
    }

    /// Reader for a Worker process, using the process's VPN session to dial.
    #[must_use]
    pub fn for_worker() -> Self {
        Self::new(Arc::new(UserspaceOverlayDial))
    }
}

#[tonic::async_trait]
impl P2pPeerReader for OverlayPeerReader {
    async fn read_chunk(
        &self,
        overlay_ipv4: &str,
        port: u32,
        source_id: &str,
        read_token: &str,
    ) -> Result<Vec<u8>, String> {
        let target = overlay_target(overlay_ipv4, port)?;
        let address = self.dial.dial_address(&target).await?;
        let channel = Endpoint::from_shared(format!("http://{address}"))
            .map_err(|error| error.to_string())?
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(READ_TIMEOUT)
            .connect()
            .await
            .map_err(|error| error.to_string())?;
        let mut client = HcodeArtifactPeerServiceClient::new(channel)
            .max_encoding_message_size(8192)
            .max_decoding_message_size(CHUNK_BYTES + 1024);
        let response = client
            .read_chunk(HcodePeerReadRequest {
                source_id: source_id.to_owned(),
                token: read_token.to_owned(),
            })
            .await
            .map_err(|status| format!("{:?}", status.code()))?;
        Ok(response.into_inner().bytes)
    }
}

/// Validates a Nodepool-issued overlay address and returns its `ip:port` target.
fn overlay_target(overlay_ipv4: &str, port: u32) -> Result<String, String> {
    let ip = Ipv4Addr::from_str(overlay_ipv4).map_err(|_| "source address is not IPv4")?;
    let overlay_network = u32::from(Ipv4Addr::new(100, 64, 0, 0));
    if u32::from(ip) & 0xffc0_0000 != overlay_network || ip.to_string() != overlay_ipv4 {
        return Err("source address is outside the overlay".into());
    }
    if port != HCODE_SOURCE_PORT {
        return Err("source port is not the overlay artifact port".into());
    }
    Ok(format!("{ip}:{HCODE_SOURCE_PORT}"))
}

/// Downloads one artifact from authorized peers. Each chunk is verified before
/// it is appended, and the complete artifact is verified before it is returned.
pub async fn download_artifact(
    control: &dyn P2pControlPlane,
    peers: &dyn P2pPeerReader,
    token: &str,
    task_id: &str,
    artifact: &ArtifactManifest,
) -> Result<Vec<u8>, P2pDownloadError> {
    validate_layout(artifact)?;
    let mut bytes = Vec::with_capacity(artifact.size_bytes as usize);
    for chunk in &artifact.chunks {
        let payload = download_chunk(control, peers, token, task_id, artifact, chunk).await?;
        bytes.extend_from_slice(&payload);
    }
    let (bytes, digest) = tokio::task::spawn_blocking(move || {
        let digest = sha256_digest(&bytes);
        (bytes, digest)
    })
    .await
    .map_err(|_| P2pDownloadError::HashingFailed)?;
    if bytes.len() as u64 != artifact.size_bytes || digest != artifact.sha256 {
        return Err(P2pDownloadError::ArtifactDigestMismatch);
    }
    Ok(bytes)
}

async fn download_chunk(
    control: &dyn P2pControlPlane,
    peers: &dyn P2pPeerReader,
    token: &str,
    task_id: &str,
    artifact: &ArtifactManifest,
    chunk: &ArtifactChunk,
) -> Result<Vec<u8>, P2pDownloadError> {
    let offset = chunk.offset;
    let coordinate = HcodeArtifactChunkDownload {
        token: token.to_owned(),
        task_id: task_id.to_owned(),
        artifact_id: artifact.artifact_id.clone(),
        offset: i64::try_from(offset)
            .map_err(|_| P2pDownloadError::InvalidManifest("chunk offset"))?,
    };
    let now = Utc::now().timestamp_millis();
    let sources = control
        .list_sources(coordinate.clone())
        .await
        .map_err(P2pDownloadError::ControlPlane)?;
    let candidates: Vec<HcodeSourceAvailability> = sources
        .into_iter()
        .filter(|source| is_live(source, task_id, artifact, now))
        .take(MAX_SOURCES_PER_CHUNK)
        .collect();
    if candidates.is_empty() {
        return Err(P2pDownloadError::NoLiveSource { offset });
    }
    let mut attempted = 0;
    for candidate in &candidates {
        let Some(registration) = candidate.source.as_ref() else {
            continue;
        };
        attempted += 1;
        let request = IssueHcodeReadLeaseRequest {
            chunk: Some(coordinate.clone()),
            source_id: registration.source_id.clone(),
        };
        let Ok(lease) = control.issue_lease(request).await else {
            continue;
        };
        if !lease_binds(&lease, registration, task_id, artifact, chunk, now) {
            continue;
        }
        let Ok(payload) = peers
            .read_chunk(
                &lease.overlay_ipv4,
                lease.port,
                &registration.source_id,
                &lease.token,
            )
            .await
        else {
            continue;
        };
        if payload.len() as u64 == chunk.size_bytes && sha256_digest(&payload) == chunk.sha256 {
            return Ok(payload);
        }
    }
    Err(P2pDownloadError::SourcesExhausted { offset, attempted })
}

fn is_live(
    availability: &HcodeSourceAvailability,
    task_id: &str,
    artifact: &ArtifactManifest,
    now: i64,
) -> bool {
    let Some(registration) = availability.source.as_ref() else {
        return false;
    };
    registration.task_id == task_id
        && registration.artifact_id == artifact.artifact_id
        && registration.artifact_sha256 == artifact.sha256
        && same_u64(artifact.size_bytes, registration.artifact_size_bytes)
        && registration.port == HCODE_SOURCE_PORT
        && registration.expires_at_unix_ms > now
        && availability.available_until_unix_ms > now
}

/// Every field of the lease must repeat the registration, the task, the
/// artifact, the chunk coordinate and its digests. A signed token alone is not
/// enough, so a mismatched lease never reaches the peer.
fn lease_binds(
    lease: &HcodeArtifactReadLease,
    registration: &HcodeSourceRegistration,
    task_id: &str,
    artifact: &ArtifactManifest,
    chunk: &ArtifactChunk,
    now: i64,
) -> bool {
    let Some(authorization) = lease.authorization.as_ref() else {
        return false;
    };
    let Some(scope) = authorization.chunk.as_ref() else {
        return false;
    };
    lease.port == HCODE_SOURCE_PORT
        && lease.overlay_ipv4 == registration.overlay_ipv4
        && !lease.token.is_empty()
        && authorization.source_id == registration.source_id
        && authorization.registration_id == registration.registration_id
        && authorization.task_id == task_id
        && authorization.artifact_id == artifact.artifact_id
        && authorization.offset == chunk.offset
        && authorization.expires_at_unix_ms > now
        && scope.artifact_sha256 == artifact.sha256
        && same_u64(artifact.size_bytes, scope.artifact_size_bytes)
        && scope.chunk_sha256 == chunk.sha256
        && same_u64(chunk.size_bytes, scope.chunk_size_bytes)
}

fn validate_layout(artifact: &ArtifactManifest) -> Result<(), P2pDownloadError> {
    if artifact.size_bytes == 0
        || artifact.size_bytes > MAX_FILE_BYTES
        || artifact.chunks.is_empty()
    {
        return Err(P2pDownloadError::InvalidManifest(
            "artifact size or chunk count",
        ));
    }
    let mut expected_offset = 0_u64;
    for chunk in &artifact.chunks {
        if chunk.offset != expected_offset
            || chunk.size_bytes == 0
            || chunk.size_bytes > CHUNK_BYTES as u64
        {
            return Err(P2pDownloadError::InvalidManifest("chunk layout"));
        }
        expected_offset = expected_offset
            .checked_add(chunk.size_bytes)
            .ok_or(P2pDownloadError::InvalidManifest("chunk layout"))?;
    }
    if expected_offset != artifact.size_bytes {
        return Err(P2pDownloadError::InvalidManifest(
            "chunks do not cover the artifact",
        ));
    }
    Ok(())
}

fn same_u64(value: u64, expected: i64) -> bool {
    i64::try_from(value).is_ok_and(|value| value == expected)
}

#[cfg(test)]
mod tests;

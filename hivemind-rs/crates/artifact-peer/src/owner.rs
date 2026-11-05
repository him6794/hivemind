//! Owner-side source hosting. A verified local artifact is registered with
//! Nodepool and served over the overlay by [`GrpcArtifactPeer`]. Every renewal
//! registers a new generation, applies it to the peer, and then publishes a new
//! probe observation. The host process owns the listener and any Windows
//! forwarding. It calls [`HostedArtifact::probe`] only after the listener serves.

use std::{future::Future, io::Read, net::Ipv4Addr, path::Path, time::Duration};

use anyhow::{ensure, Context, Result};
use chrono::Utc;
use general_compute_runtime::{
    hcode_artifacts::{CHUNK_BYTES, MAX_FILE_BYTES},
    sha256_digest, ArtifactChunk, ArtifactManifest, ArtifactRole,
};
use hivemind_proto::{
    hcode_artifact_probe_service_client::HcodeArtifactProbeServiceClient,
    hcode_artifact_source_service_client::HcodeArtifactSourceServiceClient,
    HcodeOwnerVpnNodeRequest, HcodeSourceAvailability, HcodeSourceRegistration, HcodeSourceRequest,
    RegisterHcodeSourceRequest,
};
use tonic::{transport::Channel, Response, Status};

use crate::{canonical_id, verify_local_content, GrpcArtifactPeer, PinnedSource};

const MIME_TYPE: &str = "application/x-safetensors";
const MAX_TOKEN_BYTES: usize = 8 * 1024;
const CONTROL_TIMEOUT: Duration = Duration::from_secs(10);
/// Registrations expire within 60 seconds, and a probe observation lasts 30
/// seconds. Renewals must land well inside that window.
pub const MIN_RENEWAL_INTERVAL: Duration = Duration::from_secs(1);
pub const MAX_RENEWAL_INTERVAL: Duration = Duration::from_secs(20);

/// The owner's VPN identity as Nodepool's registration request carries it.
/// Nodepool still checks the node against the persisted enrollment, so these
/// values are hints rather than proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerPeerIdentity {
    pub vpn_client_id: String,
    pub headscale_node_id: u64,
}

impl OwnerPeerIdentity {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.vpn_client_id.is_empty() && self.vpn_client_id.len() <= 255,
            "VPN client identity must be 1 to 255 bytes"
        );
        ensure!(
            self.headscale_node_id > 0 && self.headscale_node_id <= i64::MAX as u64,
            "Headscale node identity is out of range"
        );
        Ok(())
    }
}

/// Reads a regular file once, within the hcode size limit. Symbolic links and
/// other special files are refused, so the host serves the bytes it hashed.
/// Call this off the async executor, because the file can be large.
pub fn read_artifact_file(path: &Path) -> Result<Vec<u8>> {
    let metadata = std::fs::symlink_metadata(path)
        .with_context(|| format!("cannot inspect {}", path.display()))?;
    ensure!(
        metadata.file_type().is_file(),
        "artifact must be a regular file"
    );
    ensure!(metadata.len() <= MAX_FILE_BYTES, "artifact exceeds 64 MiB");
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    std::fs::File::open(path)
        .with_context(|| format!("cannot open {}", path.display()))?
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("cannot read {}", path.display()))?;
    ensure!(
        bytes.len() as u64 <= MAX_FILE_BYTES,
        "artifact grew past 64 MiB while it was read"
    );
    Ok(bytes)
}

/// Builds the manifest for `bytes` with the fixed hcode chunk layout. Nodepool
/// checks the whole-file digest against the registration, and each probe checks
/// one chunk against the manifest it stored at submission. The result is always
/// accepted by [`verify_local_content`], so it can be pinned without rework.
pub fn manifest_for_bytes(artifact_id: &str, bytes: &[u8]) -> Result<ArtifactManifest> {
    ensure!(
        bytes.len() as u64 <= MAX_FILE_BYTES,
        "artifact exceeds 64 MiB"
    );
    let chunks = bytes
        .chunks(CHUNK_BYTES)
        .enumerate()
        .map(|(index, part)| ArtifactChunk {
            offset: (index * CHUNK_BYTES) as u64,
            size_bytes: part.len() as u64,
            sha256: sha256_digest(part),
        })
        .collect();
    let manifest = ArtifactManifest {
        artifact_id: artifact_id.to_owned(),
        role: ArtifactRole::Input,
        size_bytes: bytes.len() as u64,
        mime_type: MIME_TYPE.to_owned(),
        sha256: sha256_digest(bytes),
        chunks,
        inline_bytes: None,
    };
    verify_local_content(&manifest, bytes)?;
    Ok(manifest)
}

/// Owner calls to Nodepool's source registry and probe service. The account
/// token goes only to Nodepool, and peers never receive it.
#[derive(Clone)]
pub struct OwnerSourceClient {
    sources: HcodeArtifactSourceServiceClient<Channel>,
    probes: HcodeArtifactProbeServiceClient<Channel>,
}

impl OwnerSourceClient {
    #[must_use]
    pub fn new(nodepool: Channel) -> Self {
        Self {
            sources: HcodeArtifactSourceServiceClient::new(nodepool.clone()),
            probes: HcodeArtifactProbeServiceClient::new(nodepool),
        }
    }

    /// Registers the owner's source for one artifact, or renews it. A renewal
    /// keeps the source id and issues a new registration generation.
    pub async fn register(
        &self,
        token: &str,
        task_id: &str,
        artifact_id: &str,
        identity: &OwnerPeerIdentity,
    ) -> Result<HcodeSourceRegistration> {
        validate_token(token)?;
        identity.validate()?;
        let request = RegisterHcodeSourceRequest {
            token: token.to_owned(),
            task_id: task_id.to_owned(),
            artifact_id: artifact_id.to_owned(),
            vpn_client_id: identity.vpn_client_id.clone(),
            headscale_node_id: identity.headscale_node_id,
        };
        let registration = call(self.sources.clone().register_source(request)).await?;
        check_registration(&registration, task_id, artifact_id)?;
        Ok(registration)
    }

    /// Asks Nodepool which enrolled Headscale node belongs to this VPN client label.
    /// The reply is an identity hint. Registration asks Nodepool to verify it again,
    /// so nothing here is trusted beyond the shape checks below.
    pub async fn resolve_identity(
        &self,
        token: &str,
        vpn_client_id: &str,
    ) -> Result<OwnerPeerIdentity> {
        validate_token(token)?;
        ensure!(
            !vpn_client_id.is_empty() && vpn_client_id.len() <= 255,
            "VPN client identity must be 1 to 255 bytes"
        );
        let request = HcodeOwnerVpnNodeRequest {
            token: token.to_owned(),
            vpn_client_id: vpn_client_id.to_owned(),
        };
        let node = call(self.sources.clone().resolve_owner_vpn_node(request)).await?;
        ensure!(
            node.vpn_client_id == vpn_client_id,
            "Nodepool resolved a different VPN client"
        );
        let overlay: Ipv4Addr = node
            .overlay_ipv4
            .parse()
            .context("Nodepool returned a malformed overlay address")?;
        let octets = overlay.octets();
        ensure!(
            octets[0] == 100 && (64..=127).contains(&octets[1]),
            "Nodepool returned an address outside the overlay"
        );
        let identity = OwnerPeerIdentity {
            vpn_client_id: node.vpn_client_id,
            headscale_node_id: node.headscale_node_id,
        };
        identity.validate()?;
        Ok(identity)
    }

    /// Asks Nodepool to probe the current generation of `source_id`. Nodepool
    /// dials the peer, so the peer must already be serving.
    pub async fn probe(&self, token: &str, source_id: &str) -> Result<HcodeSourceAvailability> {
        validate_token(token)?;
        let request = HcodeSourceRequest {
            token: token.to_owned(),
            source_id: source_id.to_owned(),
        };
        let availability = call(self.probes.clone().probe_source(request)).await?;
        check_availability(&availability, source_id)?;
        Ok(availability)
    }

    /// Stops publication. Returns false when Nodepool had nothing live to revoke.
    pub async fn revoke(&self, token: &str, source_id: &str) -> Result<bool> {
        validate_token(token)?;
        let request = HcodeSourceRequest {
            token: token.to_owned(),
            source_id: source_id.to_owned(),
        };
        let response = call(self.sources.clone().revoke_source(request)).await?;
        Ok(response.revoked)
    }
}

/// One artifact this host serves. The bytes live in the peer; this value keeps
/// only identifiers and the current registration generation.
pub struct HostedArtifact {
    client: OwnerSourceClient,
    token: String,
    identity: OwnerPeerIdentity,
    task_id: String,
    artifact_id: String,
    source_id: String,
    registration_id: String,
}

impl HostedArtifact {
    /// Verifies the local copy, registers it with Nodepool, and returns the
    /// pinned source for the peer. Verification runs first, so a corrupt copy
    /// never creates a registration. A registration that cannot be pinned is
    /// revoked on a best-effort basis.
    pub async fn publish(
        client: OwnerSourceClient,
        token: &str,
        identity: OwnerPeerIdentity,
        task_id: &str,
        manifest: ArtifactManifest,
        bytes: Vec<u8>,
    ) -> Result<(Self, PinnedSource)> {
        verify_local_content(&manifest, &bytes)?;
        let registration = client
            .register(token, task_id, &manifest.artifact_id, &identity)
            .await?;
        let pinned = match PinnedSource::new(registration.clone(), manifest.clone(), bytes) {
            Ok(pinned) => pinned,
            Err(error) => {
                // The registration also expires within a minute, so a failed revoke is harmless.
                let _ = client.revoke(token, &registration.source_id).await;
                return Err(error);
            }
        };
        let hosted = Self {
            client,
            token: token.to_owned(),
            identity,
            task_id: task_id.to_owned(),
            artifact_id: manifest.artifact_id,
            source_id: registration.source_id,
            registration_id: registration.registration_id,
        };
        Ok((hosted, pinned))
    }

    #[must_use]
    pub fn source_id(&self) -> &str {
        &self.source_id
    }

    #[must_use]
    pub fn registration_id(&self) -> &str {
        &self.registration_id
    }

    /// Probes the current generation. Call only after the listener serves the peer.
    pub async fn probe(&self) -> Result<HcodeSourceAvailability> {
        self.client.probe(&self.token, &self.source_id).await
    }

    /// Registers a new generation, applies it to the peer, and then probes it.
    /// The order matters. Nodepool must hold the new generation before it probes,
    /// and the peer must accept it before the probe asks the peer for bytes.
    pub async fn renew(&mut self, peer: &GrpcArtifactPeer) -> Result<HcodeSourceAvailability> {
        let registration = self
            .client
            .register(
                &self.token,
                &self.task_id,
                &self.artifact_id,
                &self.identity,
            )
            .await?;
        ensure!(
            registration.source_id == self.source_id,
            "renewal changed the source identity"
        );
        peer.renew_source(registration.clone())?;
        self.registration_id = registration.registration_id;
        self.probe().await
    }

    /// Stops publication. Nodepool refuses later probes and reads for this source.
    pub async fn revoke(self) -> Result<bool> {
        self.client.revoke(&self.token, &self.source_id).await
    }
}

/// Renews every hosted artifact once per `every`, starting immediately. The first
/// failure is returned rather than retried, so the host can stop serving instead
/// of advertising a source it cannot refresh.
pub async fn keep_published(
    peer: &GrpcArtifactPeer,
    hosted: &mut [HostedArtifact],
    every: Duration,
) -> Result<()> {
    ensure!(
        (MIN_RENEWAL_INTERVAL..=MAX_RENEWAL_INTERVAL).contains(&every),
        "renewal interval must be between 1 and 20 seconds"
    );
    loop {
        for artifact in hosted.iter_mut() {
            artifact.renew(peer).await?;
        }
        tokio::time::sleep(every).await;
    }
}

async fn call<T>(future: impl Future<Output = Result<Response<T>, Status>>) -> Result<T> {
    let response = tokio::time::timeout(CONTROL_TIMEOUT, future)
        .await
        .context("Nodepool control call timed out")?
        .map_err(|status| anyhow::anyhow!("Nodepool rejected the request: {:?}", status.code()))?;
    Ok(response.into_inner())
}

fn validate_token(token: &str) -> Result<()> {
    ensure!(
        !token.is_empty()
            && token.len() <= MAX_TOKEN_BYTES
            && token.bytes().all(|byte| byte.is_ascii_graphic()),
        "account token is malformed"
    );
    Ok(())
}

fn check_registration(
    registration: &HcodeSourceRegistration,
    task_id: &str,
    artifact_id: &str,
) -> Result<()> {
    canonical_id(&registration.source_id)?;
    canonical_id(&registration.registration_id)?;
    ensure!(
        registration.task_id == task_id
            && registration.artifact_id == artifact_id
            && registration.state == "registered_pending"
            && registration.expires_at_unix_ms > Utc::now().timestamp_millis(),
        "Nodepool returned a registration for another artifact or an expired generation"
    );
    Ok(())
}

fn check_availability(availability: &HcodeSourceAvailability, source_id: &str) -> Result<()> {
    let registration = availability
        .source
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("probe response has no source"))?;
    canonical_id(&registration.registration_id)?;
    ensure!(
        registration.source_id == source_id
            && availability.available_until_unix_ms > Utc::now().timestamp_millis(),
        "probe response is for another source or has already expired"
    );
    Ok(())
}

#[cfg(test)]
mod tests;

use super::*;

use hivemind_auth::{artifact_probe::ArtifactProbeTokenSigner, artifact_read::ArtifactReadScope};
use hivemind_proto::{
    hcode_artifact_peer_service_client::HcodeArtifactPeerServiceClient,
    hcode_artifact_probe_service_server::{
        HcodeArtifactProbeService, HcodeArtifactProbeServiceServer,
    },
    hcode_artifact_source_service_server::{
        HcodeArtifactSourceService, HcodeArtifactSourceServiceServer,
    },
    HcodeArtifactChunkAuthorization, HcodeOwnerVpnNode, HcodeOwnerVpnNodeRequest,
    HcodePeerProbeRequest, HcodePeerReadAuthorization, RevokeHcodeSourceResponse,
};
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{transport::Endpoint, Request};
use uuid::Uuid;

const OWNER_TOKEN: &str = "owner-account-token";
const TASK: &str = "task-1";
const ARTIFACT: &str = "model";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Counters {
    registers: usize,
    probes: usize,
    validations: usize,
}

struct NodepoolState {
    registration_id: String,
    expires_at_unix_ms: i64,
    revoked: bool,
    peer_url: String,
    counters: Counters,
    last_probe_at: Option<tokio::time::Instant>,
}

/// A loopback Nodepool stand-in. Probes are real: it signs a probe token, dials
/// the host's peer over gRPC, and checks the chunk digest it gets back.
struct Nodepool {
    source_id: String,
    manifest: ArtifactManifest,
    private: String,
    state: Mutex<NodepoolState>,
}

impl Nodepool {
    fn new(manifest: ArtifactManifest, private: String) -> Self {
        Self {
            source_id: Uuid::new_v4().to_string(),
            manifest,
            private,
            state: Mutex::new(NodepoolState {
                registration_id: Uuid::new_v4().to_string(),
                expires_at_unix_ms: 0,
                revoked: false,
                peer_url: String::new(),
                counters: Counters::default(),
                last_probe_at: None,
            }),
        }
    }

    fn counters(&self) -> Counters {
        self.state.lock().unwrap().counters
    }

    fn set_peer_url(&self, url: String) {
        self.state.lock().unwrap().peer_url = url;
    }

    fn registration(&self, state: &NodepoolState) -> HcodeSourceRegistration {
        HcodeSourceRegistration {
            source_id: self.source_id.clone(),
            registration_id: state.registration_id.clone(),
            task_id: TASK.into(),
            artifact_id: ARTIFACT.into(),
            artifact_sha256: self.manifest.sha256.clone(),
            artifact_size_bytes: self.manifest.size_bytes as i64,
            overlay_ipv4: "100.64.1.2".into(),
            port: 50054,
            expires_at_unix_ms: state.expires_at_unix_ms,
            state: "registered_pending".into(),
        }
    }

    fn first_chunk(&self) -> &ArtifactChunk {
        &self.manifest.chunks[0]
    }
}

#[tonic::async_trait]
impl HcodeArtifactSourceService for Nodepool {
    async fn register_source(
        &self,
        request: Request<RegisterHcodeSourceRequest>,
    ) -> Result<Response<HcodeSourceRegistration>, Status> {
        let request = request.into_inner();
        if request.token != OWNER_TOKEN {
            return Err(Status::unauthenticated("owner token rejected"));
        }
        if request.task_id != TASK || request.artifact_id != ARTIFACT {
            return Err(Status::not_found("artifact is not registered"));
        }
        let mut state = self.state.lock().unwrap();
        if state.revoked {
            return Err(Status::failed_precondition("source is revoked"));
        }
        state.counters.registers += 1;
        state.registration_id = Uuid::new_v4().to_string();
        state.expires_at_unix_ms = Utc::now().timestamp_millis() + 60_000;
        Ok(Response::new(self.registration(&state)))
    }

    async fn get_source(
        &self,
        _request: Request<HcodeSourceRequest>,
    ) -> Result<Response<HcodeSourceRegistration>, Status> {
        Err(Status::unimplemented("not used by the host"))
    }

    async fn resolve_owner_vpn_node(
        &self,
        request: Request<HcodeOwnerVpnNodeRequest>,
    ) -> Result<Response<HcodeOwnerVpnNode>, Status> {
        let request = request.into_inner();
        if request.token != OWNER_TOKEN {
            return Err(Status::unauthenticated("owner token rejected"));
        }
        let label = request.vpn_client_id;
        let node = |client: String, overlay: &str| {
            Ok(Response::new(HcodeOwnerVpnNode {
                vpn_client_id: client,
                headscale_node_id: 7,
                overlay_ipv4: overlay.into(),
            }))
        };
        match label.as_str() {
            "not-joined" => Err(Status::failed_precondition(
                "enrolled VPN node has not joined yet",
            )),
            "other-label" => node("someone-else".into(), "100.64.1.2"),
            "outside-overlay" => node(label.clone(), "8.8.8.8"),
            _ => node(label.clone(), "100.64.1.2"),
        }
    }

    async fn revoke_source(
        &self,
        request: Request<HcodeSourceRequest>,
    ) -> Result<Response<RevokeHcodeSourceResponse>, Status> {
        let request = request.into_inner();
        if request.token != OWNER_TOKEN {
            return Err(Status::unauthenticated("owner token rejected"));
        }
        let mut state = self.state.lock().unwrap();
        let revoked = !state.revoked;
        state.revoked = true;
        Ok(Response::new(RevokeHcodeSourceResponse { revoked }))
    }
}

#[tonic::async_trait]
impl HcodeArtifactProbeService for Nodepool {
    async fn probe_source(
        &self,
        request: Request<HcodeSourceRequest>,
    ) -> Result<Response<HcodeSourceAvailability>, Status> {
        let request = request.into_inner();
        if request.token != OWNER_TOKEN {
            return Err(Status::unauthenticated("owner token rejected"));
        }
        if request.source_id != self.source_id {
            return Err(Status::not_found("unknown source"));
        }
        let (registration, peer_url) = {
            let mut state = self.state.lock().unwrap();
            if state.revoked {
                return Err(Status::failed_precondition("source is revoked"));
            }
            if state
                .last_probe_at
                .is_some_and(|at| at.elapsed() < Duration::from_secs(3))
            {
                return Err(Status::resource_exhausted("source probe cooldown"));
            }
            state.last_probe_at = Some(tokio::time::Instant::now());
            state.counters.probes += 1;
            (self.registration(&state), state.peer_url.clone())
        };
        let now = Utc::now().timestamp();
        let chunk = self.first_chunk();
        let scope = ArtifactReadScope {
            task_id: TASK.into(),
            artifact_id: ARTIFACT.into(),
            artifact_sha256: self.manifest.sha256.clone(),
            artifact_size_bytes: self.manifest.size_bytes,
            offset: chunk.offset,
            chunk_sha256: chunk.sha256.clone(),
            chunk_size_bytes: chunk.size_bytes,
        };
        let token = ArtifactProbeTokenSigner::from_pem(&self.private)
            .map_err(|error| Status::internal(error.to_string()))?
            .sign(
                Uuid::parse_str(&self.source_id)
                    .map_err(|error| Status::internal(error.to_string()))?,
                Uuid::new_v4(),
                scope,
                (now + 15) as u64,
            )
            .map_err(|error| Status::internal(error.to_string()))?;
        let channel = Endpoint::from_shared(peer_url)
            .map_err(|error| Status::internal(error.to_string()))?
            .connect()
            .await
            .map_err(|error| Status::unavailable(error.to_string()))?;
        let response = HcodeArtifactPeerServiceClient::new(channel)
            .probe_chunk(HcodePeerProbeRequest {
                source_id: self.source_id.clone(),
                token,
            })
            .await?
            .into_inner();
        if response.bytes.len() as u64 != chunk.size_bytes
            || sha256_digest(&response.bytes) != chunk.sha256
        {
            return Err(Status::failed_precondition(
                "sampled chunk did not match the manifest",
            ));
        }
        Ok(Response::new(HcodeSourceAvailability {
            source: Some(registration),
            checked_at_unix_ms: now * 1000,
            available_until_unix_ms: (now + 30) * 1000,
            sampled_offset: chunk.offset,
        }))
    }

    async fn validate_probe_lease(
        &self,
        request: Request<HcodePeerProbeRequest>,
    ) -> Result<Response<HcodePeerReadAuthorization>, Status> {
        let request = request.into_inner();
        let (registration_id, expires_at_unix_ms) = {
            let mut state = self.state.lock().unwrap();
            if state.revoked {
                return Err(Status::failed_precondition("source is revoked"));
            }
            state.counters.validations += 1;
            (
                state.registration_id.clone(),
                (Utc::now().timestamp() + 10) * 1000,
            )
        };
        let chunk = self.first_chunk();
        Ok(Response::new(HcodePeerReadAuthorization {
            source_id: request.source_id,
            registration_id,
            task_id: TASK.into(),
            artifact_id: ARTIFACT.into(),
            offset: chunk.offset,
            chunk: Some(HcodeArtifactChunkAuthorization {
                artifact_sha256: self.manifest.sha256.clone(),
                artifact_size_bytes: self.manifest.size_bytes as i64,
                chunk_sha256: chunk.sha256.clone(),
                chunk_size_bytes: chunk.size_bytes as i64,
            }),
            expires_at_unix_ms,
        }))
    }
}

async fn start_nodepool(nodepool: Arc<Nodepool>) -> Channel {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = nodepool.clone();
    tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(HcodeArtifactSourceServiceServer::from_arc(server.clone()))
            .add_service(HcodeArtifactProbeServiceServer::from_arc(server))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .unwrap();
    });
    Channel::from_shared(endpoint)
        .unwrap()
        .connect()
        .await
        .unwrap()
}

async fn serve_peer(peer: Arc<GrpcArtifactPeer>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(peer.server())
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .unwrap();
    });
    url
}

/// One full hcode chunk and a partial second chunk, so the manifest has two entries.
fn artifact_bytes() -> Vec<u8> {
    (0..CHUNK_BYTES + 4096)
        .map(|index| (index % 253) as u8)
        .collect()
}

fn identity() -> OwnerPeerIdentity {
    OwnerPeerIdentity {
        vpn_client_id: "owner-host".into(),
        headscale_node_id: 7,
    }
}

struct Fixture {
    bytes: Vec<u8>,
    manifest: ArtifactManifest,
    public: String,
    channel: Channel,
    nodepool: Arc<Nodepool>,
}

impl Fixture {
    async fn start() -> Self {
        let bytes = artifact_bytes();
        let manifest = manifest_for_bytes(ARTIFACT, &bytes).unwrap();
        let (private, public) = hivemind_config::generate_worker_execution_test_key_pair();
        let nodepool = Arc::new(Nodepool::new(manifest.clone(), private));
        let channel = start_nodepool(nodepool.clone()).await;
        Self {
            bytes,
            manifest,
            public,
            channel,
            nodepool,
        }
    }

    async fn publish(&self) -> (HostedArtifact, PinnedSource) {
        HostedArtifact::publish(
            OwnerSourceClient::new(self.channel.clone()),
            OWNER_TOKEN,
            identity(),
            TASK,
            self.manifest.clone(),
            self.bytes.clone(),
        )
        .await
        .unwrap()
    }

    /// Starts the peer on loopback and points Nodepool's probes at it.
    async fn serve(&self, pinned: PinnedSource) -> Arc<GrpcArtifactPeer> {
        let peer = Arc::new(
            GrpcArtifactPeer::new(&self.public, self.channel.clone(), vec![pinned]).unwrap(),
        );
        self.nodepool.set_peer_url(serve_peer(peer.clone()).await);
        peer
    }
}

#[tokio::test]
async fn publish_rejects_a_corrupt_local_copy_before_registering() {
    let fixture = Fixture::start().await;
    let mut corrupt = fixture.bytes.clone();
    corrupt[10] ^= 0xff;

    let result = HostedArtifact::publish(
        OwnerSourceClient::new(fixture.channel.clone()),
        OWNER_TOKEN,
        identity(),
        TASK,
        fixture.manifest.clone(),
        corrupt,
    )
    .await;

    assert!(result.is_err());
    assert_eq!(fixture.nodepool.counters().registers, 0);
}

#[tokio::test]
async fn published_source_is_probed_through_the_live_listener() {
    let fixture = Fixture::start().await;
    let (hosted, pinned) = fixture.publish().await;
    let _peer = fixture.serve(pinned).await;

    let availability = hosted.probe().await.unwrap();

    let registration = availability.source.unwrap();
    assert_eq!(registration.source_id, hosted.source_id());
    assert_eq!(registration.registration_id, hosted.registration_id());
    assert_eq!(
        fixture.nodepool.counters(),
        Counters {
            registers: 1,
            probes: 1,
            validations: 1,
        }
    );
}

#[tokio::test]
async fn renewal_applies_the_new_generation_to_the_peer_before_probing() {
    let fixture = Fixture::start().await;
    let (mut hosted, pinned) = fixture.publish().await;
    let peer = fixture.serve(pinned).await;
    hosted.probe().await.unwrap();
    let first = hosted.registration_id().to_owned();

    let renewed = hosted.renew(&peer).await.unwrap();

    assert_ne!(hosted.registration_id(), first);
    assert_eq!(
        renewed.source.unwrap().registration_id,
        hosted.registration_id()
    );
    assert_eq!(
        fixture.nodepool.counters(),
        Counters {
            registers: 2,
            probes: 2,
            validations: 2,
        }
    );
}

#[tokio::test]
async fn probe_fails_while_the_peer_still_holds_the_previous_generation() {
    let fixture = Fixture::start().await;
    let (hosted, pinned) = fixture.publish().await;
    let _peer = fixture.serve(pinned).await;
    // Nodepool moves to a new generation that the host never applies locally.
    OwnerSourceClient::new(fixture.channel.clone())
        .register(OWNER_TOKEN, TASK, ARTIFACT, &identity())
        .await
        .unwrap();

    assert!(hosted.probe().await.is_err());
    // The peer asked Nodepool, received the new generation, and refused it locally.
    assert_eq!(fixture.nodepool.counters().validations, 1);
}

#[tokio::test]
async fn revoke_stops_further_probes_and_reports_the_change() {
    let fixture = Fixture::start().await;
    let (hosted, pinned) = fixture.publish().await;
    let _peer = fixture.serve(pinned).await;
    hosted.probe().await.unwrap();
    let source_id = hosted.source_id().to_owned();

    assert!(hosted.revoke().await.unwrap());

    let client = OwnerSourceClient::new(fixture.channel.clone());
    assert!(client.probe(OWNER_TOKEN, &source_id).await.is_err());
    assert!(
        !client.revoke(OWNER_TOKEN, &source_id).await.unwrap(),
        "a second revoke finds nothing live"
    );
}

#[tokio::test]
async fn keep_published_renews_until_the_caller_stops_it() {
    let fixture = Fixture::start().await;
    let (hosted, pinned) = fixture.publish().await;
    let peer = fixture.serve(pinned).await;
    let mut hosted = [hosted];

    let outcome = tokio::time::timeout(
        Duration::from_millis(1500),
        keep_published(&peer, &mut hosted, Duration::from_secs(1)),
    )
    .await;

    assert!(outcome.is_err(), "the loop runs until the caller stops it");
    assert!(
        fixture.nodepool.counters().registers >= 2,
        "the first renewal starts immediately"
    );
}

#[tokio::test]
async fn keep_published_respects_probe_cooldown_after_manual_publication() {
    for every in [1, 2, 20] {
        let fixture = Fixture::start().await;
        let (hosted, pinned) = fixture.publish().await;
        let peer = fixture.serve(pinned).await;
        hosted.probe().await.unwrap();
        let first_generation = hosted.registration_id().to_owned();
        let mut hosted = [hosted];

        let early = tokio::time::timeout(
            Duration::from_millis(100),
            keep_published(&peer, &mut hosted, Duration::from_secs(every)),
        )
        .await;
        assert!(early.is_err());
        assert_eq!(hosted[0].registration_id(), first_generation);
        assert_eq!(fixture.nodepool.counters().registers, 1);

        let outcome = tokio::time::timeout(
            Duration::from_millis(6500),
            keep_published(&peer, &mut hosted, Duration::from_secs(every)),
        )
        .await;
        assert!(
            outcome.is_err(),
            "interval {every}: loop must not fail on cooldown"
        );
        let expected = if every == 20 { 2 } else { 3 };
        assert!(fixture.nodepool.counters().probes >= expected);
        assert_eq!(
            fixture.nodepool.counters().registers,
            fixture.nodepool.counters().probes
        );
    }
}

#[tokio::test]
async fn identities_tokens_and_intervals_are_checked_before_any_call() {
    let fixture = Fixture::start().await;
    let client = OwnerSourceClient::new(fixture.channel.clone());
    let no_node = OwnerPeerIdentity {
        vpn_client_id: "owner-host".into(),
        headscale_node_id: 0,
    };
    assert!(client
        .register(OWNER_TOKEN, TASK, ARTIFACT, &no_node)
        .await
        .is_err());
    assert!(client
        .register("", TASK, ARTIFACT, &identity())
        .await
        .is_err());
    assert!(client
        .register("token with spaces", TASK, ARTIFACT, &identity())
        .await
        .is_err());
    assert_eq!(fixture.nodepool.counters().registers, 0);

    let (hosted, pinned) = fixture.publish().await;
    let peer = fixture.serve(pinned).await;
    let mut hosted = [hosted];
    for interval in [Duration::from_millis(500), Duration::from_secs(60)] {
        assert!(
            keep_published(&peer, &mut hosted, interval).await.is_err(),
            "{interval:?}"
        );
    }
    assert_eq!(
        fixture.nodepool.counters().registers,
        1,
        "only the publish registered"
    );
}

#[test]
fn manifests_use_the_fixed_chunk_layout_and_refuse_invalid_content() {
    let bytes = artifact_bytes();
    let manifest = manifest_for_bytes(ARTIFACT, &bytes).unwrap();
    assert_eq!(manifest.chunks.len(), 2);
    assert_eq!(manifest.chunks[1].offset, CHUNK_BYTES as u64);
    assert_eq!(manifest.chunks[1].size_bytes, 4096);
    assert!(manifest_for_bytes(ARTIFACT, &[1, 2, 3]).is_err());
}

#[test]
fn artifact_files_are_read_as_regular_files_within_the_limit() {
    let directory = std::env::temp_dir().join(format!("artifact-peer-owner-{}", Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let file = directory.join("model.safetensors");
    let bytes = artifact_bytes();
    std::fs::write(&file, &bytes).unwrap();

    assert_eq!(read_artifact_file(&file).unwrap(), bytes);
    assert!(
        read_artifact_file(&directory).is_err(),
        "directories are not artifacts"
    );

    std::fs::remove_dir_all(&directory).unwrap();
}

#[tokio::test]
async fn owner_client_resolves_the_enrolled_node_and_registers_with_it() {
    let fixture = Fixture::start().await;
    let client = OwnerSourceClient::new(fixture.channel.clone());

    let resolved = client
        .resolve_identity(OWNER_TOKEN, "owner-host")
        .await
        .unwrap();
    assert_eq!(resolved, identity());

    let (hosted, pinned) = HostedArtifact::publish(
        client,
        OWNER_TOKEN,
        resolved,
        TASK,
        fixture.manifest.clone(),
        fixture.bytes.clone(),
    )
    .await
    .unwrap();
    let _peer = fixture.serve(pinned).await;
    hosted.probe().await.unwrap();
    assert_eq!(fixture.nodepool.counters().registers, 1);
}

#[tokio::test]
async fn owner_client_rejects_resolutions_that_do_not_match_the_request() {
    let fixture = Fixture::start().await;
    let client = OwnerSourceClient::new(fixture.channel.clone());
    for label in ["not-joined", "other-label", "outside-overlay"] {
        assert!(
            client.resolve_identity(OWNER_TOKEN, label).await.is_err(),
            "{label}"
        );
    }
    assert!(client.resolve_identity("", "owner-host").await.is_err());
    assert!(client.resolve_identity(OWNER_TOKEN, "").await.is_err());
    assert!(client
        .resolve_identity("wrong-token", "owner-host")
        .await
        .is_err());
}

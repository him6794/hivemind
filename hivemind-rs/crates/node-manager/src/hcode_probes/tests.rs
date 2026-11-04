use super::*;
use crate::hcode_reads::GrpcHcodeArtifactReadLeaseService;
use chrono::Utc;
use general_compute_runtime::{
    hcode_artifacts::CHUNK_BYTES, sha256_digest, ArtifactChunk, ArtifactManifest, ArtifactRole,
};
use hivemind_artifact_peer::{GrpcArtifactPeer, PinnedSource};
use hivemind_auth::{
    artifact_probe::ArtifactProbeTokenSigner,
    artifact_read::ArtifactReadScope,
    jwt_service::JwtService,
    worker_execution::{WorkerExecutionIdentity, WorkerExecutionSigner},
};
use hivemind_config::HivemindConfig;
use hivemind_database::{
    postgres::{create_isolated_test_pool, run_migrations, IsolatedTestPool},
    DatabaseManager,
};
use hivemind_models::Claims;
use hivemind_proto::{
    hcode_artifact_peer_service_client::HcodeArtifactPeerServiceClient,
    hcode_artifact_peer_service_server::{
        HcodeArtifactPeerService, HcodeArtifactPeerServiceServer,
    },
    hcode_artifact_probe_service_client::HcodeArtifactProbeServiceClient,
    hcode_artifact_probe_service_server::HcodeArtifactProbeServiceServer,
    hcode_artifact_read_lease_service_client::HcodeArtifactReadLeaseServiceClient,
    hcode_artifact_read_lease_service_server::HcodeArtifactReadLeaseServiceServer,
    HcodeArtifactChunkDownload, HcodeArtifactChunkDownloadResponse, HcodePeerProbeRequest,
    HcodePeerReadRequest, HcodeSourceAvailability, HcodeSourceRegistration, HcodeSourceRequest,
    IssueHcodeReadLeaseRequest,
};
use hivemind_vpn_service::VpnService;
use sqlx::types::Json;
use std::sync::Arc;
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::{Channel, Server};

const SECRET: &str = "hcode-probe-test-secret";

fn digest() -> String {
    sha256_digest(b"probe request identity")
}

fn token(id: Uuid, name: &str) -> String {
    JwtService::new(SECRET, 24)
        .encode_claims(&Claims {
            sub: name.into(),
            user_id: id.to_string(),
            role: None,
            task_id: None,
            worker_id: None,
            exp: (Utc::now().timestamp() + 3600) as usize,
            iat: (Utc::now().timestamp() - 60) as usize,
        })
        .unwrap()
}

struct Fixture {
    db: IsolatedTestPool,
    private: String,
    public: String,
    owner: Uuid,
    owner_token: String,
    stranger_token: String,
    task: Uuid,
    artifact: Uuid,
    source: Uuid,
    registration: Uuid,
    expires_at: chrono::DateTime<Utc>,
    manifest: ArtifactManifest,
    bytes: Vec<u8>,
}

impl Fixture {
    /// Starts a PENDING hcode task whose source is registered but not yet dispatched.
    async fn new(name: &str) -> Self {
        let db = create_isolated_test_pool(name)
            .await
            .expect("isolated PostgreSQL required");
        run_migrations(&db.pool).await.unwrap();
        let (private, public) = hivemind_config::generate_worker_execution_test_key_pair();
        let owner: Uuid = sqlx::query_scalar("INSERT INTO users (username, password_hash) VALUES ('probe-owner', 'hash') RETURNING id")
            .fetch_one(&db.pool).await.unwrap();
        let stranger: Uuid = sqlx::query_scalar("INSERT INTO users (username, password_hash) VALUES ('probe-stranger', 'hash') RETURNING id")
            .fetch_one(&db.pool).await.unwrap();
        let task: Uuid = sqlx::query_scalar("INSERT INTO tasks (task_id, owner, runtime, status, deadline) VALUES ('probe-task', 'probe-owner', 'hcode', 'PENDING', clock_timestamp() + INTERVAL '1 hour') RETURNING id")
            .fetch_one(&db.pool).await.unwrap();
        let bytes = vec![42_u8; CHUNK_BYTES + 31];
        let manifest = ArtifactManifest {
            artifact_id: "model".into(),
            role: ArtifactRole::Input,
            size_bytes: bytes.len() as u64,
            mime_type: "application/x-safetensors".into(),
            sha256: sha256_digest(&bytes),
            inline_bytes: None,
            chunks: bytes
                .chunks(CHUNK_BYTES)
                .enumerate()
                .map(|(i, data)| ArtifactChunk {
                    offset: (i * CHUNK_BYTES) as u64,
                    size_bytes: data.len() as u64,
                    sha256: sha256_digest(data),
                })
                .collect(),
        };
        // Pending artifacts are normal here: a probe checks the source, not readiness.
        let artifact: Uuid = sqlx::query_scalar("INSERT INTO general_compute_artifacts (task_id, artifact_id, sha256, size_bytes, expected_chunk_count, complete, availability_status, expires_at, owner_user_id) VALUES ('probe-task', 'model', $1, $2, 2, false, 'pending', clock_timestamp() + INTERVAL '1 hour', $3) RETURNING id")
            .bind(&manifest.sha256).bind(manifest.size_bytes as i64).bind(owner).fetch_one(&db.pool).await.unwrap();
        for chunk in &manifest.chunks {
            sqlx::query("INSERT INTO general_compute_artifact_manifest_chunks (task_id, artifact_id, offset_bytes, size_bytes, sha256) VALUES ('probe-task', 'model', $1, $2, $3)")
                .bind(chunk.offset as i64).bind(chunk.size_bytes as i64).bind(&chunk.sha256).execute(&db.pool).await.unwrap();
        }
        let peer: Uuid = sqlx::query_scalar("INSERT INTO vpn_peers (worker_id, hostname, virtual_ip, auth_key) VALUES ('probe-peer', 'source', '127.0.0.9', 'unused') RETURNING id")
            .fetch_one(&db.pool).await.unwrap();
        let enrollment: Uuid = sqlx::query_scalar("INSERT INTO vpn_peer_enrollments (peer_id, user_id, controller_url, namespace, headscale_key_id) VALUES ($1, $2, 'https://controller.invalid/api/v1/', 'nodepool', 7) RETURNING enrollment_id")
            .bind(peer).bind(owner).fetch_one(&db.pool).await.unwrap();
        let registration = Uuid::new_v4();
        let (source, expires_at): (Uuid, chrono::DateTime<Utc>) = sqlx::query_as("INSERT INTO hcode_artifact_sources (registration_id, artifact_row_id, owner_user_id, peer_id, enrollment_id, headscale_node_id, overlay_ipv4, observed_at, expires_at) VALUES ($1, $2, $3, $4, $5, 7, '100.64.42.7', clock_timestamp(), clock_timestamp() + INTERVAL '60 seconds') RETURNING source_id, expires_at")
            .bind(registration).bind(artifact).bind(owner).bind(peer).bind(enrollment).fetch_one(&db.pool).await.unwrap();
        Self {
            owner_token: token(owner, "probe-owner"),
            stranger_token: token(stranger, "probe-stranger"),
            db,
            private,
            public,
            owner,
            task,
            artifact,
            source,
            registration,
            expires_at,
            manifest,
            bytes,
        }
    }

    fn vpn(&self) -> Arc<VpnService> {
        let mut config = HivemindConfig::for_test();
        config.auth.jwt_secret = SECRET.into();
        // Owner authentication is local; the controller is not contacted here.
        config.vpn.headscale_url = "http://127.0.0.1:9".into();
        config.vpn.headscale_api_key = "unused".into();
        Arc::new(
            VpnService::new(
                config,
                DatabaseManager {
                    pool: self.db.pool.clone(),
                },
            )
            .unwrap(),
        )
    }

    fn probe_service(&self) -> GrpcHcodeArtifactProbeService {
        GrpcHcodeArtifactProbeService::new(
            self.db.pool.clone(),
            self.vpn(),
            &self.private,
            &self.public,
        )
        .unwrap()
    }

    fn read_service(&self) -> GrpcHcodeArtifactReadLeaseService {
        GrpcHcodeArtifactReadLeaseService::new(self.db.pool.clone(), &self.private, &self.public)
            .unwrap()
    }

    fn registration_message(&self) -> HcodeSourceRegistration {
        HcodeSourceRegistration {
            source_id: self.source.to_string(),
            registration_id: self.registration.to_string(),
            task_id: "probe-task".into(),
            artifact_id: "model".into(),
            artifact_sha256: self.manifest.sha256.clone(),
            artifact_size_bytes: self.manifest.size_bytes as i64,
            overlay_ipv4: "100.64.42.7".into(),
            port: 50054,
            expires_at_unix_ms: self.expires_at.timestamp_millis(),
            state: "registered_pending".into(),
        }
    }

    fn pinned(&self) -> PinnedSource {
        PinnedSource::new(
            self.registration_message(),
            self.manifest.clone(),
            self.bytes.clone(),
        )
        .unwrap()
    }

    /// A Worker execution credential for the assignment created by `assign`.
    fn execution_token(&self) -> String {
        let now = Utc::now().timestamp();
        WorkerExecutionSigner::from_pem(&self.private)
            .unwrap()
            .encode_consensus_claims(
                &Claims {
                    sub: "probe-owner".into(),
                    user_id: "probe-owner".into(),
                    role: Some("worker-execution".into()),
                    task_id: Some("probe-task".into()),
                    worker_id: Some("worker-1".into()),
                    iat: now as usize,
                    exp: (now + 3600) as usize,
                },
                &WorkerExecutionIdentity {
                    execution_id: "exec".into(),
                    attempt_id: "round:replica-1".into(),
                    idempotency_key: format!("managed-consensus-v1:{}", self.task.simple()),
                    request_digest: digest(),
                    transfer_generation: 1,
                },
                "round",
                "replica-1",
                1,
            )
            .unwrap()
    }

    /// Dispatch in the database, with the readiness that execution reads require.
    async fn assign(&self) {
        let attempt: Uuid = sqlx::query_scalar("INSERT INTO managed_consensus_attempts (task_id, execution_id, round_id, request_digest, replica_count, quorum, state, deadline) VALUES ('probe-task', 'exec', 'round', $1, 3, 2, 'running', clock_timestamp() + INTERVAL '1 hour') RETURNING id")
            .bind(digest()).fetch_one(&self.db.pool).await.unwrap();
        sqlx::query("UPDATE tasks SET managed_consensus_attempt_id = $1, status = 'RUNNING' WHERE task_id = 'probe-task'")
            .bind(attempt).execute(&self.db.pool).await.unwrap();
        sqlx::query("INSERT INTO managed_consensus_replicas (task_id, attempt_id, replica_id, worker_id, execution_id, worker_attempt_id, request_digest, state) VALUES ('probe-task', $1, 'replica-1', 'worker-1', 'exec', 'round:replica-1', $2, 'running')")
            .bind(attempt).bind(digest()).execute(&self.db.pool).await.unwrap();
        sqlx::query("UPDATE general_compute_artifacts SET complete = true, availability_status = 'available' WHERE id = $1")
            .bind(self.artifact).execute(&self.db.pool).await.unwrap();
    }

    async fn probe_row(&self) -> (Uuid, Json<ArtifactReadScope>, chrono::DateTime<Utc>) {
        sqlx::query_as(
            "SELECT probe_id, scope, expires_at FROM hcode_artifact_probes WHERE source_id = $1",
        )
        .bind(self.source)
        .fetch_one(&self.db.pool)
        .await
        .unwrap()
    }

    /// Nodepool caps a probe token at the observation row's own expiry, so the
    /// test helper must do the same. A slow probe would otherwise sign a token
    /// that outlives its row, which the live check correctly rejects.
    fn probe_token(
        &self,
        lease: Uuid,
        scope: ArtifactReadScope,
        row_expiry: chrono::DateTime<Utc>,
    ) -> String {
        ArtifactProbeTokenSigner::from_pem(&self.private)
            .unwrap()
            .sign(self.source, lease, scope, row_expiry.timestamp() as u64)
            .unwrap()
    }

    async fn sql(&self, statement: &'static str) {
        sqlx::query(statement).execute(&self.db.pool).await.unwrap();
    }

    async fn finish(self) {
        self.db.cleanup().await.unwrap();
    }
}

/// A malicious peer that returns bytes the manifest does not describe.
struct CorruptPeer;

#[tonic::async_trait]
impl HcodeArtifactPeerService for CorruptPeer {
    async fn read_chunk(
        &self,
        _: Request<HcodePeerReadRequest>,
    ) -> Result<Response<HcodeArtifactChunkDownloadResponse>, Status> {
        Err(Status::unimplemented("not used by probe tests"))
    }

    async fn probe_chunk(
        &self,
        _: Request<HcodePeerProbeRequest>,
    ) -> Result<Response<HcodeArtifactChunkDownloadResponse>, Status> {
        Ok(Response::new(HcodeArtifactChunkDownloadResponse {
            bytes: vec![0; CHUNK_BYTES],
        }))
    }
}

async fn serve_nodepool(
    listener: TcpListener,
    probe: Arc<GrpcHcodeArtifactProbeService>,
    reads: Arc<GrpcHcodeArtifactReadLeaseService>,
) {
    Server::builder()
        .add_service(HcodeArtifactProbeServiceServer::from_arc(probe))
        .add_service(HcodeArtifactReadLeaseServiceServer::from_arc(reads))
        .serve_with_incoming(TcpListenerStream::new(listener))
        .await
        .unwrap();
}

/// Real loopback gRPC: Nodepool serves both services, and a genuine
/// `GrpcArtifactPeer` serves the immutable bytes and calls Nodepool back.
struct Live {
    nodepool: Channel,
    peer: Channel,
    tasks: Vec<JoinHandle<()>>,
}

impl Live {
    async fn start(fixture: &Fixture) -> Self {
        let nodepool_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let nodepool_url = format!("http://{}", nodepool_listener.local_addr().unwrap());
        let peer_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let peer_url = format!("http://{}", peer_listener.local_addr().unwrap());
        let probe = Arc::new(fixture.probe_service().with_peer_url(peer_url.clone()));
        let reads = Arc::new(fixture.read_service());
        let mut tasks = vec![tokio::spawn(serve_nodepool(
            nodepool_listener,
            probe,
            reads,
        ))];
        let nodepool = Channel::from_shared(nodepool_url)
            .unwrap()
            .connect()
            .await
            .unwrap();
        let peer_authority =
            GrpcArtifactPeer::new(&fixture.public, nodepool.clone(), vec![fixture.pinned()])
                .unwrap();
        tasks.push(tokio::spawn(async move {
            Server::builder()
                .add_service(peer_authority.into_server())
                .serve_with_incoming(TcpListenerStream::new(peer_listener))
                .await
                .unwrap();
        }));
        let peer = Channel::from_shared(peer_url)
            .unwrap()
            .connect()
            .await
            .unwrap();
        Self {
            nodepool,
            peer,
            tasks,
        }
    }

    fn probes(&self) -> HcodeArtifactProbeServiceClient<Channel> {
        HcodeArtifactProbeServiceClient::new(self.nodepool.clone())
    }

    fn reads(&self) -> HcodeArtifactReadLeaseServiceClient<Channel> {
        HcodeArtifactReadLeaseServiceClient::new(self.nodepool.clone())
    }

    fn peer_client(&self) -> HcodeArtifactPeerServiceClient<Channel> {
        HcodeArtifactPeerServiceClient::new(self.peer.clone())
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

fn owner_request(fixture: &Fixture) -> HcodeSourceRequest {
    HcodeSourceRequest {
        token: fixture.owner_token.clone(),
        source_id: fixture.source.to_string(),
    }
}

fn discovery(fixture: &Fixture) -> HcodeArtifactChunkDownload {
    HcodeArtifactChunkDownload {
        token: fixture.execution_token(),
        task_id: "probe-task".into(),
        artifact_id: "model".into(),
        offset: 0,
    }
}

#[tokio::test]
async fn pre_dispatch_probe_observes_a_real_source_and_discovery_waits_for_assignment() {
    let fixture = Fixture::new("probe_round_trip").await;
    let live = Live::start(&fixture).await;
    let mut probes = live.probes();
    let observed = probes
        .probe_source(owner_request(&fixture))
        .await
        .unwrap()
        .into_inner();
    let registration = observed.source.clone().unwrap();
    assert_eq!(registration.source_id, fixture.source.to_string());
    assert_eq!(
        registration.registration_id,
        fixture.registration.to_string()
    );
    assert!(observed.checked_at_unix_ms <= observed.available_until_unix_ms);
    assert!(observed.available_until_unix_ms > Utc::now().timestamp_millis());

    // Before dispatch there is no execution assignment, so nothing is disclosed.
    let mut reads = live.reads();
    assert!(reads
        .list_read_sources(discovery(&fixture))
        .await
        .unwrap()
        .into_inner()
        .sources
        .is_empty());

    fixture.assign().await;
    let listed = reads
        .list_read_sources(discovery(&fixture))
        .await
        .unwrap()
        .into_inner()
        .sources;
    assert_eq!(listed.len(), 1);
    assert_eq!(
        listed[0].source.as_ref().unwrap().overlay_ipv4,
        "100.64.42.7"
    );

    // Discovery is not a read: the Worker still needs a per-chunk lease.
    let chunk = discovery(&fixture);
    let lease = reads
        .issue_read_lease(IssueHcodeReadLeaseRequest {
            source_id: fixture.source.to_string(),
            chunk: Some(chunk),
        })
        .await
        .unwrap()
        .into_inner();
    let bytes = live
        .peer_client()
        .read_chunk(HcodePeerReadRequest {
            source_id: fixture.source.to_string(),
            token: lease.token,
        })
        .await
        .unwrap()
        .into_inner()
        .bytes;
    assert_eq!(bytes.as_slice(), &fixture.bytes[..CHUNK_BYTES]);

    fixture
        .sql("UPDATE hcode_artifact_sources SET revoked_at = clock_timestamp()")
        .await;
    assert!(reads
        .list_read_sources(discovery(&fixture))
        .await
        .unwrap()
        .into_inner()
        .sources
        .is_empty());
    drop(live);
    fixture.finish().await;
}

#[tokio::test]
async fn probe_is_limited_to_the_owner_and_well_formed_source_identities() {
    let fixture = Fixture::new("probe_identity").await;
    let service = fixture.probe_service();
    let code =
        |result: Result<Response<HcodeSourceAvailability>, Status>| result.unwrap_err().code();
    let request = |token: &str, source_id: String| {
        Request::new(HcodeSourceRequest {
            token: token.into(),
            source_id,
        })
    };
    let source = fixture.source.to_string();
    assert_eq!(
        code(
            service
                .probe_source(request("not-a-token", source.clone()))
                .await
        ),
        tonic::Code::Unauthenticated
    );
    // A valid account that does not own the source gets the same generic refusal
    // as an absent source, so ownership cannot be probed by enumeration.
    assert_eq!(
        code(
            service
                .probe_source(request(&fixture.stranger_token, source.clone()))
                .await
        ),
        tonic::Code::FailedPrecondition
    );
    assert_eq!(
        code(
            service
                .probe_source(request(&fixture.owner_token, Uuid::nil().to_string()))
                .await
        ),
        tonic::Code::InvalidArgument
    );
    assert_eq!(
        code(
            service
                .probe_source(request(&fixture.owner_token, source.to_uppercase()))
                .await
        ),
        tonic::Code::InvalidArgument
    );
    assert_eq!(
        code(
            service
                .probe_source(request(&fixture.owner_token, Uuid::new_v4().to_string()))
                .await
        ),
        tonic::Code::FailedPrecondition
    );
    fixture.finish().await;
}

#[tokio::test]
async fn probe_issuance_is_rate_limited_and_keeps_one_observation_per_source() {
    let fixture = Fixture::new("probe_rate").await;
    let service = fixture.probe_service();
    let expiry = Utc::now() + chrono::Duration::hours(1);
    let first = service
        .repo
        .issue_hcode_probe(fixture.source, fixture.owner, "probe-owner", expiry)
        .await
        .unwrap();
    assert!(first.is_some());
    let second = service
        .repo
        .issue_hcode_probe(fixture.source, fixture.owner, "probe-owner", expiry)
        .await
        .unwrap();
    assert!(
        second.is_none(),
        "a second probe must wait out the attempt window"
    );
    let rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM hcode_artifact_probes WHERE source_id = $1")
            .bind(fixture.source)
            .fetch_one(&fixture.db.pool)
            .await
            .unwrap();
    assert_eq!(rows, 1);
    fixture.finish().await;
}

#[tokio::test]
async fn probe_fails_closed_when_the_source_returns_unverified_bytes() {
    let fixture = Fixture::new("probe_corrupt").await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let peer_url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        Server::builder()
            .add_service(HcodeArtifactPeerServiceServer::new(CorruptPeer))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .unwrap();
    });
    let service = Arc::new(fixture.probe_service().with_peer_url(peer_url));
    let result = service
        .probe_source(Request::new(owner_request(&fixture)))
        .await;
    assert!(result.is_err());
    let verified: Option<chrono::DateTime<Utc>> =
        sqlx::query_scalar("SELECT verified_at FROM hcode_artifact_probes WHERE source_id = $1")
            .bind(fixture.source)
            .fetch_one(&fixture.db.pool)
            .await
            .unwrap();
    assert!(
        verified.is_none(),
        "unverified bytes must leave no observation"
    );
    task.abort();
    fixture.finish().await;
}

#[tokio::test]
async fn observations_expire_or_are_revoked_before_discovery_or_validation() {
    let fixture = Fixture::new("probe_expiry").await;
    let live = Live::start(&fixture).await;
    let mut probes = live.probes();
    probes.probe_source(owner_request(&fixture)).await.unwrap();
    fixture.assign().await;
    let mut reads = live.reads();
    assert_eq!(
        reads
            .list_read_sources(discovery(&fixture))
            .await
            .unwrap()
            .into_inner()
            .sources
            .len(),
        1
    );
    let (lease, scope, row_expiry) = fixture.probe_row().await;
    let request = HcodePeerProbeRequest {
        source_id: fixture.source.to_string(),
        token: fixture.probe_token(lease, scope.0, row_expiry),
    };
    probes.validate_probe_lease(request.clone()).await.unwrap();

    fixture.sql("UPDATE hcode_artifact_probes SET available_until = clock_timestamp() - INTERVAL '1 second'").await;
    assert!(reads
        .list_read_sources(discovery(&fixture))
        .await
        .unwrap()
        .into_inner()
        .sources
        .is_empty());

    fixture
        .sql("UPDATE hcode_artifact_sources SET revoked_at = clock_timestamp()")
        .await;
    assert!(probes.validate_probe_lease(request).await.is_err());
    drop(live);
    fixture.finish().await;
}

#[tokio::test]
async fn renewal_invalidates_an_outstanding_probe_capability() {
    let fixture = Fixture::new("probe_renewal").await;
    let live = Live::start(&fixture).await;
    let mut probes = live.probes();
    probes.probe_source(owner_request(&fixture)).await.unwrap();
    let (lease, scope, row_expiry) = fixture.probe_row().await;
    let request = HcodePeerProbeRequest {
        source_id: fixture.source.to_string(),
        token: fixture.probe_token(lease, scope.0, row_expiry),
    };
    probes.validate_probe_lease(request.clone()).await.unwrap();
    fixture
        .sql("UPDATE hcode_artifact_sources SET registration_id = gen_random_uuid()")
        .await;
    assert!(probes.validate_probe_lease(request).await.is_err());
    drop(live);
    fixture.finish().await;
}

#[tokio::test]
async fn a_probe_capability_cannot_authorize_execution_or_reads() {
    let fixture = Fixture::new("probe_purpose").await;
    let live = Live::start(&fixture).await;
    let mut probes = live.probes();
    probes.probe_source(owner_request(&fixture)).await.unwrap();
    fixture.assign().await;
    let (lease, scope, row_expiry) = fixture.probe_row().await;
    let probe_token = fixture.probe_token(lease, scope.0, row_expiry);
    let source = fixture.source.to_string();
    let mut reads = live.reads();

    let as_execution = HcodeArtifactChunkDownload {
        token: probe_token.clone(),
        task_id: "probe-task".into(),
        artifact_id: "model".into(),
        offset: 0,
    };
    assert_eq!(
        reads
            .issue_read_lease(IssueHcodeReadLeaseRequest {
                source_id: source.clone(),
                chunk: Some(as_execution.clone()),
            })
            .await
            .unwrap_err()
            .code(),
        tonic::Code::Unauthenticated
    );
    assert_eq!(
        reads
            .list_read_sources(as_execution)
            .await
            .unwrap_err()
            .code(),
        tonic::Code::Unauthenticated
    );
    assert_eq!(
        reads
            .validate_read_lease(HcodePeerReadRequest {
                source_id: source.clone(),
                token: probe_token,
            })
            .await
            .unwrap_err()
            .code(),
        tonic::Code::Unauthenticated
    );
    assert_eq!(
        probes
            .validate_probe_lease(HcodePeerProbeRequest {
                source_id: source,
                token: fixture.execution_token(),
            })
            .await
            .unwrap_err()
            .code(),
        tonic::Code::Unauthenticated
    );
    drop(live);
    fixture.finish().await;
}

#[tokio::test]
async fn discovery_is_bound_to_the_current_assignment_task_and_artifact() {
    let fixture = Fixture::new("probe_scope").await;
    let live = Live::start(&fixture).await;
    let mut probes = live.probes();
    probes.probe_source(owner_request(&fixture)).await.unwrap();
    fixture.assign().await;
    let mut reads = live.reads();
    let base = discovery(&fixture);
    assert_eq!(
        reads
            .list_read_sources(base.clone())
            .await
            .unwrap()
            .into_inner()
            .sources
            .len(),
        1
    );

    let mut other_task = base.clone();
    other_task.task_id = "other-task".into();
    assert_eq!(
        reads
            .list_read_sources(other_task)
            .await
            .unwrap_err()
            .code(),
        tonic::Code::PermissionDenied
    );
    let mut other_artifact = base.clone();
    other_artifact.artifact_id = "other-model".into();
    assert!(reads
        .list_read_sources(other_artifact)
        .await
        .unwrap()
        .into_inner()
        .sources
        .is_empty());
    let mut outside_manifest = base;
    outside_manifest.offset = (CHUNK_BYTES as i64) * 5;
    assert!(reads
        .list_read_sources(outside_manifest)
        .await
        .unwrap()
        .into_inner()
        .sources
        .is_empty());
    drop(live);
    fixture.finish().await;
}

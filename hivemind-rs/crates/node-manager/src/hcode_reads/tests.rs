use super::*;
use chrono::Utc;
use general_compute_runtime::{sha256_digest, ArtifactChunk, ArtifactManifest, ArtifactRole};
use hivemind_auth::{
    artifact_read::ArtifactReadClaims,
    worker_execution::{WorkerExecutionClaims, WorkerExecutionIdentity, WorkerExecutionSigner},
};
use hivemind_database::postgres::{create_isolated_test_pool, run_migrations, IsolatedTestPool};
use hivemind_models::Claims;
use hivemind_proto::{
    hcode_artifact_peer_service_client::HcodeArtifactPeerServiceClient,
    hcode_artifact_read_lease_service_client::HcodeArtifactReadLeaseServiceClient,
    hcode_artifact_read_lease_service_server::HcodeArtifactReadLeaseServiceServer,
    HcodeArtifactChunkDownload, HcodeSourceRegistration,
};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;

struct Fixture {
    db: IsolatedTestPool,
    service: Arc<GrpcHcodeArtifactReadLeaseService>,
    private: String,
    public: String,
    claims: WorkerExecutionClaims,
    source: HcodeSourceRegistration,
    manifest: ArtifactManifest,
    bytes: Vec<u8>,
}

impl Fixture {
    async fn new(name: &str) -> Self {
        let db = create_isolated_test_pool(name)
            .await
            .expect("isolated PostgreSQL required");
        run_migrations(&db.pool).await.unwrap();
        let (private, public) = hivemind_config::generate_worker_execution_test_key_pair();
        let service = Arc::new(
            GrpcHcodeArtifactReadLeaseService::new(db.pool.clone(), &private, &public).unwrap(),
        );
        let owner: Uuid = sqlx::query_scalar("INSERT INTO users (username, password_hash) VALUES ('read-owner', 'hash') RETURNING id")
            .fetch_one(&db.pool).await.unwrap();
        let task: Uuid = sqlx::query_scalar("INSERT INTO tasks (task_id, owner, runtime, status, deadline) VALUES ('read-task', 'read-owner', 'hcode', 'RUNNING', clock_timestamp() + INTERVAL '1 hour') RETURNING id")
            .fetch_one(&db.pool).await.unwrap();
        let digest = sha256_digest(b"request identity");
        let attempt: Uuid = sqlx::query_scalar(
            "INSERT INTO managed_consensus_attempts (task_id, execution_id, round_id, request_digest, replica_count, quorum, state, deadline)
             VALUES ('read-task', 'exec', 'round', $1, 3, 2, 'running', clock_timestamp() + INTERVAL '1 hour') RETURNING id"
        ).bind(&digest).fetch_one(&db.pool).await.unwrap();
        sqlx::query(
            "UPDATE tasks SET managed_consensus_attempt_id = $1 WHERE task_id = 'read-task'",
        )
        .bind(attempt)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO managed_consensus_replicas (task_id, attempt_id, replica_id, worker_id, execution_id, worker_attempt_id, request_digest, state) VALUES ('read-task', $1, 'replica-1', 'worker-1', 'exec', 'round:replica-1', $2, 'running')")
            .bind(attempt).bind(&digest).execute(&db.pool).await.unwrap();
        let bytes = vec![42_u8; general_compute_runtime::hcode_artifacts::CHUNK_BYTES + 31];
        let manifest = ArtifactManifest {
            artifact_id: "model".into(),
            role: ArtifactRole::Input,
            size_bytes: bytes.len() as u64,
            mime_type: "application/x-safetensors".into(),
            sha256: sha256_digest(&bytes),
            inline_bytes: None,
            chunks: bytes
                .chunks(general_compute_runtime::hcode_artifacts::CHUNK_BYTES)
                .enumerate()
                .map(|(i, data)| ArtifactChunk {
                    offset: (i * general_compute_runtime::hcode_artifacts::CHUNK_BYTES) as u64,
                    size_bytes: data.len() as u64,
                    sha256: sha256_digest(data),
                })
                .collect(),
        };
        // Seed the already-ready/current-assignment control-plane state. No
        // compatibility content rows exist: this tests reads, not admission.
        let artifact: Uuid = sqlx::query_scalar("INSERT INTO general_compute_artifacts (task_id, artifact_id, sha256, size_bytes, expected_chunk_count, complete, availability_status, expires_at, owner_user_id) VALUES ('read-task', 'model', $1, $2, 2, true, 'available', clock_timestamp() + INTERVAL '1 hour', $3) RETURNING id")
            .bind(&manifest.sha256).bind(manifest.size_bytes as i64).bind(owner).fetch_one(&db.pool).await.unwrap();
        for chunk in &manifest.chunks {
            sqlx::query("INSERT INTO general_compute_artifact_manifest_chunks (task_id, artifact_id, offset_bytes, size_bytes, sha256) VALUES ('read-task', 'model', $1, $2, $3)")
                .bind(chunk.offset as i64).bind(chunk.size_bytes as i64).bind(&chunk.sha256).execute(&db.pool).await.unwrap();
        }
        let peer: Uuid = sqlx::query_scalar("INSERT INTO vpn_peers (worker_id, hostname, virtual_ip, auth_key) VALUES ('source-peer', 'source', '127.0.0.9', 'unused') RETURNING id")
            .fetch_one(&db.pool).await.unwrap();
        let enrollment: Uuid = sqlx::query_scalar("INSERT INTO vpn_peer_enrollments (peer_id, user_id, controller_url, namespace, headscale_key_id) VALUES ($1, $2, 'https://controller.invalid/api/v1/', 'nodepool', 7) RETURNING enrollment_id")
            .bind(peer).bind(owner).fetch_one(&db.pool).await.unwrap();
        let registration = Uuid::new_v4();
        let (source, expiry): (Uuid, chrono::DateTime<Utc>) = sqlx::query_as("INSERT INTO hcode_artifact_sources (registration_id, artifact_row_id, owner_user_id, peer_id, enrollment_id, headscale_node_id, overlay_ipv4, observed_at, expires_at) VALUES ($1, $2, $3, $4, $5, 7, '100.64.42.7', clock_timestamp(), clock_timestamp() + INTERVAL '60 seconds') RETURNING source_id, expires_at")
            .bind(registration).bind(artifact).bind(owner).bind(peer).bind(enrollment).fetch_one(&db.pool).await.unwrap();
        let source = HcodeSourceRegistration {
            source_id: source.to_string(),
            registration_id: registration.to_string(),
            task_id: "read-task".into(),
            artifact_id: "model".into(),
            artifact_sha256: manifest.sha256.clone(),
            artifact_size_bytes: manifest.size_bytes as i64,
            overlay_ipv4: "100.64.42.7".into(),
            port: 50054,
            expires_at_unix_ms: expiry.timestamp_millis(),
            state: "registered_pending".into(),
        };
        let claims = WorkerExecutionClaims {
            claims: Claims {
                sub: "read-owner".into(),
                user_id: "read-owner".into(),
                role: Some("worker-execution".into()),
                task_id: Some("read-task".into()),
                worker_id: Some("worker-1".into()),
                iat: Utc::now().timestamp() as usize,
                exp: (Utc::now().timestamp() + 3600) as usize,
            },
            execution_id: Some("exec".into()),
            attempt_id: Some("round:replica-1".into()),
            request_digest: Some(digest),
            idempotency_key: Some(format!("managed-consensus-v1:{}", task.simple())),
            transfer_generation: Some(1),
            consensus_round_id: Some("round".into()),
            replica_id: Some("replica-1".into()),
            consensus_protocol_version: Some(1),
        };
        Self {
            db,
            service,
            private,
            public,
            claims,
            source,
            manifest,
            bytes,
        }
    }

    fn request(&self, claims: &WorkerExecutionClaims, offset: i64) -> IssueHcodeReadLeaseRequest {
        let token = WorkerExecutionSigner::from_pem(&self.private)
            .unwrap()
            .encode_consensus_claims(
                &claims.claims,
                &WorkerExecutionIdentity {
                    execution_id: claims.execution_id.clone().unwrap(),
                    attempt_id: claims.attempt_id.clone().unwrap(),
                    idempotency_key: claims.idempotency_key.clone().unwrap(),
                    request_digest: claims.request_digest.clone().unwrap(),
                    transfer_generation: claims.transfer_generation.unwrap(),
                },
                claims.consensus_round_id.as_deref().unwrap(),
                claims.replica_id.as_deref().unwrap(),
                claims.consensus_protocol_version.unwrap(),
            )
            .unwrap();
        IssueHcodeReadLeaseRequest {
            source_id: self.source.source_id.clone(),
            chunk: Some(HcodeArtifactChunkDownload {
                token,
                task_id: "read-task".into(),
                artifact_id: "model".into(),
                offset,
            }),
        }
    }

    async fn issue(&self) -> HcodeArtifactReadLease {
        self.service
            .issue_read_lease(Request::new(self.request(&self.claims, 0)))
            .await
            .unwrap()
            .into_inner()
    }

    fn read(&self, lease: &HcodeArtifactReadLease) -> HcodePeerReadRequest {
        HcodePeerReadRequest {
            source_id: self.source.source_id.clone(),
            token: lease.token.clone(),
        }
    }

    async fn validate(
        &self,
        lease: &HcodeArtifactReadLease,
    ) -> Result<Response<HcodePeerReadAuthorization>, Status> {
        self.service
            .validate_read_lease(Request::new(self.read(lease)))
            .await
    }

    async fn sql(&self, sql: &'static str) {
        sqlx::query(sql).execute(&self.db.pool).await.unwrap();
    }

    async fn finish(self) {
        self.db.cleanup().await.unwrap();
    }
}

#[tokio::test]
async fn peer_read_constructor_and_request_validation_fail_closed() {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
        .unwrap();
    let (private, public) = hivemind_config::generate_worker_execution_test_key_pair();
    let (_, other_public) = hivemind_config::generate_worker_execution_test_key_pair();
    assert!(GrpcHcodeArtifactReadLeaseService::new(pool.clone(), &private, &other_public).is_err());
    assert!(GrpcHcodeArtifactReadLeaseService::new(pool.clone(), "bad", &public).is_err());
    let service = GrpcHcodeArtifactReadLeaseService::new(pool, &private, &public).unwrap();
    let valid = IssueHcodeReadLeaseRequest {
        source_id: Uuid::new_v4().to_string(),
        chunk: Some(HcodeArtifactChunkDownload {
            token: "bad-signature".into(),
            task_id: "task".into(),
            artifact_id: "model".into(),
            offset: 0,
        }),
    };
    for case in 0..9 {
        let mut request = valid.clone();
        match case {
            0 => request.source_id = Uuid::nil().to_string(),
            1 => request.source_id = "../file".into(),
            2 => request.chunk = None,
            3 => request.chunk.as_mut().unwrap().task_id = "task..name".into(),
            4 => request.chunk.as_mut().unwrap().artifact_id = "../model".into(),
            5 => request.chunk.as_mut().unwrap().offset = -1,
            6 => request.chunk.as_mut().unwrap().token.clear(),
            7 => {
                request.chunk.as_mut().unwrap().token =
                    "a".repeat(hivemind_proto::HCODE_ARTIFACT_EXECUTION_TOKEN_MAX_BYTES + 1)
            }
            _ => {}
        }
        let error = service
            .issue_read_lease(Request::new(request))
            .await
            .unwrap_err();
        assert!(matches!(
            error.code(),
            tonic::Code::InvalidArgument | tonic::Code::Unauthenticated
        ));
    }
}

#[tokio::test]
async fn peer_read_real_rpc_transfers_bounded_chunks_without_nodepool_bytes() {
    let fixture = Fixture::new("peer_read_socket").await;
    run_migrations(&fixture.db.pool).await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let nodepool_url = format!("http://{}", listener.local_addr().unwrap());
    let service = fixture.service.clone();
    let nodepool = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(
                HcodeArtifactReadLeaseServiceServer::from_arc(service)
                    .max_decoding_message_size(HCODE_READ_LEASE_RPC_MAX_BYTES)
                    .max_encoding_message_size(HCODE_READ_LEASE_RPC_MAX_BYTES),
            )
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .unwrap();
    });
    let channel = tonic::transport::Endpoint::from_shared(nodepool_url)
        .unwrap()
        .connect()
        .await
        .unwrap();
    let pinned = hivemind_artifact_peer::PinnedSource::new(
        fixture.source.clone(),
        fixture.manifest.clone(),
        fixture.bytes.clone(),
    )
    .unwrap();
    let peer_host = Arc::new(
        hivemind_artifact_peer::GrpcArtifactPeer::new(
            &fixture.public,
            channel.clone(),
            vec![pinned],
        )
        .unwrap(),
    );
    let peer_service = peer_host.clone();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let peer_url = format!("http://{}", listener.local_addr().unwrap());
    let peer_server = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(peer_service.server())
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .unwrap();
    });
    let mut issuer = HcodeArtifactReadLeaseServiceClient::new(channel);
    let mut peer = HcodeArtifactPeerServiceClient::connect(peer_url)
        .await
        .unwrap();
    let mut complete = Vec::new();
    let mut last = None;
    for chunk in &fixture.manifest.chunks {
        let request = fixture.request(&fixture.claims, chunk.offset as i64);
        let execution_token = request.chunk.as_ref().unwrap().token.clone();
        let lease = issuer.issue_read_lease(request).await.unwrap().into_inner();
        assert_eq!(lease.overlay_ipv4, "100.64.42.7");
        assert_eq!(lease.port, 50054);
        assert_ne!(execution_token, lease.token);
        assert!(WorkerExecutionVerifier::from_pem(&fixture.public)
            .unwrap()
            .decode_execution_claims(&lease.token)
            .is_err());
        assert!(peer
            .read_chunk(HcodePeerReadRequest {
                source_id: fixture.source.source_id.clone(),
                token: execution_token
            })
            .await
            .is_err());
        let bytes = peer
            .read_chunk(fixture.read(&lease))
            .await
            .unwrap()
            .into_inner()
            .bytes;
        assert_eq!(bytes.len() as u64, chunk.size_bytes);
        assert_eq!(sha256_digest(&bytes), chunk.sha256);
        complete.extend(bytes);
        last = Some(lease);
    }
    assert_eq!(sha256_digest(&complete), fixture.manifest.sha256);
    assert_eq!(complete, fixture.bytes);
    let new_registration: Uuid = sqlx::query_scalar(
        "UPDATE hcode_artifact_sources SET registration_id = gen_random_uuid() RETURNING registration_id"
    ).fetch_one(&fixture.db.pool).await.unwrap();
    assert!(peer
        .read_chunk(fixture.read(last.as_ref().unwrap()))
        .await
        .is_err());
    let renewed = fixture.issue().await;
    // Nodepool has renewed, but the host still serves the old binding.
    assert!(peer.read_chunk(fixture.read(&renewed)).await.is_err());
    let mut registration = fixture.source.clone();
    registration.registration_id = new_registration.to_string();
    let mut wrong = registration.clone();
    wrong.artifact_sha256 = sha256_digest(b"wrong");
    assert!(peer_host.renew_source(wrong).is_err());
    peer_host.renew_source(registration).unwrap();
    assert!(peer.read_chunk(fixture.read(&renewed)).await.is_ok());
    last = Some(renewed);
    fixture
        .sql("UPDATE hcode_artifact_sources SET revoked_at = clock_timestamp()")
        .await;
    assert_eq!(
        peer.read_chunk(fixture.read(last.as_ref().unwrap()))
            .await
            .unwrap_err()
            .code(),
        tonic::Code::PermissionDenied
    );
    for query in [
        "SELECT COUNT(*) FROM general_compute_artifact_chunks",
        "SELECT COUNT(*) FROM general_compute_artifact_sources",
        "SELECT COUNT(*) FROM managed_consensus_usage_holds",
        "SELECT COUNT(*) FROM ledger_entries",
    ] {
        let count: i64 = sqlx::query_scalar(query)
            .fetch_one(&fixture.db.pool)
            .await
            .unwrap();
        assert_eq!(count, 0, "{query}");
    }
    assert!(peer
        .read_chunk(HcodePeerReadRequest {
            source_id: fixture.source.source_id.clone(),
            token: "x".repeat(64 * 1024)
        })
        .await
        .is_err());
    peer_server.abort();
    nodepool.abort();
    fixture.finish().await;
}

#[tokio::test]
async fn peer_read_live_gates_reject_changed_assignment_source_owner_and_deadlines() {
    let fixture = Fixture::new("peer_read_fences").await;
    let lease = fixture.issue().await;
    for (mutation, restore) in [
        ("UPDATE tasks SET status = 'COMPLETED'", "UPDATE tasks SET status = 'RUNNING'"),
        ("UPDATE tasks SET runtime = 'managed-function-v0'", "UPDATE tasks SET runtime = 'hcode'"),
        ("UPDATE tasks SET deadline = clock_timestamp() - INTERVAL '1 second'", "UPDATE tasks SET deadline = clock_timestamp() + INTERVAL '1 hour'"),
        ("UPDATE tasks SET managed_consensus_attempt_id = NULL", "UPDATE tasks SET managed_consensus_attempt_id = (SELECT id FROM managed_consensus_attempts WHERE task_id = 'read-task')"),
        ("UPDATE managed_consensus_attempts SET state = 'stop_pending'", "UPDATE managed_consensus_attempts SET state = 'running'"),
        ("UPDATE managed_consensus_attempts SET deadline = clock_timestamp() - INTERVAL '1 second'", "UPDATE managed_consensus_attempts SET deadline = clock_timestamp() + INTERVAL '1 hour'"),
        ("UPDATE managed_consensus_attempts SET execution_id = 'other'", "UPDATE managed_consensus_attempts SET execution_id = 'exec'"),
        ("UPDATE managed_consensus_attempts SET round_id = 'other'", "UPDATE managed_consensus_attempts SET round_id = 'round'"),
        ("UPDATE managed_consensus_replicas SET worker_id = 'worker-other'", "UPDATE managed_consensus_replicas SET worker_id = 'worker-1'"),
        ("UPDATE managed_consensus_replicas SET worker_attempt_id = 'old'", "UPDATE managed_consensus_replicas SET worker_attempt_id = 'round:replica-1'"),
        ("UPDATE managed_consensus_replicas SET state = 'stopped'", "UPDATE managed_consensus_replicas SET state = 'running'"),
        ("UPDATE managed_consensus_replicas SET transfer_generation = 2", "UPDATE managed_consensus_replicas SET transfer_generation = NULL"),
        ("UPDATE general_compute_artifacts SET complete = false", "UPDATE general_compute_artifacts SET complete = true"),
        ("UPDATE general_compute_artifacts SET availability_status = 'pending'", "UPDATE general_compute_artifacts SET availability_status = 'available'"),
        ("UPDATE general_compute_artifacts SET expires_at = clock_timestamp() - INTERVAL '1 second'", "UPDATE general_compute_artifacts SET expires_at = clock_timestamp() + INTERVAL '1 hour'"),
        ("UPDATE users SET is_active = false", "UPDATE users SET is_active = true"),
        ("UPDATE hcode_artifact_sources SET revoked_at = clock_timestamp()", "UPDATE hcode_artifact_sources SET revoked_at = NULL"),
        ("UPDATE hcode_artifact_sources SET observed_at = clock_timestamp() - INTERVAL '2 hours', expires_at = clock_timestamp() - INTERVAL '1 hour'", "UPDATE hcode_artifact_sources SET observed_at = clock_timestamp(), expires_at = clock_timestamp() + INTERVAL '60 seconds'"),
        ("UPDATE general_compute_artifacts SET owner_user_id = NULL", "UPDATE general_compute_artifacts SET owner_user_id = (SELECT id FROM users WHERE username = 'read-owner')"),
    ] {
        assert!(fixture.validate(&lease).await.is_ok(), "before {mutation}");
        fixture.sql(mutation).await;
        assert!(fixture.validate(&lease).await.is_err(), "read accepted: {mutation}");
        assert!(fixture.service.issue_read_lease(Request::new(fixture.request(&fixture.claims, 0))).await.is_err(), "issue accepted: {mutation}");
        fixture.sql(restore).await;
    }
    fixture.sql("UPDATE tasks SET deadline = NULL").await;
    assert!(fixture.validate(&lease).await.is_ok());
    fixture.finish().await;
}

#[tokio::test]
async fn peer_read_renewal_enrollment_rotation_and_recreated_owner_invalidate_tokens() {
    let fixture = Fixture::new("peer_read_generations").await;
    let old = fixture.issue().await;
    fixture
        .sql("UPDATE hcode_artifact_sources SET registration_id = gen_random_uuid()")
        .await;
    assert!(fixture.validate(&old).await.is_err());
    let renewed = fixture.issue().await;
    assert!(fixture.validate(&renewed).await.is_ok());
    fixture
        .sql("UPDATE vpn_peer_enrollments SET enrollment_id = gen_random_uuid()")
        .await;
    assert!(fixture.validate(&renewed).await.is_err());
    assert!(fixture
        .service
        .issue_read_lease(Request::new(fixture.request(&fixture.claims, 0)))
        .await
        .is_err());
    fixture
        .sql("DELETE FROM users WHERE username = 'read-owner'")
        .await;
    fixture
        .sql("INSERT INTO users (username, password_hash) VALUES ('read-owner', 'new-hash')")
        .await;
    assert!(fixture.validate(&renewed).await.is_err());
    assert!(fixture
        .service
        .issue_read_lease(Request::new(fixture.request(&fixture.claims, 0)))
        .await
        .is_err());
    fixture.finish().await;
}

#[tokio::test]
async fn peer_read_reissue_is_bounded_and_invalidates_previous_capability() {
    let fixture = Fixture::new("peer_read_replace").await;
    let first = fixture.issue().await;
    let second = fixture.issue().await;
    assert!(fixture.validate(&first).await.is_err());
    assert!(fixture.validate(&second).await.is_ok());
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM hcode_artifact_read_leases")
        .fetch_one(&fixture.db.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    let saved: serde_json::Value =
        sqlx::query_scalar("SELECT execution_claims FROM hcode_artifact_read_leases")
            .fetch_one(&fixture.db.pool)
            .await
            .unwrap();
    assert!(!saved
        .to_string()
        .contains(&fixture.request(&fixture.claims, 0).chunk.unwrap().token));
    fixture.sql("UPDATE hcode_artifact_read_leases SET expires_at = clock_timestamp() - INTERVAL '1 second'").await;
    assert!(fixture.validate(&second).await.is_err());
    fixture.finish().await;
}

#[tokio::test]
async fn peer_read_rejects_wrong_execution_coordinates_and_strictly_expired_tokens() {
    let fixture = Fixture::new("peer_read_execution_scope").await;
    for case in 0..12 {
        let mut claims = fixture.claims.clone();
        match case {
            0 => claims.claims.sub = "other-owner".into(),
            1 => claims.claims.worker_id = Some("other-worker".into()),
            2 => claims.execution_id = Some("old-exec".into()),
            3 => claims.attempt_id = Some("old-attempt".into()),
            4 => claims.idempotency_key = Some("wrong-key".into()),
            5 => claims.request_digest = Some(sha256_digest(b"changed")),
            6 => claims.transfer_generation = Some(2),
            7 => claims.consensus_round_id = Some("old-round".into()),
            8 => claims.replica_id = Some("other-replica".into()),
            9 => claims.consensus_protocol_version = Some(2),
            10 => claims.claims.role = Some("admin".into()),
            _ => claims.claims.exp = (Utc::now().timestamp() - 2) as usize,
        }
        assert!(
            fixture
                .service
                .issue_read_lease(Request::new(fixture.request(&claims, 0)))
                .await
                .is_err(),
            "case {case}"
        );
    }
    let mut request = fixture.request(&fixture.claims, 1);
    assert!(fixture
        .service
        .issue_read_lease(Request::new(request.clone()))
        .await
        .is_err());
    request.chunk.as_mut().unwrap().offset = 0;
    request.source_id = Uuid::new_v4().to_string();
    assert!(fixture
        .service
        .issue_read_lease(Request::new(request))
        .await
        .is_err());
    fixture.finish().await;
}

#[tokio::test]
async fn peer_read_caps_expiry_at_every_authority_and_handles_null_deadline() {
    let fixture = Fixture::new("peer_read_ttl").await;
    fixture.sql("UPDATE tasks SET deadline = NULL").await;
    for (update, restore) in [
        ("UPDATE tasks SET deadline = clock_timestamp() + INTERVAL '20 seconds' RETURNING deadline", "UPDATE tasks SET deadline = NULL"),
        ("UPDATE general_compute_artifacts SET expires_at = clock_timestamp() + INTERVAL '20 seconds' RETURNING expires_at", "UPDATE general_compute_artifacts SET expires_at = clock_timestamp() + INTERVAL '1 hour'"),
        ("UPDATE managed_consensus_attempts SET deadline = clock_timestamp() + INTERVAL '20 seconds' RETURNING deadline", "UPDATE managed_consensus_attempts SET deadline = clock_timestamp() + INTERVAL '1 hour'"),
        ("UPDATE hcode_artifact_sources SET expires_at = clock_timestamp() + INTERVAL '20 seconds' RETURNING expires_at", "UPDATE hcode_artifact_sources SET expires_at = clock_timestamp() + INTERVAL '60 seconds'"),
    ] {
        let cap: chrono::DateTime<Utc> = sqlx::query_scalar(update).fetch_one(&fixture.db.pool).await.unwrap();
        let lease = fixture.issue().await;
        let expiry = lease.authorization.as_ref().unwrap().expires_at_unix_ms;
        assert!(expiry > Utc::now().timestamp_millis());
        assert!(expiry <= cap.timestamp_millis());
        assert!(fixture.validate(&lease).await.is_ok());
        fixture.sql(restore).await;
    }
    let mut claims = fixture.claims.clone();
    claims.claims.exp = (Utc::now().timestamp() + 15) as usize;
    let lease = fixture
        .service
        .issue_read_lease(Request::new(fixture.request(&claims, 0)))
        .await
        .unwrap()
        .into_inner();
    assert!(lease.authorization.unwrap().expires_at_unix_ms <= (claims.claims.exp * 1000) as i64);
    fixture.finish().await;
}

#[tokio::test]
async fn peer_read_rejects_signed_but_unpersisted_or_changed_scope() {
    let fixture = Fixture::new("peer_read_scope").await;
    let lease = fixture.issue().await;
    let read = fixture
        .service
        .verifier
        .verify(
            &lease.token,
            Uuid::parse_str(&fixture.source.source_id).unwrap(),
        )
        .unwrap();
    for case in 0..8 {
        let mut scope = read.scope.clone();
        let mut id = read.lease_id;
        match case {
            0 => scope.task_id = "other-task".into(),
            1 => scope.artifact_id = "other-model".into(),
            2 => scope.artifact_sha256 = sha256_digest(b"other"),
            3 => scope.artifact_size_bytes += 1,
            4 => scope.offset = 1,
            5 => scope.chunk_sha256 = sha256_digest(b"other"),
            6 => scope.chunk_size_bytes -= 1,
            _ => id = Uuid::new_v4(),
        }
        let token = fixture
            .service
            .signer
            .sign(read.aud, id, scope, read.exp)
            .unwrap();
        let request = HcodePeerReadRequest {
            source_id: fixture.source.source_id.clone(),
            token,
        };
        assert!(
            fixture
                .service
                .validate_read_lease(Request::new(request))
                .await
                .is_err(),
            "case {case}"
        );
    }
    fixture.sql("UPDATE general_compute_artifact_manifest_chunks SET sha256 = 'sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' WHERE offset_bytes = 0").await;
    assert!(fixture.validate(&lease).await.is_err());
    let current: ArtifactReadClaims = fixture
        .service
        .verifier
        .verify(&lease.token, read.aud)
        .unwrap();
    assert_eq!(current.lease_id, read.lease_id); // signature alone remains valid
    fixture.finish().await;
}

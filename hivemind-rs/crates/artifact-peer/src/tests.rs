use super::*;
use general_compute_runtime::ArtifactChunk;
use hivemind_auth::artifact_read::ArtifactReadTokenSigner;
use hivemind_proto::{
    hcode_artifact_read_lease_service_server::{
        HcodeArtifactReadLeaseService, HcodeArtifactReadLeaseServiceServer,
    },
    HcodeArtifactChunkAuthorization, HcodeArtifactReadLease, HcodePeerReadAuthorization,
    IssueHcodeReadLeaseRequest,
};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex,
};
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;

struct Authority {
    grant: Mutex<Result<HcodePeerReadAuthorization, Status>>,
    tokens: Mutex<Vec<String>>,
    calls: AtomicUsize,
    gate: Option<Arc<Semaphore>>,
}

#[tonic::async_trait]
impl HcodeArtifactReadLeaseService for Authority {
    async fn issue_read_lease(
        &self,
        _: Request<IssueHcodeReadLeaseRequest>,
    ) -> Result<Response<HcodeArtifactReadLease>, Status> {
        panic!("a peer must never ask for execution credentials");
    }
    async fn validate_read_lease(
        &self,
        request: Request<HcodePeerReadRequest>,
    ) -> Result<Response<HcodePeerReadAuthorization>, Status> {
        self.tokens.lock().unwrap().push(request.into_inner().token);
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(gate) = &self.gate {
            gate.acquire().await.unwrap().forget();
        }
        self.grant.lock().unwrap().clone().map(Response::new)
    }
}

struct Material {
    source: HcodeSourceRegistration,
    manifest: ArtifactManifest,
    bytes: Vec<u8>,
    scope: ArtifactReadScope,
    private: String,
    public: String,
}

impl Material {
    fn new() -> Self {
        let bytes = b"immutable artifact content".to_vec();
        let digest = sha256_digest(&bytes);
        let manifest = ArtifactManifest {
            artifact_id: "model".into(),
            role: ArtifactRole::Input,
            size_bytes: bytes.len() as u64,
            mime_type: "application/x-safetensors".into(),
            sha256: digest.clone(),
            chunks: vec![ArtifactChunk {
                offset: 0,
                size_bytes: bytes.len() as u64,
                sha256: digest.clone(),
            }],
            inline_bytes: None,
        };
        let source = HcodeSourceRegistration {
            source_id: Uuid::new_v4().to_string(),
            registration_id: Uuid::new_v4().to_string(),
            task_id: "task".into(),
            artifact_id: "model".into(),
            artifact_sha256: digest.clone(),
            artifact_size_bytes: bytes.len() as i64,
            overlay_ipv4: "100.64.1.2".into(),
            port: 50054,
            expires_at_unix_ms: (Utc::now().timestamp() + 60) * 1000,
            state: "registered_pending".into(),
        };
        let scope = ArtifactReadScope {
            task_id: "task".into(),
            artifact_id: "model".into(),
            artifact_sha256: digest.clone(),
            artifact_size_bytes: bytes.len() as u64,
            offset: 0,
            chunk_sha256: digest,
            chunk_size_bytes: bytes.len() as u64,
        };
        let (private, public) = hivemind_config::generate_worker_execution_test_key_pair();
        Self {
            source,
            manifest,
            bytes,
            scope,
            private,
            public,
        }
    }
    fn pinned(&self) -> PinnedSource {
        PinnedSource::new(
            self.source.clone(),
            self.manifest.clone(),
            self.bytes.clone(),
        )
        .unwrap()
    }
    fn request(&self, scope: ArtifactReadScope) -> HcodePeerReadRequest {
        HcodePeerReadRequest {
            source_id: self.source.source_id.clone(),
            token: ArtifactReadTokenSigner::from_pem(&self.private)
                .unwrap()
                .sign(
                    Uuid::parse_str(&self.source.source_id).unwrap(),
                    Uuid::new_v4(),
                    scope,
                    (self.source.expires_at_unix_ms / 1000) as u64,
                )
                .unwrap(),
        }
    }
    fn grant(&self) -> HcodePeerReadAuthorization {
        HcodePeerReadAuthorization {
            source_id: self.source.source_id.clone(),
            registration_id: self.source.registration_id.clone(),
            task_id: "task".into(),
            artifact_id: "model".into(),
            offset: 0,
            expires_at_unix_ms: self.source.expires_at_unix_ms,
            chunk: Some(HcodeArtifactChunkAuthorization {
                artifact_sha256: self.scope.artifact_sha256.clone(),
                artifact_size_bytes: self.bytes.len() as i64,
                chunk_sha256: self.scope.chunk_sha256.clone(),
                chunk_size_bytes: self.bytes.len() as i64,
            }),
        }
    }
}

async fn server(authority: Arc<Authority>) -> (Channel, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(HcodeArtifactReadLeaseServiceServer::from_arc(authority))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .unwrap();
    });
    (
        Channel::from_shared(endpoint)
            .unwrap()
            .connect()
            .await
            .unwrap(),
        task,
    )
}

#[test]
fn pinned_source_rejects_corruption_scope_and_manifest_limits() {
    let material = Material::new();
    for case in 0..9 {
        let mut source = material.source.clone();
        let mut manifest = material.manifest.clone();
        let mut bytes = material.bytes.clone();
        match case {
            0 => bytes[0] ^= 1,
            1 => source.artifact_sha256 = sha256_digest(b"wrong"),
            2 => source.artifact_size_bytes += 1,
            3 => source.artifact_id = "other".into(),
            4 => manifest.chunks[0].sha256 = sha256_digest(b"wrong"),
            5 => manifest.chunks[0].offset = 1,
            6 => manifest.role = ArtifactRole::Output,
            7 => source.registration_id = Uuid::nil().to_string(),
            _ => source.task_id = "../file".into(),
        }
        assert!(
            PinnedSource::new(source, manifest, bytes).is_err(),
            "case {case}"
        );
    }
}

#[tokio::test]
async fn peer_checks_every_read_and_never_caches_success_or_forwards_execution_authority() {
    let material = Material::new();
    let authority = Arc::new(Authority {
        grant: Mutex::new(Ok(material.grant())),
        tokens: Mutex::new(vec![]),
        calls: AtomicUsize::new(0),
        gate: None,
    });
    let (channel, task) = server(authority.clone()).await;
    let peer = GrpcArtifactPeer::new(&material.public, channel, vec![material.pinned()]).unwrap();
    let request = material.request(material.scope.clone());
    for _ in 0..2 {
        assert_eq!(
            peer.read_chunk(Request::new(request.clone()))
                .await
                .unwrap()
                .into_inner()
                .bytes,
            material.bytes
        );
    }
    *authority.grant.lock().unwrap() = Err(Status::permission_denied("revoked"));
    assert_eq!(
        peer.read_chunk(Request::new(request.clone()))
            .await
            .unwrap_err()
            .code(),
        tonic::Code::PermissionDenied
    );
    assert_eq!(
        *authority.tokens.lock().unwrap(),
        vec![request.token.clone(); 3]
    );
    let mut invalid = request.clone();
    invalid.token = "not-a-read-capability".into();
    assert!(peer.read_chunk(Request::new(invalid)).await.is_err());
    let mut scope = material.scope.clone();
    scope.offset = 1;
    scope.chunk_size_bytes -= 1;
    assert!(peer
        .read_chunk(Request::new(material.request(scope)))
        .await
        .is_err());
    assert_eq!(authority.calls.load(Ordering::SeqCst), 3);
    task.abort();
}

#[tokio::test]
async fn peer_rejects_changed_live_scope_and_expiry_after_validation() {
    let material = Material::new();
    let authority = Arc::new(Authority {
        grant: Mutex::new(Ok(material.grant())),
        tokens: Mutex::new(vec![]),
        calls: AtomicUsize::new(0),
        gate: None,
    });
    let (channel, task) = server(authority.clone()).await;
    let peer = GrpcArtifactPeer::new(&material.public, channel, vec![material.pinned()]).unwrap();
    let request = material.request(material.scope.clone());
    for case in 0..10 {
        let mut grant = material.grant();
        match case {
            0 => grant.source_id = Uuid::new_v4().to_string(),
            1 => grant.registration_id = Uuid::new_v4().to_string(),
            2 => grant.task_id = "other".into(),
            3 => grant.artifact_id = "other".into(),
            4 => grant.offset = 1,
            5 => grant.chunk = None,
            6 => grant.chunk.as_mut().unwrap().chunk_size_bytes += 1,
            7 => grant.chunk.as_mut().unwrap().chunk_sha256 = sha256_digest(b"wrong"),
            8 => grant.expires_at_unix_ms = Utc::now().timestamp_millis() - 1,
            _ => grant.expires_at_unix_ms += 1000,
        }
        *authority.grant.lock().unwrap() = Ok(grant);
        assert_eq!(
            peer.read_chunk(Request::new(request.clone()))
                .await
                .unwrap_err()
                .code(),
            tonic::Code::PermissionDenied,
            "case {case}"
        );
    }
    task.abort();
}

#[tokio::test]
async fn peer_fails_closed_when_nodepool_is_unreachable_or_stalls() {
    let material = Material::new();
    let channel = Channel::from_static("http://127.0.0.1:0").connect_lazy();
    let peer = GrpcArtifactPeer::new(&material.public, channel, vec![material.pinned()]).unwrap();
    assert!(peer
        .read_chunk(Request::new(material.request(material.scope.clone())))
        .await
        .is_err());
    let authority = Arc::new(Authority {
        grant: Mutex::new(Ok(material.grant())),
        tokens: Mutex::new(vec![]),
        calls: AtomicUsize::new(0),
        gate: Some(Arc::new(Semaphore::new(0))),
    });
    let (channel, task) = server(authority).await;
    let peer = GrpcArtifactPeer::new(&material.public, channel, vec![material.pinned()]).unwrap();
    let error = tokio::time::timeout(
        Duration::from_secs(7),
        peer.read_chunk(Request::new(material.request(material.scope.clone()))),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert_eq!(error.code(), tonic::Code::Unavailable);
    task.abort();
}

#[tokio::test]
async fn peer_bounds_in_flight_reads_and_source_count() {
    let material = Material::new();
    let gate = Arc::new(Semaphore::new(0));
    let authority = Arc::new(Authority {
        grant: Mutex::new(Ok(material.grant())),
        tokens: Mutex::new(vec![]),
        calls: AtomicUsize::new(0),
        gate: Some(gate.clone()),
    });
    let (channel, task) = server(authority.clone()).await;
    assert!(GrpcArtifactPeer::new(&material.public, channel.clone(), vec![]).is_err());
    assert!(GrpcArtifactPeer::new(
        &material.public,
        channel.clone(),
        vec![material.pinned(), material.pinned()]
    )
    .is_err());
    let peer = Arc::new(
        GrpcArtifactPeer::new(&material.public, channel, vec![material.pinned()]).unwrap(),
    );
    let request = material.request(material.scope.clone());
    let mut reads = Vec::new();
    for _ in 0..MAX_CONCURRENT_READS {
        let peer = peer.clone();
        let request = request.clone();
        reads.push(tokio::spawn(async move {
            peer.read_chunk(Request::new(request)).await
        }));
    }
    tokio::time::timeout(Duration::from_secs(3), async {
        while authority.calls.load(Ordering::SeqCst) < MAX_CONCURRENT_READS {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        peer.read_chunk(Request::new(request))
            .await
            .unwrap_err()
            .code(),
        tonic::Code::ResourceExhausted
    );
    gate.add_permits(MAX_CONCURRENT_READS);
    for read in reads {
        assert!(read.await.unwrap().is_ok());
    }
    task.abort();
}

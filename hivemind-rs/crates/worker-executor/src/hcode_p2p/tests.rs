use super::*;

use chrono::Utc;
use general_compute_runtime::ArtifactRole;
use hivemind_proto::{
    hcode_artifact_peer_service_client::HcodeArtifactPeerServiceClient,
    hcode_artifact_peer_service_server::{
        HcodeArtifactPeerService, HcodeArtifactPeerServiceServer,
    },
    HcodeArtifactChunkAuthorization, HcodeArtifactChunkDownloadResponse, HcodePeerProbeRequest,
    HcodePeerReadAuthorization,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Endpoint;
use tonic::{Request, Response, Status};

const EXECUTION_TOKEN: &str = "execution-token-never-forwarded";
const TASK_ID: &str = "task-1";
const CHUNK: usize = 4096;
const SOURCE_IDS: [&str; 9] = [
    "source-0", "source-1", "source-2", "source-3", "source-4", "source-5", "source-6", "source-7",
    "source-8",
];

fn payload() -> Vec<u8> {
    (0..10_000_u32).map(|value| (value % 251) as u8).collect()
}

fn manifest(data: &[u8]) -> ArtifactManifest {
    let chunks = data
        .chunks(CHUNK)
        .enumerate()
        .map(|(index, part)| ArtifactChunk {
            offset: (index * CHUNK) as u64,
            size_bytes: part.len() as u64,
            sha256: sha256_digest(part),
        })
        .collect();
    ArtifactManifest {
        artifact_id: "model".into(),
        role: ArtifactRole::Input,
        size_bytes: data.len() as u64,
        mime_type: "application/x-safetensors".into(),
        sha256: sha256_digest(data),
        chunks,
        inline_bytes: None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Behavior {
    Honest,
    Corrupt,
    Short,
    Refuse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tamper {
    None,
    StaleObservation,
    WrongArtifactDigest,
    WrongRegistrationPort,
    ExpiredRegistration,
    LeaseDenied,
    LeaseWrongRegistration,
    LeaseWrongChunkDigest,
    LeaseWrongPort,
    LeaseWrongOverlay,
    LeaseExpired,
}

/// A real loopback gRPC source. It records every token it receives so the
/// tests can prove which credential reached the peer.
struct Peer {
    data: Arc<Vec<u8>>,
    behavior: Behavior,
    seen_tokens: Arc<Mutex<Vec<String>>>,
}

#[tonic::async_trait]
impl HcodeArtifactPeerService for Peer {
    async fn read_chunk(
        &self,
        request: Request<HcodePeerReadRequest>,
    ) -> Result<Response<HcodeArtifactChunkDownloadResponse>, Status> {
        let request = request.into_inner();
        self.seen_tokens.lock().unwrap().push(request.token.clone());
        if self.behavior == Behavior::Refuse {
            return Err(Status::permission_denied("refused"));
        }
        // The fake lease token ends with the chunk offset, which keeps the peer simple.
        let offset: usize = request
            .token
            .rsplit(':')
            .next()
            .and_then(|value| value.parse().ok())
            .ok_or_else(|| Status::invalid_argument("token has no offset"))?;
        let end = (offset + CHUNK).min(self.data.len());
        let mut bytes = self.data[offset..end].to_vec();
        match self.behavior {
            Behavior::Corrupt => bytes[0] ^= 0xff,
            Behavior::Short => {
                bytes.pop();
            }
            Behavior::Honest | Behavior::Refuse => {}
        }
        Ok(Response::new(HcodeArtifactChunkDownloadResponse { bytes }))
    }

    async fn probe_chunk(
        &self,
        _request: Request<HcodePeerProbeRequest>,
    ) -> Result<Response<HcodeArtifactChunkDownloadResponse>, Status> {
        Err(Status::unimplemented(
            "probe is not part of the download path",
        ))
    }
}

async fn serve(peer: Peer) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(HcodeArtifactPeerServiceServer::new(peer))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .unwrap();
    });
    address
}

struct SourceSpec {
    id: String,
    overlay: String,
    tamper: Tamper,
}

/// Nodepool stand-in. It returns metadata and leases only, never bytes.
struct Control {
    manifest: ArtifactManifest,
    sources: Vec<SourceSpec>,
    list_error: Option<String>,
    lists: AtomicUsize,
    leases: AtomicUsize,
    seen_by_control: Mutex<Vec<String>>,
}

impl Control {
    fn availability(&self, spec: &SourceSpec, task_id: &str, now: i64) -> HcodeSourceAvailability {
        let artifact_sha256 = if spec.tamper == Tamper::WrongArtifactDigest {
            format!("sha256:{}", "1".repeat(64))
        } else {
            self.manifest.sha256.clone()
        };
        HcodeSourceAvailability {
            source: Some(HcodeSourceRegistration {
                source_id: spec.id.clone(),
                registration_id: "registration-1".into(),
                task_id: task_id.into(),
                artifact_id: self.manifest.artifact_id.clone(),
                artifact_sha256,
                artifact_size_bytes: self.manifest.size_bytes as i64,
                overlay_ipv4: spec.overlay.clone(),
                port: if spec.tamper == Tamper::WrongRegistrationPort {
                    9999
                } else {
                    HCODE_SOURCE_PORT
                },
                expires_at_unix_ms: if spec.tamper == Tamper::ExpiredRegistration {
                    now - 1
                } else {
                    now + 60_000
                },
                state: "registered_pending".into(),
            }),
            checked_at_unix_ms: now,
            available_until_unix_ms: if spec.tamper == Tamper::StaleObservation {
                now - 1
            } else {
                now + 30_000
            },
            sampled_offset: 0,
        }
    }
}

#[tonic::async_trait]
impl P2pControlPlane for Control {
    async fn list_sources(
        &self,
        request: HcodeArtifactChunkDownload,
    ) -> Result<Vec<HcodeSourceAvailability>, String> {
        self.lists.fetch_add(1, Ordering::SeqCst);
        self.seen_by_control
            .lock()
            .unwrap()
            .push(request.token.clone());
        if let Some(error) = &self.list_error {
            return Err(error.clone());
        }
        let now = Utc::now().timestamp_millis();
        Ok(self
            .sources
            .iter()
            .map(|spec| self.availability(spec, &request.task_id, now))
            .collect())
    }

    async fn issue_lease(
        &self,
        request: IssueHcodeReadLeaseRequest,
    ) -> Result<HcodeArtifactReadLease, String> {
        self.leases.fetch_add(1, Ordering::SeqCst);
        let chunk = request.chunk.ok_or_else(|| "missing chunk".to_owned())?;
        self.seen_by_control
            .lock()
            .unwrap()
            .push(chunk.token.clone());
        let spec = self
            .sources
            .iter()
            .find(|spec| spec.id == request.source_id)
            .ok_or_else(|| "unknown source".to_owned())?;
        if spec.tamper == Tamper::LeaseDenied {
            return Err("lease denied".into());
        }
        let offset = chunk.offset as u64;
        let meta = self
            .manifest
            .chunks
            .iter()
            .find(|meta| meta.offset == offset)
            .ok_or_else(|| "unknown offset".to_owned())?;
        let now = Utc::now().timestamp_millis();
        let mut registration_id = "registration-1".to_owned();
        let mut chunk_sha256 = meta.sha256.clone();
        let mut port = HCODE_SOURCE_PORT;
        let mut overlay_ipv4 = spec.overlay.clone();
        let mut expires_at_unix_ms = now + 60_000;
        match spec.tamper {
            Tamper::LeaseWrongRegistration => registration_id = "registration-0".into(),
            Tamper::LeaseWrongChunkDigest => chunk_sha256 = format!("sha256:{}", "2".repeat(64)),
            Tamper::LeaseWrongPort => port = 9999,
            Tamper::LeaseWrongOverlay => overlay_ipv4 = "100.64.9.9".into(),
            Tamper::LeaseExpired => expires_at_unix_ms = now - 1,
            _ => {}
        }
        Ok(HcodeArtifactReadLease {
            token: format!("lease:{}:{}", spec.id, offset),
            overlay_ipv4,
            port,
            authorization: Some(HcodePeerReadAuthorization {
                source_id: spec.id.clone(),
                registration_id,
                task_id: chunk.task_id,
                artifact_id: chunk.artifact_id,
                offset,
                chunk: Some(HcodeArtifactChunkAuthorization {
                    artifact_sha256: self.manifest.sha256.clone(),
                    artifact_size_bytes: self.manifest.size_bytes as i64,
                    chunk_sha256,
                    chunk_size_bytes: meta.size_bytes as i64,
                }),
                expires_at_unix_ms,
            }),
        })
    }
}

/// Routes each lease to the loopback address of its source. The overlay address
/// is ignored here because the production reader's address rules are tested directly.
struct Loopback {
    addresses: HashMap<String, String>,
}

#[tonic::async_trait]
impl P2pPeerReader for Loopback {
    async fn read_chunk(
        &self,
        _overlay_ipv4: &str,
        _port: u32,
        source_id: &str,
        read_token: &str,
    ) -> Result<Vec<u8>, String> {
        let address = self
            .addresses
            .get(source_id)
            .ok_or_else(|| "unknown source".to_owned())?;
        let channel = Endpoint::from_shared(format!("http://{address}"))
            .map_err(|error| error.to_string())?
            .connect()
            .await
            .map_err(|error| error.to_string())?;
        let mut client = HcodeArtifactPeerServiceClient::new(channel);
        let response = client
            .read_chunk(HcodePeerReadRequest {
                source_id: source_id.to_owned(),
                token: read_token.to_owned(),
            })
            .await
            .map_err(|status| status.message().to_owned())?;
        Ok(response.into_inner().bytes)
    }
}

struct Harness {
    control: Control,
    reader: Loopback,
    peers: Vec<(String, Arc<Mutex<Vec<String>>>)>,
    manifest: ArtifactManifest,
}

impl Harness {
    async fn new(data: &[u8], sources: &[(&str, Tamper, Behavior)]) -> Self {
        let shared = Arc::new(data.to_vec());
        let mut specs = Vec::new();
        let mut addresses = HashMap::new();
        let mut peers = Vec::new();
        for (index, (id, tamper, behavior)) in sources.iter().enumerate() {
            let seen_tokens = Arc::new(Mutex::new(Vec::new()));
            let address = serve(Peer {
                data: shared.clone(),
                behavior: *behavior,
                seen_tokens: seen_tokens.clone(),
            })
            .await;
            addresses.insert((*id).to_owned(), address);
            specs.push(SourceSpec {
                id: (*id).to_owned(),
                overlay: format!("100.64.0.{}", index + 1),
                tamper: *tamper,
            });
            peers.push(((*id).to_owned(), seen_tokens));
        }
        let manifest = manifest(data);
        Self {
            control: Control {
                manifest: manifest.clone(),
                sources: specs,
                list_error: None,
                lists: AtomicUsize::new(0),
                leases: AtomicUsize::new(0),
                seen_by_control: Mutex::new(Vec::new()),
            },
            reader: Loopback { addresses },
            peers,
            manifest,
        }
    }

    async fn download(&self, manifest: &ArtifactManifest) -> Result<Vec<u8>, P2pDownloadError> {
        download_artifact(
            &self.control,
            &self.reader,
            EXECUTION_TOKEN,
            TASK_ID,
            manifest,
        )
        .await
    }

    fn peer_tokens(&self, id: &str) -> Vec<String> {
        self.peers
            .iter()
            .find(|(peer, _)| peer == id)
            .map(|(_, seen)| seen.lock().unwrap().clone())
            .unwrap_or_default()
    }

    fn total_peer_requests(&self) -> usize {
        self.peers
            .iter()
            .map(|(_, seen)| seen.lock().unwrap().len())
            .sum()
    }
}

#[tokio::test]
async fn honest_source_returns_the_exact_artifact_and_never_sees_the_execution_token() {
    let data = payload();
    let h = Harness::new(&data, &[("source-a", Tamper::None, Behavior::Honest)]).await;
    let manifest = h.manifest.clone();

    assert_eq!(h.download(&manifest).await.unwrap(), data);
    assert_eq!(h.control.lists.load(Ordering::SeqCst), 3);
    assert_eq!(h.control.leases.load(Ordering::SeqCst), 3);
    let peer_tokens = h.peer_tokens("source-a");
    assert_eq!(peer_tokens.len(), 3);
    assert!(peer_tokens.iter().all(|token| token.starts_with("lease:")));
    assert!(h
        .control
        .seen_by_control
        .lock()
        .unwrap()
        .iter()
        .all(|token| token == EXECUTION_TOKEN));
}

#[tokio::test]
async fn corrupt_source_is_rejected_and_an_authorized_alternative_serves_the_chunk() {
    let data = payload();
    let h = Harness::new(
        &data,
        &[
            ("bad", Tamper::None, Behavior::Corrupt),
            ("good", Tamper::None, Behavior::Honest),
        ],
    )
    .await;
    let manifest = h.manifest.clone();

    assert_eq!(h.download(&manifest).await.unwrap(), data);
    assert_eq!(h.peer_tokens("bad").len(), 3);
    assert_eq!(h.peer_tokens("good").len(), 3);
}

#[tokio::test]
async fn short_or_refused_chunk_fails_without_any_byte_fallback() {
    let data = payload();
    for behavior in [Behavior::Short, Behavior::Refuse] {
        let h = Harness::new(&data, &[("only", Tamper::None, behavior)]).await;
        let manifest = h.manifest.clone();

        assert_eq!(
            h.download(&manifest).await,
            Err(P2pDownloadError::SourcesExhausted {
                offset: 0,
                attempted: 1
            }),
            "{behavior:?}"
        );
        assert_eq!(h.peer_tokens("only").len(), 1, "{behavior:?}");
    }
}

#[tokio::test]
async fn discovery_filters_stale_foreign_and_expired_observations_before_any_lease() {
    let data = payload();
    for tamper in [
        Tamper::StaleObservation,
        Tamper::WrongArtifactDigest,
        Tamper::WrongRegistrationPort,
        Tamper::ExpiredRegistration,
    ] {
        let h = Harness::new(&data, &[("source", tamper, Behavior::Honest)]).await;
        let manifest = h.manifest.clone();

        assert_eq!(
            h.download(&manifest).await,
            Err(P2pDownloadError::NoLiveSource { offset: 0 }),
            "{tamper:?}"
        );
        assert_eq!(h.control.leases.load(Ordering::SeqCst), 0, "{tamper:?}");
        assert_eq!(h.total_peer_requests(), 0, "{tamper:?}");
    }
}

#[tokio::test]
async fn every_lease_field_must_bind_the_registration_task_chunk_and_port() {
    let data = payload();
    for tamper in [
        Tamper::LeaseDenied,
        Tamper::LeaseWrongRegistration,
        Tamper::LeaseWrongChunkDigest,
        Tamper::LeaseWrongPort,
        Tamper::LeaseWrongOverlay,
        Tamper::LeaseExpired,
    ] {
        let h = Harness::new(&data, &[("source", tamper, Behavior::Honest)]).await;
        let manifest = h.manifest.clone();

        assert_eq!(
            h.download(&manifest).await,
            Err(P2pDownloadError::SourcesExhausted {
                offset: 0,
                attempted: 1
            }),
            "{tamper:?}"
        );
        assert_eq!(h.total_peer_requests(), 0, "{tamper:?}");
    }
}

#[tokio::test]
async fn a_denied_lease_moves_to_the_next_authorized_source() {
    let data = payload();
    let h = Harness::new(
        &data,
        &[
            ("denied", Tamper::LeaseDenied, Behavior::Honest),
            ("good", Tamper::None, Behavior::Honest),
        ],
    )
    .await;
    let manifest = h.manifest.clone();

    assert_eq!(h.download(&manifest).await.unwrap(), data);
    assert!(h.peer_tokens("denied").is_empty());
    assert_eq!(h.peer_tokens("good").len(), 3);
}

#[tokio::test]
async fn at_most_eight_sources_are_tried_for_one_chunk() {
    let data = payload();
    let mut sources: Vec<(&str, Tamper, Behavior)> = SOURCE_IDS[..8]
        .iter()
        .map(|id| (*id, Tamper::None, Behavior::Corrupt))
        .collect();
    sources.push((SOURCE_IDS[8], Tamper::None, Behavior::Honest));
    let h = Harness::new(&data, &sources).await;
    let manifest = h.manifest.clone();

    assert_eq!(
        h.download(&manifest).await,
        Err(P2pDownloadError::SourcesExhausted {
            offset: 0,
            attempted: 8
        })
    );
    assert!(h.peer_tokens(SOURCE_IDS[8]).is_empty());
}

#[tokio::test]
async fn complete_artifact_digest_is_checked_after_every_chunk_matches() {
    let data = payload();
    let mut h = Harness::new(&data, &[("source", Tamper::None, Behavior::Honest)]).await;
    // Nodepool's authorization and the manifest agree on the wrong whole-file
    // digest, so only the final assembled-artifact check can reject it.
    h.manifest.sha256 = format!("sha256:{}", "3".repeat(64));
    h.control.manifest.sha256 = h.manifest.sha256.clone();
    let manifest = h.manifest.clone();

    assert_eq!(
        h.download(&manifest).await,
        Err(P2pDownloadError::ArtifactDigestMismatch)
    );
    assert_eq!(h.peer_tokens("source").len(), 3);
}

#[tokio::test]
async fn invalid_chunk_layout_fails_before_any_control_call() {
    let data = payload();
    let h = Harness::new(&data, &[("source", Tamper::None, Behavior::Honest)]).await;
    let mut manifest = h.manifest.clone();
    manifest.chunks[1].offset += 1;

    assert_eq!(
        h.download(&manifest).await,
        Err(P2pDownloadError::InvalidManifest("chunk layout"))
    );
    assert_eq!(h.control.lists.load(Ordering::SeqCst), 0);
    assert_eq!(h.total_peer_requests(), 0);
}

#[tokio::test]
async fn control_plane_failure_is_reported_and_no_bytes_are_requested() {
    let data = payload();
    let mut h = Harness::new(&data, &[("source", Tamper::None, Behavior::Honest)]).await;
    h.control.list_error = Some("UNAVAILABLE".into());
    let manifest = h.manifest.clone();

    assert_eq!(
        h.download(&manifest).await,
        Err(P2pDownloadError::ControlPlane("UNAVAILABLE".into()))
    );
    assert_eq!(h.total_peer_requests(), 0);
}

#[test]
fn production_reader_dials_only_canonical_overlay_addresses_on_the_fixed_port() {
    assert_eq!(
        overlay_address("100.64.0.1", HCODE_SOURCE_PORT).unwrap(),
        "http://100.64.0.1:50054"
    );
    assert_eq!(
        overlay_address("100.127.255.254", HCODE_SOURCE_PORT).unwrap(),
        "http://100.127.255.254:50054"
    );
    for (address, port) in [
        ("127.0.0.1", HCODE_SOURCE_PORT),
        ("100.128.0.1", HCODE_SOURCE_PORT),
        ("100.63.255.255", HCODE_SOURCE_PORT),
        ("100.64.0.01", HCODE_SOURCE_PORT),
        ("100.64.0.1", 50055),
        ("100.64.0.1", 0),
        ("example.test", HCODE_SOURCE_PORT),
    ] {
        assert!(overlay_address(address, port).is_err(), "{address}:{port}");
    }
}

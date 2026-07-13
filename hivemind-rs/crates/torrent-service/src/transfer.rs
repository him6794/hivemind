//! Minimal BitTorrent-compatible piece transfer for Hivemind task packages.
//!
//! This is intentionally small: enough for nodepool to seed a single-file
//! package and for workers to fetch it after tracker announce. It is not a
//! full public BT client.

use crate::metainfo::{self, DEFAULT_PIECE_SIZE};
use crate::tracker::{PeerEntry, Tracker};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;
use tracing::{info, warn};

const PROTOCOL: &str = "BitTorrent protocol";
const HANDSHAKE_LEN: usize = 68;

#[derive(Debug, Clone)]
pub struct SeededTorrent {
    pub info_hash: String,
    pub info_hash_bytes: [u8; 20],
    pub name: String,
    pub data: Arc<Vec<u8>>,
    pub piece_hashes: Vec<[u8; 20]>,
    pub piece_length: usize,
    pub seed_path: PathBuf,
    pub torrent_path: PathBuf,
    pub announce_url: String,
    pub magnet_uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WireRequest {
    info_hash: String,
    piece_index: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WireResponse {
    ok: bool,
    message: String,
    piece_index: u32,
    data: Vec<u8>,
    #[serde(default)]
    total_length: u64,
    #[serde(default)]
    piece_count: u32,
}

#[derive(Clone)]
pub struct SeedStore {
    torrents: Arc<RwLock<HashMap<String, SeededTorrent>>>,
}

impl SeedStore {
    pub fn new() -> Self {
        Self {
            torrents: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn insert(&self, torrent: SeededTorrent) {
        self.torrents
            .write()
            .await
            .insert(torrent.info_hash.to_ascii_lowercase(), torrent);
    }

    pub async fn get(&self, info_hash: &str) -> Option<SeededTorrent> {
        self.torrents
            .read()
            .await
            .get(&info_hash.to_ascii_lowercase())
            .cloned()
    }
}

impl Default for SeedStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Create metainfo, store package bytes, and register the package as seedable.
pub async fn create_and_store_seed(
    store: &SeedStore,
    package_data: &[u8],
    package_path: &Path,
    announce_url: &str,
    api_dir: &Path,
    bt_dir: &Path,
    magnet_uri: String,
) -> Result<SeededTorrent> {
    std::fs::create_dir_all(api_dir)?;
    std::fs::create_dir_all(bt_dir)?;

    let meta = metainfo::create_metainfo(package_data, package_path, announce_url)?;
    let file_name = package_path
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("task.zip"));
    let seed_path = api_dir.join(file_name);
    std::fs::write(&seed_path, package_data)
        .with_context(|| format!("failed to write seed package {}", seed_path.display()))?;

    let torrent_path = bt_dir.join(format!("{}.torrent", meta.info_hash));
    let torrent_bytes = bendy::serde::to_bytes(&meta.metainfo)
        .map_err(|e| anyhow::anyhow!("Failed to serialize torrent: {}", e))?;
    std::fs::write(&torrent_path, &torrent_bytes)
        .with_context(|| format!("failed to write torrent file {}", torrent_path.display()))?;

    let seeded = SeededTorrent {
        info_hash: meta.info_hash.clone(),
        info_hash_bytes: meta.info_hash_bytes,
        name: meta.metainfo.info.name.clone(),
        data: Arc::new(package_data.to_vec()),
        piece_hashes: meta.piece_hashes,
        piece_length: meta.piece_size as usize,
        seed_path,
        torrent_path,
        announce_url: announce_url.to_string(),
        magnet_uri,
    };
    store.insert(seeded.clone()).await;
    Ok(seeded)
}

/// Start the nodepool BT seed listener.
pub async fn start_seed_listener(
    listen_addr: SocketAddr,
    store: SeedStore,
) -> Result<tokio::task::JoinHandle<()>> {
    let listener = TcpListener::bind(listen_addr)
        .await
        .with_context(|| format!("failed to bind torrent seed listener on {listen_addr}"))?;
    info!("Torrent seed listener started on {}", listen_addr);
    Ok(tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, peer)) => {
                    let store = store.clone();
                    tokio::spawn(async move {
                        if let Err(err) = handle_seed_connection(stream, store).await {
                            warn!("seed connection from {} failed: {:#}", peer, err);
                        }
                    });
                }
                Err(err) => {
                    warn!("torrent seed accept failed: {}", err);
                }
            }
        }
    }))
}

async fn handle_seed_connection(mut stream: TcpStream, store: SeedStore) -> Result<()> {
    let mut handshake = [0u8; HANDSHAKE_LEN];
    stream.read_exact(&mut handshake).await?;
    let info_hash = parse_handshake_info_hash(&handshake)?;
    let torrent = store
        .get(&info_hash)
        .await
        .ok_or_else(|| anyhow::anyhow!("unknown info_hash {info_hash}"))?;

    // Respond with the same info-hash and a fixed seeder peer id.
    let mut response = handshake;
    response[48..68].copy_from_slice(b"-HM0001-nodepoolseed"); // 20 bytes
    stream.write_all(&response).await?;

    loop {
        let mut len_buf = [0u8; 4];
        if let Err(err) = stream.read_exact(&mut len_buf).await {
            if matches!(
                err.kind(),
                std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::ConnectionReset
            ) {
                break;
            }
            return Err(err.into());
        }
        let len = u32::from_be_bytes(len_buf) as usize;
        if len == 0 || len > 64 * 1024 {
            anyhow::bail!("invalid piece request length {len}");
        }
        let mut payload = vec![0u8; len];
        stream.read_exact(&mut payload).await?;
        let request: WireRequest = serde_json::from_slice(&payload)
            .map_err(|e| anyhow::anyhow!("invalid piece request payload: {e}"))?;
        if !request.info_hash.eq_ignore_ascii_case(&torrent.info_hash) {
            let response = WireResponse {
                ok: false,
                message: "info_hash mismatch".into(),
                piece_index: request.piece_index,
                data: Vec::new(),
                total_length: torrent.data.len() as u64,
                piece_count: torrent.piece_hashes.len() as u32,
            };
            write_framed_json(&mut stream, &response).await?;
            continue;
        }

        let piece_index = request.piece_index as usize;
        let response = match read_piece(&torrent, piece_index) {
            Ok(data) => WireResponse {
                ok: true,
                message: String::new(),
                piece_index: request.piece_index,
                data,
                total_length: torrent.data.len() as u64,
                piece_count: torrent.piece_hashes.len() as u32,
            },
            Err(err) => WireResponse {
                ok: false,
                message: err.to_string(),
                piece_index: request.piece_index,
                data: Vec::new(),
                total_length: torrent.data.len() as u64,
                piece_count: torrent.piece_hashes.len() as u32,
            },
        };
        write_framed_json(&mut stream, &response).await?;
        if !response.ok {
            break;
        }
        // Stay open until the client closes. Closing early races the client on
        // Windows and aborts the final framed response (os error 10053).
    }
    let _ = stream.shutdown().await;
    Ok(())
}

fn read_piece(torrent: &SeededTorrent, piece_index: usize) -> Result<Vec<u8>> {
    if piece_index >= torrent.piece_hashes.len() {
        anyhow::bail!("piece index {piece_index} out of range");
    }
    let start = piece_index * torrent.piece_length;
    if start >= torrent.data.len() && !(torrent.data.is_empty() && piece_index == 0) {
        anyhow::bail!("piece start {start} out of range");
    }
    let end = (start + torrent.piece_length).min(torrent.data.len());
    let piece = if torrent.data.is_empty() {
        Vec::new()
    } else {
        torrent.data[start..end].to_vec()
    };
    let expected = &torrent.piece_hashes[piece_index];
    if !metainfo::verify_piece(&piece, expected) {
        anyhow::bail!("seed piece {piece_index} failed local hash verification");
    }
    Ok(piece)
}

async fn write_framed_json<T: Serialize>(stream: &mut TcpStream, value: &T) -> Result<()> {
    let payload = serde_json::to_vec(value)?;
    let len = (payload.len() as u32).to_be_bytes();
    stream.write_all(&len).await?;
    stream.write_all(&payload).await?;
    stream.flush().await?;
    Ok(())
}

fn parse_handshake_info_hash(handshake: &[u8]) -> Result<String> {
    if handshake.len() != HANDSHAKE_LEN {
        anyhow::bail!("invalid handshake length");
    }
    if handshake[0] as usize != PROTOCOL.len() || &handshake[1..20] != PROTOCOL.as_bytes() {
        anyhow::bail!("unsupported BT handshake protocol");
    }
    Ok(hex::encode(&handshake[28..48]))
}

fn build_handshake(info_hash_bytes: &[u8; 20], peer_id: &[u8; 20]) -> [u8; HANDSHAKE_LEN] {
    let mut out = [0u8; HANDSHAKE_LEN];
    out[0] = PROTOCOL.len() as u8;
    out[1..20].copy_from_slice(PROTOCOL.as_bytes());
    out[28..48].copy_from_slice(info_hash_bytes);
    out[48..68].copy_from_slice(peer_id);
    out
}

/// Download a single-file torrent package from one or more seed peers.
pub async fn download_from_peers(
    info_hash: &str,
    peers: &[PeerEntry],
    destination: &Path,
    expected_name: Option<&str>,
    max_bytes: u64,
) -> Result<PathBuf> {
    if peers.is_empty() {
        anyhow::bail!("no torrent peers available for info_hash {info_hash}");
    }

    let mut last_error = None;
    for peer in peers {
        match download_from_peer(info_hash, peer, destination, expected_name, max_bytes).await {
            Ok(path) => return Ok(path),
            Err(err) => {
                warn!(
                    "failed downloading {} from {}:{}: {:#}",
                    info_hash, peer.ip, peer.port, err
                );
                last_error = Some(err);
            }
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("no usable torrent peers")))
}

async fn download_from_peer(
    info_hash: &str,
    peer: &PeerEntry,
    destination: &Path,
    expected_name: Option<&str>,
    max_bytes: u64,
) -> Result<PathBuf> {
    let addr = format!("{}:{}", peer.ip, peer.port);
    let mut stream = TcpStream::connect(&addr)
        .await
        .with_context(|| format!("failed to connect torrent peer {addr}"))?;

    let info_hash_bytes = hex_to_20(info_hash)?;
    let peer_id = *b"-HM0001-workerdownld"; // exactly 20 bytes
    let handshake = build_handshake(&info_hash_bytes, &peer_id);
    stream.write_all(&handshake).await?;

    let mut response = [0u8; HANDSHAKE_LEN];
    stream.read_exact(&mut response).await?;
    let remote_hash = parse_handshake_info_hash(&response)?;
    if !remote_hash.eq_ignore_ascii_case(info_hash) {
        anyhow::bail!(
            "peer returned mismatched info_hash {} (expected {})",
            remote_hash,
            info_hash
        );
    }

    // Minimal bootstrap request: ask for piece 0, then continue.
    // Prefer explicit piece_count/total_length from the seeder when present.
    let mut assembled = Vec::new();
    let mut piece_index = 0u32;
    let mut expected_total: Option<u64> = None;
    let mut expected_pieces: Option<u32> = None;
    loop {
        let request = WireRequest {
            info_hash: info_hash.to_string(),
            piece_index,
        };
        write_framed_json(&mut stream, &request).await?;

        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf).await?;
        let len = u32::from_be_bytes(len_buf) as usize;
        if len == 0 || len > 16 * 1024 * 1024 {
            anyhow::bail!("invalid piece response length {len}");
        }
        let mut payload = vec![0u8; len];
        stream.read_exact(&mut payload).await?;
        let response: WireResponse = serde_json::from_slice(&payload)
            .map_err(|e| anyhow::anyhow!("invalid piece response payload: {e}"))?;
        if !response.ok {
            anyhow::bail!("peer rejected piece {}: {}", piece_index, response.message);
        }
        if response.piece_index != piece_index {
            anyhow::bail!(
                "peer returned piece {} while requesting {}",
                response.piece_index,
                piece_index
            );
        }
        if response.total_length > 0 {
            expected_total = Some(response.total_length);
        }
        if response.piece_count > 0 {
            expected_pieces = Some(response.piece_count);
        }
        if assembled.len() as u64 + response.data.len() as u64 > max_bytes {
            anyhow::bail!(
                "torrent download exceeds task storage limit of {} bytes",
                max_bytes
            );
        }
        assembled.extend_from_slice(&response.data);

        let done_by_count = expected_pieces
            .map(|count| piece_index + 1 >= count)
            .unwrap_or(false);
        let done_by_length = expected_total
            .map(|total| assembled.len() as u64 >= total)
            .unwrap_or(false);
        // Metainfo always uses DEFAULT_PIECE_SIZE, so only the final piece is short.
        let done_by_short_piece =
            response.data.len() < DEFAULT_PIECE_SIZE || response.data.is_empty();
        if done_by_count || done_by_length || done_by_short_piece {
            break;
        }
        piece_index += 1;
        // Hard stop to avoid infinite loops on buggy peers.
        if piece_index > 4096 {
            anyhow::bail!("torrent download exceeded piece budget");
        }
    }
    if let Some(total) = expected_total {
        if assembled.len() as u64 != total {
            anyhow::bail!(
                "downloaded {} bytes but peer advertised total_length {}",
                assembled.len(),
                total
            );
        }
    }

    // Verify reconstructed package against the same metainfo algorithm used by
    // nodepool. Name is taken from magnet display name when present.
    let file_name = expected_name
        .map(|name| name.to_string())
        .unwrap_or_else(|| format!("{info_hash}.zip"));
    let temp_name = Path::new(&file_name)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("task.zip");
    let meta = metainfo::create_metainfo(&assembled, Path::new(temp_name), "")?;
    if !meta.info_hash.eq_ignore_ascii_case(info_hash) {
        anyhow::bail!(
            "downloaded package info hash {} does not match magnet BTIH {}",
            meta.info_hash,
            info_hash
        );
    }

    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(destination, &assembled).with_context(|| {
        format!(
            "failed to write downloaded package {}",
            destination.display()
        )
    })?;
    Ok(destination.to_path_buf())
}

fn hex_to_20(value: &str) -> Result<[u8; 20]> {
    let bytes = hex::decode(value).map_err(|e| anyhow::anyhow!("invalid info_hash hex: {e}"))?;
    if bytes.len() != 20 {
        anyhow::bail!("info_hash must be 20 bytes, got {}", bytes.len());
    }
    let mut out = [0u8; 20];
    out.copy_from_slice(&bytes);
    Ok(out)
}

/// HTTP tracker announce endpoint helpers.
pub async fn handle_tracker_announce(
    tracker: &Tracker,
    query: &str,
    remote_ip: Option<&str>,
) -> Result<Vec<u8>> {
    let params = parse_query(query);
    let info_hash = params
        .get("info_hash")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("missing info_hash"))?;
    let peer_id = params
        .get("peer_id")
        .cloned()
        .unwrap_or_else(|| "anonymous".into());
    let port = params
        .get("port")
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(6881);
    let uploaded = params
        .get("uploaded")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let downloaded = params
        .get("downloaded")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let left = params
        .get("left")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let ip = params
        .get("ip")
        .cloned()
        .or_else(|| remote_ip.map(ToString::to_string))
        .unwrap_or_else(|| "127.0.0.1".into());

    let peers = tracker
        .announce(
            &info_hash,
            PeerEntry {
                peer_id,
                ip,
                port,
                uploaded,
                downloaded,
                left,
                last_announce: 0,
            },
        )
        .await
        .map_err(|e| anyhow::anyhow!(e))?;

    // Compact-ish JSON response is enough for Hivemind workers.
    let body = serde_json::json!({
        "interval": 60,
        "peers": peers.iter().map(|peer| serde_json::json!({
            "peer_id": peer.peer_id,
            "ip": peer.ip,
            "port": peer.port,
        })).collect::<Vec<_>>(),
    });
    Ok(serde_json::to_vec(&body)?)
}

fn parse_query(query: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for part in query.split('&') {
        if part.is_empty() {
            continue;
        }
        let mut split = part.splitn(2, '=');
        let key = split.next().unwrap_or_default();
        let value = split.next().unwrap_or_default();
        out.insert(percent_decode(key), percent_decode(value));
    }
    out
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex_nibble(bytes[i + 1]), hex_nibble(bytes[i + 2])) {
                decoded.push((high << 4) | low);
                i += 3;
                continue;
            }
        } else if bytes[i] == b'+' {
            decoded.push(b' ');
            i += 1;
            continue;
        }
        decoded.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&decoded).to_string()
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Start a tiny HTTP tracker on the given address.
pub async fn start_http_tracker(
    listen_addr: SocketAddr,
    tracker: Arc<Tracker>,
) -> Result<tokio::task::JoinHandle<()>> {
    let listener = TcpListener::bind(listen_addr)
        .await
        .with_context(|| format!("failed to bind torrent tracker on {listen_addr}"))?;
    info!("Torrent tracker started on {}", listen_addr);
    Ok(tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, peer)) => {
                    let tracker = tracker.clone();
                    tokio::spawn(async move {
                        if let Err(err) =
                            handle_http_tracker_connection(stream, tracker, peer).await
                        {
                            warn!("tracker connection from {} failed: {:#}", peer, err);
                        }
                    });
                }
                Err(err) => warn!("tracker accept failed: {}", err),
            }
        }
    }))
}

async fn handle_http_tracker_connection(
    mut stream: TcpStream,
    tracker: Arc<Tracker>,
    peer: SocketAddr,
) -> Result<()> {
    let mut buf = vec![0u8; 8192];
    let n = stream.read(&mut buf).await?;
    if n == 0 {
        return Ok(());
    }
    let request = String::from_utf8_lossy(&buf[..n]);
    let first_line = request.lines().next().unwrap_or_default();
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    if method != "GET" {
        write_http_response(&mut stream, 405, b"method not allowed").await?;
        return Ok(());
    }
    let (path, query) = match target.split_once('?') {
        Some((path, query)) => (path, query),
        None => (target, ""),
    };
    if path != "/announce" {
        write_http_response(&mut stream, 404, b"not found").await?;
        return Ok(());
    }
    match handle_tracker_announce(&tracker, query, Some(&peer.ip().to_string())).await {
        Ok(body) => write_http_response(&mut stream, 200, &body).await?,
        Err(err) => {
            let body = serde_json::json!({"failure reason": err.to_string()});
            write_http_response(&mut stream, 400, &serde_json::to_vec(&body)?).await?;
        }
    }
    Ok(())
}

async fn write_http_response(stream: &mut TcpStream, status: u16, body: &[u8]) -> Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Error",
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes()).await?;
    stream.write_all(body).await?;
    Ok(())
}

/// Worker-side announce helper against nodepool HTTP tracker.
pub async fn announce_to_tracker(
    announce_url: &str,
    info_hash: &str,
    peer_id: &str,
    port: u16,
    left: u64,
) -> Result<Vec<PeerEntry>> {
    let url = if announce_url.contains('?') {
        format!(
            "{announce_url}&info_hash={}&peer_id={}&port={port}&uploaded=0&downloaded=0&left={left}&compact=0",
            urlencoding(info_hash),
            urlencoding(peer_id)
        )
    } else {
        format!(
            "{announce_url}?info_hash={}&peer_id={}&port={port}&uploaded=0&downloaded=0&left={left}&compact=0",
            urlencoding(info_hash),
            urlencoding(peer_id)
        )
    };
    let body = http_get_bytes(&url).await?;
    let value: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|e| anyhow::anyhow!("invalid tracker response: {e}"))?;
    if let Some(reason) = value.get("failure reason").and_then(|v| v.as_str()) {
        anyhow::bail!("tracker announce failed: {reason}");
    }
    let peers = value
        .get("peers")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    for peer in peers {
        let peer_id = peer
            .get("peer_id")
            .and_then(|v| v.as_str())
            .unwrap_or("peer")
            .to_string();
        let ip = peer
            .get("ip")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let port = peer.get("port").and_then(|v| v.as_u64()).unwrap_or(0) as u16;
        if ip.is_empty() || port == 0 {
            continue;
        }
        out.push(PeerEntry {
            peer_id,
            ip,
            port,
            uploaded: 0,
            downloaded: 0,
            left: 0,
            last_announce: 0,
        });
    }
    Ok(out)
}

async fn http_get_bytes(url: &str) -> Result<Vec<u8>> {
    let parsed = parse_http_url(url)?;
    let mut stream = TcpStream::connect((parsed.host.as_str(), parsed.port))
        .await
        .with_context(|| format!("failed to connect tracker {}", url))?;
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: hivemind-torrent-service\r\nConnection: close\r\n\r\n",
        parsed.path_and_query, parsed.host
    );
    stream.write_all(request.as_bytes()).await?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await?;
    let header_end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| anyhow::anyhow!("tracker response missing headers"))?;
    let body_start = header_end + 4;
    let headers = String::from_utf8_lossy(&response[..header_end]);
    let status = headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or(0);
    if !(200..300).contains(&status) {
        anyhow::bail!("tracker HTTP status {status}");
    }
    Ok(response[body_start..].to_vec())
}

struct ParsedHttpUrl {
    host: String,
    port: u16,
    path_and_query: String,
}

fn parse_http_url(url: &str) -> Result<ParsedHttpUrl> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| anyhow::anyhow!("only http:// tracker URLs are supported"))?;
    let (authority, path) = match rest.split_once('/') {
        Some((authority, path)) => (authority, format!("/{path}")),
        None => (rest, "/".to_string()),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() => {
            let port = port
                .parse::<u16>()
                .map_err(|_| anyhow::anyhow!("invalid tracker port"))?;
            (host.to_string(), port)
        }
        _ => (authority.to_string(), 80),
    };
    Ok(ParsedHttpUrl {
        host,
        port,
        path_and_query: path,
    })
}

fn urlencoding(value: &str) -> String {
    let mut out = String::with_capacity(value.len() * 3);
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tracker::Tracker;
    use tempfile::TempDir;

    #[tokio::test]
    async fn seed_and_download_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let api_dir = tmp.path().join("api");
        let bt_dir = tmp.path().join("bt");
        std::fs::create_dir_all(&api_dir).unwrap();
        std::fs::create_dir_all(&bt_dir).unwrap();

        let store = SeedStore::new();
        let package = b"hello-hivemind-bt-package-bytes";
        let package_path = Path::new("demo-task.zip");
        let tracker = Arc::new(Tracker::new(60));
        let tracker_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let tracker_addr = tracker_listener.local_addr().unwrap();
        drop(tracker_listener);
        let seed_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let seed_addr = seed_listener.local_addr().unwrap();
        drop(seed_listener);

        let announce = format!("http://{tracker_addr}/announce");
        let magnet = format!(
            "magnet:?xt=urn:btih:placeholder&dn=demo-task.zip&tr={}",
            urlencoding(&announce)
        );
        let seeded = create_and_store_seed(
            &store,
            package,
            package_path,
            &announce,
            &api_dir,
            &bt_dir,
            magnet,
        )
        .await
        .unwrap();

        let _tracker_handle = start_http_tracker(tracker_addr, tracker.clone())
            .await
            .unwrap();
        let _seed_handle = start_seed_listener(seed_addr, store.clone()).await.unwrap();

        tracker
            .announce(
                &seeded.info_hash,
                PeerEntry {
                    peer_id: "nodepool-seeder".into(),
                    ip: seed_addr.ip().to_string(),
                    port: seed_addr.port(),
                    uploaded: 0,
                    downloaded: 0,
                    left: 0,
                    last_announce: 0,
                },
            )
            .await
            .unwrap();

        let peers = announce_to_tracker(
            &announce,
            &seeded.info_hash,
            "worker-test",
            0,
            package.len() as u64,
        )
        .await
        .unwrap();
        assert!(!peers.is_empty());

        let destination = tmp.path().join("downloads").join("demo-task.zip");
        let path = download_from_peers(
            &seeded.info_hash,
            &peers,
            &destination,
            Some("demo-task.zip"),
            1024 * 1024,
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read(path).unwrap(), package);
    }

    #[tokio::test]
    async fn seed_and_download_multi_piece_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let api_dir = tmp.path().join("api");
        let bt_dir = tmp.path().join("bt");
        std::fs::create_dir_all(&api_dir).unwrap();
        std::fs::create_dir_all(&bt_dir).unwrap();

        let store = SeedStore::new();
        // Larger than DEFAULT_PIECE_SIZE so the transfer spans multiple pieces.
        let package = vec![0x5Au8; DEFAULT_PIECE_SIZE + 1234];
        let package_path = Path::new("multi-piece.zip");
        let seed_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let seed_addr = seed_listener.local_addr().unwrap();
        drop(seed_listener);

        let seeded = create_and_store_seed(
            &store,
            &package,
            package_path,
            "http://127.0.0.1:1/announce",
            &api_dir,
            &bt_dir,
            String::new(),
        )
        .await
        .unwrap();
        assert!(seeded.piece_hashes.len() >= 2);

        let _seed_handle = start_seed_listener(seed_addr, store.clone()).await.unwrap();
        let peers = vec![PeerEntry {
            peer_id: "nodepool-seeder".into(),
            ip: seed_addr.ip().to_string(),
            port: seed_addr.port(),
            uploaded: 0,
            downloaded: 0,
            left: 0,
            last_announce: 0,
        }];

        let destination = tmp.path().join("downloads").join("multi-piece.zip");
        let path = download_from_peers(
            &seeded.info_hash,
            &peers,
            &destination,
            Some("multi-piece.zip"),
            (package.len() as u64) + 1024,
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read(path).unwrap(), package);
    }
}

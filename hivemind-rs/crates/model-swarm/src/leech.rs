//! Downloading side of a private swarm.
//!
//! Each peer connection claims up to `concurrent_pieces_per_peer` pieces at a
//! time, pipelines block requests across them up to `pipeline_depth`, and
//! hashes each assembled piece before writing it. A piece that fails its hash
//! is released for another peer, and the peer that sent it is dropped. The
//! file is written to `<destination>.part` and renamed only after every piece
//! has verified.
//!
//! If `<destination>.part` already exists with the expected file length, each
//! piece on disk is verified against its SHA-1 digest before connecting to peers
//! so a resumed download skips already-verified pieces instead of truncating.

use std::collections::{HashMap, VecDeque};
use std::io::{self, SeekFrom};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use rand::seq::SliceRandom;
use rand::RngCore;
use sha1::{Digest, Sha1};
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::task::JoinSet;
use tokio::time::{sleep, timeout};

use crate::metainfo::Metainfo;
use crate::wire::{self, BlockRef, Handshake, Message, WireError, BLOCK_LEN};

const POLL_INTERVAL: Duration = Duration::from_millis(50);
const MAX_PEER_ERRORS: usize = 16;
const MAX_CONCURRENT_PIECES_PER_PEER: usize = 8;

/// Timeouts and limits for one download.
#[derive(Debug, Clone)]
pub struct DownloadOptions {
    pub peer_id: [u8; 20],
    pub connect_timeout: Duration,
    /// Longest wait for any single message.
    pub message_timeout: Duration,
    /// Overall deadline for the bitfield and the unchoke. A peer cannot hold
    /// the connection open by sending keep-alives alone.
    pub setup_timeout: Duration,
    /// Overall deadline for one piece, including its block requests.
    pub piece_timeout: Duration,
    /// How long a peer waits with nothing to claim before it disconnects.
    pub idle_wait: Duration,
    /// Block requests kept in flight per connection across active pieces.
    pub pipeline_depth: usize,
    /// Pieces claimed concurrently on a single peer connection (clamped to 1..=8).
    pub concurrent_pieces_per_peer: usize,
}

impl Default for DownloadOptions {
    fn default() -> Self {
        let mut peer_id = [0u8; 20];
        peer_id[..8].copy_from_slice(b"-HM0001-");
        rand::thread_rng().fill_bytes(&mut peer_id[8..]);
        Self {
            peer_id,
            connect_timeout: Duration::from_secs(5),
            message_timeout: Duration::from_secs(30),
            setup_timeout: Duration::from_secs(60),
            piece_timeout: Duration::from_secs(300),
            idle_wait: Duration::from_secs(3),
            pipeline_depth: 32,
            concurrent_pieces_per_peer: 2,
        }
    }
}

/// What a finished download reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadReport {
    pub pieces: usize,
    /// Pieces already verified in an existing `.part` file before peers were contacted.
    pub resumed_pieces: usize,
    pub bytes: u64,
    /// Pieces that failed their hash. Each one dropped the peer that sent it.
    pub rejected_pieces: usize,
    pub peers_connected: usize,
    /// First failures per peer, for diagnosis. Capped at 16 entries.
    pub peer_errors: Vec<String>,
}

/// Reasons a download does not finish.
#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("file error: {0}")]
    Io(#[from] io::Error),
    #[error("destination already exists")]
    DestinationExists,
    #[error("{missing} pieces are still missing after every peer disconnected")]
    Incomplete {
        missing: usize,
        peer_errors: Vec<String>,
    },
}

#[derive(Debug)]
struct State {
    done: Vec<bool>,
    claimed: Vec<bool>,
    remaining: usize,
    rejected: usize,
    connected: usize,
    peer_errors: Vec<String>,
}

struct Shared {
    metainfo: Metainfo,
    options: DownloadOptions,
    state: Mutex<State>,
    file: tokio::sync::Mutex<File>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

struct InFlightPiece {
    size: usize,
    blocks: usize,
    data: Vec<u8>,
    received: Vec<bool>,
    requested: usize,
    completed: usize,
    started_at: Instant,
}

impl InFlightPiece {
    fn new(size: usize, now: Instant) -> Self {
        let block = BLOCK_LEN as usize;
        let blocks = size.div_ceil(block);
        Self {
            size,
            blocks,
            data: vec![0u8; size],
            received: vec![false; blocks],
            requested: 0,
            completed: 0,
            started_at: now,
        }
    }

    fn next_block_request(&mut self, piece_index: usize) -> Option<BlockRef> {
        if self.requested >= self.blocks {
            return None;
        }
        let block = BLOCK_LEN as usize;
        let begin = self.requested * block;
        let length = block.min(self.size - begin);
        self.requested += 1;
        Some(BlockRef {
            index: piece_index as u32,
            begin: begin as u32,
            length: length as u32,
        })
    }

    fn record_block(&mut self, begin: u32, payload: &[u8]) -> Result<bool, WireError> {
        let block = BLOCK_LEN as usize;
        let begin = begin as usize;
        if !begin.is_multiple_of(block) || begin >= self.size {
            return Err(WireError::Malformed);
        }
        let slot = begin / block;
        let expected = block.min(self.size - begin);
        if payload.len() != expected || self.received[slot] {
            return Err(WireError::Malformed);
        }
        self.data[begin..begin + expected].copy_from_slice(payload);
        self.received[slot] = true;
        self.completed += 1;
        Ok(self.completed == self.blocks)
    }
}

/// Downloads the file described by `metainfo` from `peers` into `destination`.
///
/// Every piece is verified against the metainfo before it is written. If a
/// `.part` file already exists with matching length, verified pieces are
/// retained so interrupted transfers resume without re-downloading them.
/// The caller still checks the whole file against the catalog digest.
pub async fn download(
    metainfo: &Metainfo,
    peers: &[SocketAddr],
    destination: &Path,
    options: DownloadOptions,
) -> Result<DownloadReport, DownloadError> {
    if destination.exists() {
        return Err(DownloadError::DestinationExists);
    }
    let part = part_path(destination);
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&part)
        .await?;
    let count = metainfo.piece_count();
    let mut done = vec![false; count];
    let mut resumed_pieces = 0usize;

    let existing_len = file.metadata().await?.len();
    if existing_len == metainfo.length() && metainfo.length() > 0 {
        resumed_pieces = verify_existing_part(&mut file, metainfo, &mut done).await?;
    } else {
        file.set_len(0).await?;
        file.set_len(metainfo.length()).await?;
    }

    let remaining = count - resumed_pieces;
    if remaining == 0 {
        file.flush().await?;
        file.sync_all().await?;
        drop(file);
        tokio::fs::rename(&part, destination).await?;
        return Ok(DownloadReport {
            pieces: count,
            resumed_pieces,
            bytes: metainfo.length(),
            rejected_pieces: 0,
            peers_connected: 0,
            peer_errors: Vec::new(),
        });
    }

    let shared = Arc::new(Shared {
        metainfo: metainfo.clone(),
        options,
        state: Mutex::new(State {
            done,
            claimed: vec![false; count],
            remaining,
            rejected: 0,
            connected: 0,
            peer_errors: Vec::new(),
        }),
        file: tokio::sync::Mutex::new(file),
    });
    let mut tasks = JoinSet::new();
    for &addr in peers {
        let shared = Arc::clone(&shared);
        tasks.spawn(async move { run_peer(shared, addr).await });
    }
    while tasks.join_next().await.is_some() {}
    let shared =
        Arc::try_unwrap(shared).map_err(|_| io::Error::other("peer tasks are still running"))?;
    let state = shared
        .state
        .into_inner()
        .unwrap_or_else(PoisonError::into_inner);
    let mut file = shared.file.into_inner();
    if state.remaining > 0 {
        return Err(DownloadError::Incomplete {
            missing: state.remaining,
            peer_errors: state.peer_errors,
        });
    }
    file.flush().await?;
    file.sync_all().await?;
    drop(file);
    tokio::fs::rename(&part, destination).await?;
    Ok(DownloadReport {
        pieces: count,
        resumed_pieces,
        bytes: metainfo.length(),
        rejected_pieces: state.rejected,
        peers_connected: state.connected,
        peer_errors: state.peer_errors,
    })
}

/// Scans an existing `.part` file of matching length and marks pieces whose
/// SHA-1 digest already matches the metainfo. Unverified pieces stay `false`
/// and will be fetched and overwritten.
async fn verify_existing_part(
    file: &mut File,
    metainfo: &Metainfo,
    done: &mut [bool],
) -> io::Result<usize> {
    let mut verified = 0usize;
    let mut buffer = Vec::new();
    for (index, slot) in done.iter_mut().enumerate() {
        let Some(size) = metainfo.piece_size(index) else {
            break;
        };
        let Some(expected) = metainfo.piece_hash(index) else {
            break;
        };
        buffer.resize(size as usize, 0);
        file.seek(SeekFrom::Start(metainfo.piece_offset(index)))
            .await?;
        file.read_exact(&mut buffer).await?;
        let actual: [u8; 20] = Sha1::digest(&buffer).into();
        if &actual == expected {
            *slot = true;
            verified += 1;
        }
    }
    Ok(verified)
}

fn part_path(destination: &Path) -> PathBuf {
    let mut name = destination.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}

async fn run_peer(shared: Arc<Shared>, addr: SocketAddr) {
    if let Err(error) = peer_session(&shared, addr).await {
        let mut state = shared.lock();
        if state.peer_errors.len() < MAX_PEER_ERRORS {
            state.peer_errors.push(format!("{addr}: {error}"));
        }
    }
}

fn release_claimed(shared: &Shared, active: &HashMap<usize, InFlightPiece>) {
    if active.is_empty() {
        return;
    }
    let mut state = shared.lock();
    for &index in active.keys() {
        state.claimed[index] = false;
    }
}

async fn peer_session(shared: &Shared, addr: SocketAddr) -> Result<(), WireError> {
    let options = &shared.options;
    let info_hash = shared.metainfo.infohash();
    let mut stream = timeout(options.connect_timeout, TcpStream::connect(addr))
        .await
        .map_err(|_| WireError::TimedOut)??;
    stream.set_nodelay(true)?;
    let ours = Handshake {
        info_hash,
        peer_id: options.peer_id,
    };
    wire::write_handshake(&mut stream, &ours).await?;
    let theirs = timeout(options.message_timeout, wire::read_handshake(&mut stream))
        .await
        .map_err(|_| WireError::TimedOut)??;
    if theirs.info_hash != info_hash {
        return Err(WireError::BadHandshake);
    }
    shared.lock().connected += 1;

    let have = timeout(options.setup_timeout, await_setup(&mut stream, shared))
        .await
        .map_err(|_| WireError::TimedOut)??;

    let max_active = options
        .concurrent_pieces_per_peer
        .clamp(1, MAX_CONCURRENT_PIECES_PER_PEER);
    let pipeline_depth = options.pipeline_depth.max(1);
    let mut active: HashMap<usize, InFlightPiece> = HashMap::new();
    let mut order: VecDeque<usize> = VecDeque::new();
    let mut outstanding = 0usize;
    let mut idle = Duration::ZERO;

    let result: Result<(), WireError> = async {
        loop {
            while active.len() < max_active {
                let Some(index) = claim_piece(shared, &have) else {
                    break;
                };
                let size = shared
                    .metainfo
                    .piece_size(index)
                    .ok_or(WireError::Malformed)? as usize;
                active.insert(index, InFlightPiece::new(size, Instant::now()));
                order.push_back(index);
            }

            if active.is_empty() {
                if shared.lock().remaining == 0 {
                    return Ok(());
                }
                if idle >= options.idle_wait {
                    return Ok(());
                }
                sleep(POLL_INTERVAL).await;
                idle += POLL_INTERVAL;
                continue;
            }
            idle = Duration::ZERO;

            // Fill the per-connection block pipeline round-robin across claimed pieces.
            let mut progress = true;
            while outstanding < pipeline_depth && progress {
                progress = false;
                let count = order.len();
                for _ in 0..count {
                    if outstanding >= pipeline_depth {
                        break;
                    }
                    let Some(piece_idx) = order.pop_front() else {
                        break;
                    };
                    if let Some(piece) = active.get_mut(&piece_idx) {
                        if let Some(request) = piece.next_block_request(piece_idx) {
                            wire::write_message(&mut stream, &Message::Request(request)).await?;
                            outstanding += 1;
                            progress = true;
                        }
                        order.push_back(piece_idx);
                    }
                }
            }

            // Compute remaining wait bounded by both message_timeout and earliest piece_timeout.
            let now = Instant::now();
            let mut wait_limit = options.message_timeout;
            for piece in active.values() {
                let elapsed = now.saturating_duration_since(piece.started_at);
                if elapsed >= options.piece_timeout {
                    return Err(WireError::TimedOut);
                }
                wait_limit = wait_limit.min(options.piece_timeout - elapsed);
            }

            let message = timeout(wait_limit, wire::read_message(&mut stream))
                .await
                .map_err(|_| WireError::TimedOut)??;
            match message {
                Message::Piece {
                    index: got,
                    begin,
                    data: payload,
                } => {
                    let piece_idx = got as usize;
                    let Some(piece) = active.get_mut(&piece_idx) else {
                        return Err(WireError::Malformed);
                    };
                    let finished = piece.record_block(begin, &payload)?;
                    outstanding = outstanding.saturating_sub(1);
                    if finished {
                        let completed = active.remove(&piece_idx).ok_or(WireError::Malformed)?;
                        order.retain(|&idx| idx != piece_idx);
                        if let Err(error) = store_piece(shared, piece_idx, completed.data).await {
                            shared.lock().claimed[piece_idx] = false;
                            return Err(error);
                        }
                    }
                }
                Message::Choke => return Err(WireError::Choked),
                _ => {}
            }
        }
    }
    .await;

    if result.is_err() {
        release_claimed(shared, &active);
    }
    result
}

/// Reads the peer's bitfield, sends interest, and waits for an unchoke.
async fn await_setup(stream: &mut TcpStream, shared: &Shared) -> Result<Vec<bool>, WireError> {
    let options = &shared.options;
    let mut have = read_bitfield(
        stream,
        shared.metainfo.piece_count(),
        options.message_timeout,
    )
    .await?;
    wire::write_message(stream, &Message::Interested).await?;
    wait_for_unchoke(stream, &mut have, options.message_timeout).await?;
    Ok(have)
}

async fn read_bitfield(
    stream: &mut TcpStream,
    count: usize,
    limit: Duration,
) -> Result<Vec<bool>, WireError> {
    loop {
        let message = timeout(limit, wire::read_message(stream))
            .await
            .map_err(|_| WireError::TimedOut)??;
        match message {
            Message::KeepAlive => continue,
            Message::Bitfield(bits) => {
                if bits.len() != count.div_ceil(8) {
                    return Err(WireError::Malformed);
                }
                let spare = bits.len() * 8 - count;
                if spare > 0 {
                    let mask = ((1u16 << spare) - 1) as u8;
                    if bits.last().is_some_and(|last| last & mask != 0) {
                        return Err(WireError::Malformed);
                    }
                }
                return Ok((0..count)
                    .map(|index| bits[index / 8] & (0x80 >> (index % 8)) != 0)
                    .collect());
            }
            _ => return Err(WireError::Malformed),
        }
    }
}

async fn wait_for_unchoke(
    stream: &mut TcpStream,
    have: &mut [bool],
    limit: Duration,
) -> Result<(), WireError> {
    loop {
        let message = timeout(limit, wire::read_message(stream))
            .await
            .map_err(|_| WireError::TimedOut)??;
        match message {
            Message::Unchoke => return Ok(()),
            Message::Have(index) => {
                if let Some(slot) = have.get_mut(index as usize) {
                    *slot = true;
                }
            }
            _ => {}
        }
    }
}

fn claim_piece(shared: &Shared, have: &[bool]) -> Option<usize> {
    let mut state = shared.lock();
    let candidates: Vec<usize> = (0..state.done.len())
        .filter(|&index| {
            have.get(index).copied().unwrap_or(false) && !state.done[index] && !state.claimed[index]
        })
        .collect();
    let index = *candidates.choose(&mut rand::thread_rng())?;
    state.claimed[index] = true;
    Some(index)
}

async fn store_piece(shared: &Shared, index: usize, data: Vec<u8>) -> Result<(), WireError> {
    let expected = shared
        .metainfo
        .piece_hash(index)
        .ok_or(WireError::Malformed)?;
    let actual: [u8; 20] = Sha1::digest(&data).into();
    if &actual != expected {
        let mut state = shared.lock();
        state.rejected += 1;
        return Err(WireError::BadPiece);
    }
    {
        let mut file = shared.file.lock().await;
        file.seek(SeekFrom::Start(shared.metainfo.piece_offset(index)))
            .await?;
        file.write_all(&data).await?;
    }
    let mut state = shared.lock();
    state.done[index] = true;
    state.claimed[index] = false;
    state.remaining -= 1;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seed::{SeedServer, DEFAULT_MAX_CONNECTIONS};
    use tokio::net::TcpListener;

    fn fast_options() -> DownloadOptions {
        DownloadOptions {
            peer_id: [1; 20],
            connect_timeout: Duration::from_secs(2),
            message_timeout: Duration::from_secs(5),
            setup_timeout: Duration::from_secs(5),
            piece_timeout: Duration::from_secs(20),
            idle_wait: Duration::from_secs(2),
            pipeline_depth: 16,
            concurrent_pieces_per_peer: 2,
        }
    }

    fn content(length: usize, seed: u8) -> Vec<u8> {
        (0..length)
            .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
            .collect()
    }

    async fn spawn_seed(metainfo: Metainfo, file: PathBuf) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local address");
        let seed = SeedServer::new(metainfo, file, [2; 20], DEFAULT_MAX_CONNECTIONS).expect("seed");
        tokio::spawn(async move {
            let _ = seed.serve(listener).await;
        });
        addr
    }

    #[tokio::test]
    async fn a_file_downloads_from_one_seed_and_matches_byte_for_byte() {
        let dir = tempfile::tempdir().expect("tempdir");
        let original = content(3 * BLOCK_LEN as usize + 5_000, 7);
        let source = dir.path().join("model.gguf");
        std::fs::write(&source, &original).expect("write");
        // Four pieces: three of 16 KiB and a last piece of 5,000 bytes.
        let metainfo = Metainfo::from_file(&source, BLOCK_LEN).expect("metainfo");
        assert_eq!(metainfo.piece_count(), 4);
        let addr = spawn_seed(metainfo.clone(), source.clone()).await;

        let destination = dir.path().join("out").join("model.gguf");
        std::fs::create_dir_all(destination.parent().expect("parent")).expect("mkdir");
        let report = download(&metainfo, &[addr], &destination, fast_options())
            .await
            .expect("download");
        assert_eq!(report.pieces, metainfo.piece_count());
        assert_eq!(report.resumed_pieces, 0);
        assert_eq!(report.rejected_pieces, 0);
        assert_eq!(std::fs::read(&destination).expect("read"), original);
        assert!(!part_path(&destination).exists(), "no .part file remains");
    }

    #[tokio::test]
    async fn resuming_from_partial_part_file_skips_verified_pieces_and_replaces_corrupt_ones() {
        let dir = tempfile::tempdir().expect("tempdir");
        let piece_len = BLOCK_LEN * 2;
        let original = content(piece_len as usize * 4 + 3_000, 11);
        let source = dir.path().join("source.gguf");
        std::fs::write(&source, &original).expect("write source");
        let metainfo = Metainfo::from_file(&source, piece_len).expect("metainfo");
        assert_eq!(metainfo.piece_count(), 5);

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let seed = SeedServer::new(
            metainfo.clone(),
            source.clone(),
            [2; 20],
            DEFAULT_MAX_CONNECTIONS,
        )
        .expect("seed");
        let seed_handle = seed.clone();
        tokio::spawn(async move {
            let _ = seed.serve(listener).await;
        });

        let destination = dir.path().join("resumed.gguf");
        let part = part_path(&destination);
        // Pre-populate .part with pieces 0 and 2 valid, piece 1 corrupted, pieces 3 & 4 zeroed.
        let mut partial = vec![0u8; original.len()];
        let p = piece_len as usize;
        partial[0..p].copy_from_slice(&original[0..p]);
        partial[p..2 * p].copy_from_slice(&original[p..2 * p]);
        partial[p] ^= 0xff; // corrupt piece 1
        partial[2 * p..3 * p].copy_from_slice(&original[2 * p..3 * p]);
        std::fs::write(&part, &partial).expect("write part");

        let report = download(&metainfo, &[addr], &destination, fast_options())
            .await
            .expect("resumed download");
        assert_eq!(report.pieces, 5);
        assert_eq!(
            report.resumed_pieces, 2,
            "only verified pieces 0 and 2 resume"
        );
        assert_eq!(std::fs::read(&destination).expect("read"), original);
        assert!(!part.exists());
        // Seed should have uploaded only the 3 unverified pieces (pieces 1, 3, and 4).
        let expected_uploaded = (original.len() - 2 * p) as u64;
        assert_eq!(seed_handle.uploaded_bytes(), expected_uploaded);
    }

    #[tokio::test]
    async fn complete_verified_part_file_finishes_without_contacting_any_peer() {
        let dir = tempfile::tempdir().expect("tempdir");
        let original = content(BLOCK_LEN as usize * 3 + 123, 19);
        let source = dir.path().join("source.gguf");
        std::fs::write(&source, &original).expect("write");
        let metainfo = Metainfo::from_file(&source, BLOCK_LEN).expect("metainfo");

        let destination = dir.path().join("complete.gguf");
        std::fs::write(part_path(&destination), &original).expect("write complete part");

        let report = download(&metainfo, &[], &destination, fast_options())
            .await
            .expect("finishes locally from verified .part");
        assert_eq!(report.resumed_pieces, metainfo.piece_count());
        assert_eq!(report.peers_connected, 0);
        assert_eq!(std::fs::read(&destination).expect("read"), original);
    }

    #[tokio::test]
    async fn wrong_size_part_file_is_reset_and_downloaded_cleanly() {
        let dir = tempfile::tempdir().expect("tempdir");
        let original = content(BLOCK_LEN as usize * 4, 23);
        let source = dir.path().join("source.gguf");
        std::fs::write(&source, &original).expect("write");
        let metainfo = Metainfo::from_file(&source, BLOCK_LEN).expect("metainfo");
        let addr = spawn_seed(metainfo.clone(), source).await;

        let destination = dir.path().join("reset.gguf");
        std::fs::write(part_path(&destination), b"stale-short-file").expect("write stale");

        let report = download(&metainfo, &[addr], &destination, fast_options())
            .await
            .expect("download after reset");
        assert_eq!(report.resumed_pieces, 0);
        assert_eq!(std::fs::read(&destination).expect("read"), original);
    }

    #[tokio::test]
    async fn concurrent_multi_piece_pipelining_downloads_larger_file_accurately() {
        let dir = tempfile::tempdir().expect("tempdir");
        // 12 pieces * 64 KiB = 768 KiB across 4 concurrent pieces per peer.
        let piece_len = BLOCK_LEN * 4;
        let original = content(piece_len as usize * 12 + 7_777, 41);
        let source = dir.path().join("big.gguf");
        std::fs::write(&source, &original).expect("write");
        let metainfo = Metainfo::from_file(&source, piece_len).expect("metainfo");
        let addr1 = spawn_seed(metainfo.clone(), source.clone()).await;
        let addr2 = spawn_seed(metainfo.clone(), source).await;

        let destination = dir.path().join("big-out.gguf");
        let mut opts = fast_options();
        opts.concurrent_pieces_per_peer = 4;
        opts.pipeline_depth = 32;
        let report = download(&metainfo, &[addr1, addr2], &destination, opts)
            .await
            .expect("multi-piece download");
        assert_eq!(report.pieces, metainfo.piece_count());
        assert_eq!(report.rejected_pieces, 0);
        assert_eq!(std::fs::read(&destination).expect("read"), original);
    }

    /// Returns the metainfo for the honest content and a seed whose first 100
    /// bytes are altered, so piece 0 cannot verify through that seed.
    async fn honest_and_corrupt(dir: &Path) -> (Metainfo, Vec<u8>, SocketAddr, SocketAddr) {
        let original = content(3 * BLOCK_LEN as usize, 3);
        let honest_path = dir.join("honest.gguf");
        std::fs::write(&honest_path, &original).expect("write");
        let metainfo = Metainfo::from_file(&honest_path, 2 * BLOCK_LEN).expect("metainfo");
        let mut tampered = original.clone();
        for byte in tampered.iter_mut().take(100) {
            *byte ^= 0xff;
        }
        let corrupt_path = dir.join("corrupt.gguf");
        std::fs::write(&corrupt_path, &tampered).expect("write");
        let corrupt = spawn_seed(metainfo.clone(), corrupt_path).await;
        let honest = spawn_seed(metainfo.clone(), honest_path).await;
        (metainfo, original, corrupt, honest)
    }

    #[tokio::test]
    async fn a_corrupt_seed_alone_is_rejected_by_hash_and_nothing_is_published() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (metainfo, _original, corrupt, _honest) = honest_and_corrupt(dir.path()).await;
        let destination = dir.path().join("only-corrupt.gguf");
        let result = download(&metainfo, &[corrupt], &destination, fast_options()).await;
        match result {
            Err(DownloadError::Incomplete {
                missing,
                peer_errors,
            }) => {
                assert!(missing >= 1);
                assert!(
                    peer_errors
                        .iter()
                        .any(|error| error.contains("does not match its hash")),
                    "peer errors were {peer_errors:?}"
                );
            }
            other => panic!("expected an incomplete download, got {other:?}"),
        }
        assert!(
            !destination.exists(),
            "unverified content must not be published"
        );
    }

    #[tokio::test]
    async fn a_corrupt_and_an_honest_seed_together_complete_the_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (metainfo, original, corrupt, honest) = honest_and_corrupt(dir.path()).await;
        let destination = dir.path().join("both.gguf");
        let report = download(&metainfo, &[corrupt, honest], &destination, fast_options())
            .await
            .expect("the honest seed finishes the file");
        assert_eq!(report.pieces, metainfo.piece_count());
        assert_eq!(std::fs::read(&destination).expect("read"), original);
    }

    #[tokio::test]
    async fn an_unreachable_peer_alone_yields_an_incomplete_error_and_keeps_the_part_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let original = content(BLOCK_LEN as usize * 2, 1);
        let source = dir.path().join("a.gguf");
        std::fs::write(&source, &original).expect("write");
        let metainfo = Metainfo::from_file(&source, 2 * BLOCK_LEN).expect("metainfo");
        let closed = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = closed.local_addr().expect("addr");
        drop(closed);

        let destination = dir.path().join("never.gguf");
        let result = download(&metainfo, &[addr], &destination, fast_options()).await;
        assert!(matches!(
            result,
            Err(DownloadError::Incomplete { missing: 1, .. })
        ));
        assert!(!destination.exists());
        assert!(part_path(&destination).exists());
    }

    #[tokio::test]
    async fn a_peer_that_only_sends_keep_alives_cannot_stall_the_download() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("a.gguf");
        std::fs::write(&source, content(BLOCK_LEN as usize * 2, 4)).expect("write");
        let metainfo = Metainfo::from_file(&source, 2 * BLOCK_LEN).expect("metainfo");
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let info_hash = metainfo.infohash();
        tokio::spawn(async move {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            if wire::read_handshake(&mut stream).await.is_err() {
                return;
            }
            let _ = wire::write_handshake(
                &mut stream,
                &Handshake {
                    info_hash,
                    peer_id: [9; 20],
                },
            )
            .await;
            let _ = wire::write_message(&mut stream, &Message::Bitfield(vec![0b1000_0000])).await;
            // Never unchokes. Keep-alives keep each read from timing out.
            loop {
                if wire::write_message(&mut stream, &Message::KeepAlive)
                    .await
                    .is_err()
                {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        });

        let destination = dir.path().join("stalled.gguf");
        let result = tokio::time::timeout(
            Duration::from_secs(30),
            download(&metainfo, &[addr], &destination, fast_options()),
        )
        .await
        .expect("the download must end by its own deadline");
        assert!(matches!(result, Err(DownloadError::Incomplete { .. })));
    }

    #[tokio::test]
    async fn a_seed_for_another_swarm_is_refused_in_the_handshake() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("a.gguf");
        std::fs::write(&source, content(1000, 2)).expect("write");
        let metainfo = Metainfo::from_file(&source, BLOCK_LEN * 2).expect("metainfo");
        let addr = spawn_seed(metainfo, source).await;

        let mut stream = TcpStream::connect(addr).await.expect("connect");
        wire::write_handshake(
            &mut stream,
            &Handshake {
                info_hash: [0xaa; 20],
                peer_id: [3; 20],
            },
        )
        .await
        .expect("write");
        assert!(wire::read_handshake(&mut stream).await.is_err());
    }

    #[tokio::test]
    async fn a_request_outside_the_piece_gets_the_connection_closed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("a.gguf");
        std::fs::write(&source, content(1000, 2)).expect("write");
        let metainfo = Metainfo::from_file(&source, BLOCK_LEN * 2).expect("metainfo");
        let addr = spawn_seed(metainfo.clone(), source).await;

        let mut stream = TcpStream::connect(addr).await.expect("connect");
        wire::write_handshake(
            &mut stream,
            &Handshake {
                info_hash: metainfo.infohash(),
                peer_id: [3; 20],
            },
        )
        .await
        .expect("write");
        wire::read_handshake(&mut stream).await.expect("handshake");
        assert_eq!(
            wire::read_message(&mut stream).await.expect("bitfield"),
            Message::Bitfield(vec![0b1000_0000])
        );
        wire::write_message(&mut stream, &Message::Interested)
            .await
            .expect("interested");
        assert_eq!(
            wire::read_message(&mut stream).await.expect("unchoke"),
            Message::Unchoke
        );
        // The only piece holds 1,000 bytes. A block from offset 512 with a full
        // length runs past the end of the piece, so the seed must drop the peer.
        let outside = BlockRef {
            index: 0,
            begin: 512,
            length: BLOCK_LEN,
        };
        wire::write_message(&mut stream, &Message::Request(outside))
            .await
            .expect("request");
        assert!(wire::read_message(&mut stream).await.is_err());
    }
}

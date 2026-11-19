//! Serving side of a private swarm.
//!
//! A seed answers handshakes for its own infohash, advertises every piece, and
//! unchokes peers that express interest. It serves only blocks that lie inside
//! a real piece. Connections are capped, and idle or silent peers are dropped.

use std::io::{self, SeekFrom};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Semaphore;
use tokio::time::timeout;

use crate::metainfo::Metainfo;
use crate::wire::{self, BlockRef, Handshake, Message, WireError, BLOCK_LEN};

/// Default cap on simultaneous peer connections.
pub const DEFAULT_MAX_CONNECTIONS: usize = 64;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const IDLE_TIMEOUT: Duration = Duration::from_secs(120);
/// A connection is closed after this long, whatever its traffic. Keep-alives
/// alone must not hold a slot forever.
const MAX_CONNECTION_LIFETIME: Duration = Duration::from_secs(30 * 60);

/// A seed for one file. Shared across connections.
#[derive(Debug)]
pub struct SeedServer {
    metainfo: Metainfo,
    file_path: PathBuf,
    peer_id: [u8; 20],
    uploaded: AtomicU64,
    slots: Arc<Semaphore>,
}

impl SeedServer {
    /// Creates a seed for `file_path`. The file length must match the metainfo.
    /// Piece hashes are not re-checked here. Provisioning verifies the file.
    pub fn new(
        metainfo: Metainfo,
        file_path: PathBuf,
        peer_id: [u8; 20],
        max_connections: usize,
    ) -> io::Result<Arc<Self>> {
        let length = std::fs::metadata(&file_path)?.len();
        if length != metainfo.length() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "file length does not match the metainfo",
            ));
        }
        Ok(Arc::new(Self {
            metainfo,
            file_path,
            peer_id,
            uploaded: AtomicU64::new(0),
            slots: Arc::new(Semaphore::new(max_connections.max(1))),
        }))
    }

    /// Payload bytes served since the seed started. Counts claims for future
    /// accounting; nothing enforces them yet.
    pub fn uploaded_bytes(&self) -> u64 {
        self.uploaded.load(Ordering::Relaxed)
    }

    /// Accepts connections until the task is dropped. Each connection runs in
    /// its own task, and a connection that fails only ends itself. A failed
    /// accept (for example, a reset during the handshake) is retried after a
    /// short pause rather than stopping the seed.
    pub async fn serve(self: Arc<Self>, listener: TcpListener) -> io::Result<()> {
        loop {
            let (stream, peer) = match listener.accept().await {
                Ok(accepted) => accepted,
                Err(_) => {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                }
            };
            let Ok(permit) = Arc::clone(&self.slots).try_acquire_owned() else {
                // Over the cap: close the socket instead of queueing it.
                continue;
            };
            let seed = Arc::clone(&self);
            tokio::spawn(async move {
                let _permit = permit;
                let _ = seed.handle(stream, peer).await;
            });
        }
    }

    async fn handle(&self, mut stream: TcpStream, _peer: SocketAddr) -> Result<(), WireError> {
        stream.set_nodelay(true)?;
        let incoming = timeout(HANDSHAKE_TIMEOUT, wire::read_handshake(&mut stream))
            .await
            .map_err(|_| WireError::TimedOut)??;
        if incoming.info_hash != self.metainfo.infohash() {
            return Err(WireError::BadHandshake);
        }
        let outgoing = Handshake {
            info_hash: self.metainfo.infohash(),
            peer_id: self.peer_id,
        };
        wire::write_handshake(&mut stream, &outgoing).await?;
        let bitfield = full_bitfield(self.metainfo.piece_count());
        wire::write_message(&mut stream, &Message::Bitfield(bitfield)).await?;

        let mut file = File::open(&self.file_path).await?;
        let mut unchoked = false;
        let lifetime_end = tokio::time::Instant::now() + MAX_CONNECTION_LIFETIME;
        loop {
            let remaining = lifetime_end.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(WireError::TimedOut);
            }
            let message = timeout(IDLE_TIMEOUT.min(remaining), wire::read_message(&mut stream))
                .await
                .map_err(|_| WireError::TimedOut)??;
            match message {
                Message::Interested if !unchoked => {
                    wire::write_message(&mut stream, &Message::Unchoke).await?;
                    unchoked = true;
                }
                Message::Request(block) if unchoked => {
                    let data = self.read_block(&mut file, block).await?;
                    let served = data.len() as u64;
                    let reply = Message::Piece {
                        index: block.index,
                        begin: block.begin,
                        data,
                    };
                    wire::write_message(&mut stream, &reply).await?;
                    self.uploaded.fetch_add(served, Ordering::Relaxed);
                }
                // Keep-alives, have, cancel, and anything else need no reply.
                _ => {}
            }
        }
    }

    async fn read_block(&self, file: &mut File, block: BlockRef) -> Result<Vec<u8>, WireError> {
        let piece_size = self
            .metainfo
            .piece_size(block.index as usize)
            .ok_or(WireError::Malformed)?;
        let end = u64::from(block.begin) + u64::from(block.length);
        if block.length == 0 || block.length > BLOCK_LEN || end > u64::from(piece_size) {
            return Err(WireError::Malformed);
        }
        let offset = self.metainfo.piece_offset(block.index as usize) + u64::from(block.begin);
        file.seek(SeekFrom::Start(offset)).await?;
        let mut data = vec![0u8; block.length as usize];
        file.read_exact(&mut data).await?;
        Ok(data)
    }
}

/// Bitfield with every piece set. Spare bits in the last byte stay zero.
pub(crate) fn full_bitfield(piece_count: usize) -> Vec<u8> {
    let mut bits = vec![0u8; piece_count.div_ceil(8)];
    for index in 0..piece_count {
        bits[index / 8] |= 0x80 >> (index % 8);
    }
    bits
}

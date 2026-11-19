//! Peer wire messages (BEP-3) with explicit length bounds.
//!
//! Every length prefix is checked before its payload is read, so a peer cannot
//! make the reader allocate more than `MAX_MESSAGE_LEN` for one message.

use std::io;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Protocol identifier in the handshake.
pub const PROTOCOL_STRING: &[u8] = b"BitTorrent protocol";
/// Length of the fixed handshake.
pub const HANDSHAKE_LEN: usize = 68;
/// Largest block a request may ask for. Clients use 16 KiB by convention.
pub const BLOCK_LEN: u32 = 16 * 1024;
/// Largest message payload accepted from a peer. A bitfield for the largest
/// accepted piece count is 32 KiB.
pub const MAX_MESSAGE_LEN: u32 = 64 * 1024;

/// Failures on one peer connection. Each one ends that connection only.
#[derive(Debug, thiserror::Error)]
pub enum WireError {
    #[error("connection I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("handshake names another swarm or protocol")]
    BadHandshake,
    #[error("message is malformed")]
    Malformed,
    #[error("message is longer than the accepted limit")]
    TooLong,
    #[error("peer did not answer in time")]
    TimedOut,
    #[error("peer choked the connection in the middle of a piece")]
    Choked,
    #[error("peer sent a piece that does not match its hash")]
    BadPiece,
}

/// The fixed-size handshake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handshake {
    pub info_hash: [u8; 20],
    pub peer_id: [u8; 20],
}

/// A block inside a piece.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockRef {
    pub index: u32,
    pub begin: u32,
    pub length: u32,
}

/// Messages after the handshake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    KeepAlive,
    Choke,
    Unchoke,
    Interested,
    NotInterested,
    Have(u32),
    Bitfield(Vec<u8>),
    Request(BlockRef),
    Piece {
        index: u32,
        begin: u32,
        data: Vec<u8>,
    },
    Cancel(BlockRef),
    /// A message id this crate does not use. Its payload is read and discarded.
    Other(u8),
}

/// Writes the handshake and flushes it.
pub async fn write_handshake<W>(writer: &mut W, handshake: &Handshake) -> io::Result<()>
where
    W: AsyncWriteExt + Unpin,
{
    let mut buffer = Vec::with_capacity(HANDSHAKE_LEN);
    buffer.push(PROTOCOL_STRING.len() as u8);
    buffer.extend_from_slice(PROTOCOL_STRING);
    buffer.extend_from_slice(&[0u8; 8]);
    buffer.extend_from_slice(&handshake.info_hash);
    buffer.extend_from_slice(&handshake.peer_id);
    writer.write_all(&buffer).await?;
    writer.flush().await
}

/// Reads the handshake. Reserved bits are ignored because no extensions are used.
pub async fn read_handshake<R>(reader: &mut R) -> Result<Handshake, WireError>
where
    R: AsyncReadExt + Unpin,
{
    let mut buffer = [0u8; HANDSHAKE_LEN];
    reader.read_exact(&mut buffer).await?;
    if usize::from(buffer[0]) != PROTOCOL_STRING.len() || &buffer[1..20] != PROTOCOL_STRING {
        return Err(WireError::BadHandshake);
    }
    let mut info_hash = [0u8; 20];
    info_hash.copy_from_slice(&buffer[28..48]);
    let mut peer_id = [0u8; 20];
    peer_id.copy_from_slice(&buffer[48..68]);
    Ok(Handshake { info_hash, peer_id })
}

/// Reads one message, rejecting any length prefix above `MAX_MESSAGE_LEN`.
pub async fn read_message<R>(reader: &mut R) -> Result<Message, WireError>
where
    R: AsyncReadExt + Unpin,
{
    let length = reader.read_u32().await?;
    if length == 0 {
        return Ok(Message::KeepAlive);
    }
    if length > MAX_MESSAGE_LEN {
        return Err(WireError::TooLong);
    }
    let id = reader.read_u8().await?;
    let payload = (length - 1) as usize;
    match id {
        0..=3 => {
            if payload != 0 {
                return Err(WireError::Malformed);
            }
            Ok(match id {
                0 => Message::Choke,
                1 => Message::Unchoke,
                2 => Message::Interested,
                _ => Message::NotInterested,
            })
        }
        4 => {
            if payload != 4 {
                return Err(WireError::Malformed);
            }
            Ok(Message::Have(reader.read_u32().await?))
        }
        5 => {
            if payload == 0 {
                return Err(WireError::Malformed);
            }
            let mut bits = vec![0u8; payload];
            reader.read_exact(&mut bits).await?;
            Ok(Message::Bitfield(bits))
        }
        6 | 8 => {
            if payload != 12 {
                return Err(WireError::Malformed);
            }
            let block = read_block(reader).await?;
            Ok(if id == 6 {
                Message::Request(block)
            } else {
                Message::Cancel(block)
            })
        }
        7 => {
            if payload < 8 || payload - 8 > BLOCK_LEN as usize {
                return Err(WireError::Malformed);
            }
            let index = reader.read_u32().await?;
            let begin = reader.read_u32().await?;
            let mut data = vec![0u8; payload - 8];
            reader.read_exact(&mut data).await?;
            Ok(Message::Piece { index, begin, data })
        }
        _ => {
            let mut remaining = payload;
            let mut sink = [0u8; 1024];
            while remaining > 0 {
                let take = remaining.min(sink.len());
                reader.read_exact(&mut sink[..take]).await?;
                remaining -= take;
            }
            Ok(Message::Other(id))
        }
    }
}

/// Writes one message and flushes it. `Message::Other` is never written.
pub async fn write_message<W>(writer: &mut W, message: &Message) -> io::Result<()>
where
    W: AsyncWriteExt + Unpin,
{
    let mut buffer = Vec::with_capacity(17 + BLOCK_LEN as usize);
    match message {
        Message::KeepAlive => buffer.extend_from_slice(&0u32.to_be_bytes()),
        Message::Choke => push_fixed(&mut buffer, 0),
        Message::Unchoke => push_fixed(&mut buffer, 1),
        Message::Interested => push_fixed(&mut buffer, 2),
        Message::NotInterested => push_fixed(&mut buffer, 3),
        Message::Have(index) => {
            buffer.extend_from_slice(&5u32.to_be_bytes());
            buffer.push(4);
            buffer.extend_from_slice(&index.to_be_bytes());
        }
        Message::Bitfield(bits) => {
            let length = u32::try_from(bits.len() + 1)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "bitfield too long"))?;
            buffer.extend_from_slice(&length.to_be_bytes());
            buffer.push(5);
            buffer.extend_from_slice(bits);
        }
        Message::Request(block) => push_block(&mut buffer, 6, *block),
        Message::Cancel(block) => push_block(&mut buffer, 8, *block),
        Message::Piece { index, begin, data } => {
            let length = u32::try_from(data.len() + 9)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "piece too long"))?;
            buffer.extend_from_slice(&length.to_be_bytes());
            buffer.push(7);
            buffer.extend_from_slice(&index.to_be_bytes());
            buffer.extend_from_slice(&begin.to_be_bytes());
            buffer.extend_from_slice(data);
        }
        Message::Other(_) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unknown messages are not written",
            ))
        }
    }
    writer.write_all(&buffer).await?;
    writer.flush().await
}

fn push_fixed(buffer: &mut Vec<u8>, id: u8) {
    buffer.extend_from_slice(&1u32.to_be_bytes());
    buffer.push(id);
}

fn push_block(buffer: &mut Vec<u8>, id: u8, block: BlockRef) {
    buffer.extend_from_slice(&13u32.to_be_bytes());
    buffer.push(id);
    buffer.extend_from_slice(&block.index.to_be_bytes());
    buffer.extend_from_slice(&block.begin.to_be_bytes());
    buffer.extend_from_slice(&block.length.to_be_bytes());
}

async fn read_block<R>(reader: &mut R) -> Result<BlockRef, WireError>
where
    R: AsyncReadExt + Unpin,
{
    Ok(BlockRef {
        index: reader.read_u32().await?,
        begin: reader.read_u32().await?,
        length: reader.read_u32().await?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn round_trip(message: Message) -> Message {
        let (mut a, mut b) = tokio::io::duplex(1 << 20);
        write_message(&mut a, &message).await.expect("write");
        read_message(&mut b).await.expect("read")
    }

    #[tokio::test]
    async fn every_message_kind_round_trips() {
        assert_eq!(round_trip(Message::KeepAlive).await, Message::KeepAlive);
        assert_eq!(round_trip(Message::Choke).await, Message::Choke);
        assert_eq!(round_trip(Message::Unchoke).await, Message::Unchoke);
        assert_eq!(round_trip(Message::Interested).await, Message::Interested);
        assert_eq!(
            round_trip(Message::NotInterested).await,
            Message::NotInterested
        );
        assert_eq!(round_trip(Message::Have(7)).await, Message::Have(7));
        assert_eq!(
            round_trip(Message::Bitfield(vec![0xff, 0x80])).await,
            Message::Bitfield(vec![0xff, 0x80])
        );
        let block = BlockRef {
            index: 3,
            begin: 16384,
            length: 16384,
        };
        assert_eq!(
            round_trip(Message::Request(block)).await,
            Message::Request(block)
        );
        assert_eq!(
            round_trip(Message::Cancel(block)).await,
            Message::Cancel(block)
        );
        let piece = Message::Piece {
            index: 3,
            begin: 0,
            data: vec![1, 2, 3],
        };
        assert_eq!(round_trip(piece.clone()).await, piece);
    }

    #[tokio::test]
    async fn handshake_round_trips_and_rejects_another_protocol() {
        let (mut a, mut b) = tokio::io::duplex(4096);
        let hs = Handshake {
            info_hash: [4; 20],
            peer_id: [5; 20],
        };
        write_handshake(&mut a, &hs).await.expect("write");
        assert_eq!(read_handshake(&mut b).await.expect("read"), hs);

        let (mut c, mut d) = tokio::io::duplex(4096);
        let mut wrong = vec![19u8];
        wrong.extend_from_slice(b"BitTorrent protocoX");
        wrong.extend_from_slice(&[0u8; 48]);
        tokio::io::AsyncWriteExt::write_all(&mut c, &wrong)
            .await
            .expect("write");
        assert!(matches!(
            read_handshake(&mut d).await,
            Err(WireError::BadHandshake)
        ));
    }

    #[tokio::test]
    async fn an_oversized_length_prefix_is_refused_before_allocation() {
        let (mut a, mut b) = tokio::io::duplex(64);
        tokio::io::AsyncWriteExt::write_all(&mut a, &(MAX_MESSAGE_LEN + 1).to_be_bytes())
            .await
            .expect("write");
        assert!(matches!(
            read_message(&mut b).await,
            Err(WireError::TooLong)
        ));
    }

    #[tokio::test]
    async fn fixed_messages_with_a_wrong_length_are_malformed() {
        let (mut a, mut b) = tokio::io::duplex(64);
        // A `have` must carry exactly four payload bytes.
        tokio::io::AsyncWriteExt::write_all(&mut a, &[0, 0, 0, 3, 4, 0, 0])
            .await
            .expect("write");
        assert!(matches!(
            read_message(&mut b).await,
            Err(WireError::Malformed)
        ));
    }

    #[tokio::test]
    async fn a_piece_longer_than_one_block_is_malformed() {
        let (mut a, mut b) = tokio::io::duplex(1 << 20);
        let data = vec![0u8; BLOCK_LEN as usize + 1];
        let length = (data.len() + 9) as u32;
        tokio::io::AsyncWriteExt::write_all(&mut a, &length.to_be_bytes())
            .await
            .expect("write");
        tokio::io::AsyncWriteExt::write_all(&mut a, &[7u8])
            .await
            .expect("write");
        tokio::io::AsyncWriteExt::write_all(&mut a, &[0u8; 8])
            .await
            .expect("write");
        tokio::io::AsyncWriteExt::write_all(&mut a, &data)
            .await
            .expect("write");
        assert!(matches!(
            read_message(&mut b).await,
            Err(WireError::Malformed)
        ));
    }

    #[tokio::test]
    async fn unknown_messages_are_read_and_skipped() {
        let (mut a, mut b) = tokio::io::duplex(64);
        tokio::io::AsyncWriteExt::write_all(&mut a, &[0, 0, 0, 4, 20, 1, 2, 3])
            .await
            .expect("write");
        assert_eq!(
            read_message(&mut b).await.expect("read"),
            Message::Other(20)
        );
    }
}

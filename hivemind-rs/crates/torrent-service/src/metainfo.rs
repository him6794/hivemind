//! BitTorrent metainfo (.torrent) file creation.
//! Produces Bencode-encoded metainfo with SHA-1 piece hashing
//! following the BitTorrent v1 specification.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

/// The size of each piece in the torrent (512 KiB = standard)
pub const DEFAULT_PIECE_SIZE: usize = 524_288; // 512 KiB

/// A serializable BitTorrent metainfo dictionary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metainfo {
    pub announce: String,
    #[serde(rename = "created by")]
    pub created_by: String,
    #[serde(rename = "creation date")]
    pub creation_date: i64,
    pub info: InfoDict,
}

/// The 'info' dictionary inside a torrent metainfo
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfoDict {
    #[serde(rename = "piece length")]
    pub piece_length: u64,
    /// Concatenated 20-byte SHA-1 hashes. Stored as bytes so binary piece
    /// digests remain stable through bencode round-trips.
    pub pieces: Vec<u8>,
    pub name: String,
    pub length: u64,
}

/// Result of creating a torrent metainfo
pub struct TorrentMetainfo {
    pub metainfo: Metainfo,
    pub info_hash: String,
    pub info_hash_bytes: [u8; 20],
    pub piece_hashes: Vec<[u8; 20]>,
    pub piece_count: usize,
    pub piece_size: u64,
}

/// Create a BitTorrent metainfo from raw data.
pub fn create_metainfo(
    data: &[u8],
    file_path: &std::path::Path,
    announce: &str,
) -> Result<TorrentMetainfo> {
    let piece_size = DEFAULT_PIECE_SIZE as u64;
    let file_name = file_path
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("task"))
        .to_string_lossy()
        .to_string();

    let piece_hashes = compute_pieces(data, piece_size as usize);
    let pieces_binary: Vec<u8> = piece_hashes.iter().flat_map(|h| h.to_vec()).collect();

    let info = InfoDict {
        piece_length: piece_size,
        pieces: pieces_binary,
        name: file_name,
        length: data.len() as u64,
    };

    let info_bytes = bendy::serde::to_bytes(&info)
        .map_err(|e| anyhow::anyhow!("Failed to bencode info dict: {}", e))?;
    let digest = Sha1::digest(&info_bytes);
    let mut info_hash_bytes = [0u8; 20];
    info_hash_bytes.copy_from_slice(&digest);
    let info_hash = hex::encode(info_hash_bytes);

    let metainfo = Metainfo {
        announce: announce.to_string(),
        created_by: "Hivemind Torrent Service".to_string(),
        creation_date: chrono::Utc::now().timestamp(),
        info,
    };

    Ok(TorrentMetainfo {
        metainfo,
        info_hash,
        info_hash_bytes,
        piece_count: piece_hashes.len(),
        piece_hashes,
        piece_size,
    })
}

/// Compute SHA-1 hashes for each piece of data
pub fn compute_pieces(data: &[u8], piece_size: usize) -> Vec<[u8; 20]> {
    if data.is_empty() {
        let mut hasher = Sha1::new();
        hasher.update([]);
        return vec![hasher.finalize().into()];
    }
    data.chunks(piece_size)
        .map(|chunk| {
            let mut hasher = Sha1::new();
            hasher.update(chunk);
            hasher.finalize().into()
        })
        .collect()
}

/// Verify a piece against its expected SHA-1 hash
pub fn verify_piece(data: &[u8], expected_hash: &[u8; 20]) -> bool {
    let mut hasher = Sha1::new();
    hasher.update(data);
    let actual: [u8; 20] = hasher.finalize().into();
    &actual == expected_hash
}

/// Decode concatenated piece hashes from a metainfo pieces blob.
pub fn decode_piece_hashes(pieces: &[u8]) -> Result<Vec<[u8; 20]>> {
    if !pieces.len().is_multiple_of(20) {
        anyhow::bail!(
            "invalid pieces blob length {}; expected multiple of 20",
            pieces.len()
        );
    }
    Ok(pieces
        .chunks_exact(20)
        .map(|chunk| {
            let mut hash = [0u8; 20];
            hash.copy_from_slice(chunk);
            hash
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_pieces_single_piece() {
        let data = b"hello world torrent test data!";
        let pieces = compute_pieces(data, 1024);
        assert_eq!(pieces.len(), 1, "small data should fit in one piece");
        assert_eq!(pieces[0].len(), 20, "SHA-1 output is 20 bytes");
    }

    #[test]
    fn test_compute_pieces_multiple() {
        let data = vec![0u8; 600_000]; // > 512 KiB
        let pieces = compute_pieces(&data, DEFAULT_PIECE_SIZE);
        assert!(
            pieces.len() >= 2,
            "data larger than piece_size should span multiple pieces"
        );
    }

    #[test]
    fn test_create_metainfo() {
        let data = b"test payload for torrent metainfo";
        let path = std::path::Path::new("task_abc.zip");
        let result = create_metainfo(data, path, "http://tracker:6969/announce").unwrap();
        assert_eq!(result.metainfo.info.length, data.len() as u64);
        assert!(!result.info_hash.is_empty());
        assert!(result.piece_count >= 1);
        assert_eq!(result.metainfo.info.pieces.len(), result.piece_count * 20);
    }

    #[test]
    fn test_verify_piece_valid() {
        let data = b"some chunk data";
        let mut hasher = Sha1::new();
        hasher.update(data);
        let hash: [u8; 20] = hasher.finalize().into();
        assert!(verify_piece(data, &hash));
    }

    #[test]
    fn test_verify_piece_invalid() {
        let data = b"some chunk data";
        let bad_hash = [0u8; 20];
        assert!(!verify_piece(data, &bad_hash));
    }

    #[test]
    fn create_metainfo_info_hash_is_stable_for_binary_piece_hashes() {
        let data = b"binary-piece-hash-stability-payload";
        let path = std::path::Path::new("task.zip");
        let first = create_metainfo(data, path, "http://tracker:6969/announce").unwrap();
        let second = create_metainfo(data, path, "http://tracker:6969/announce").unwrap();
        assert_eq!(first.info_hash, second.info_hash);
        assert_eq!(first.metainfo.info.pieces, second.metainfo.info.pieces);
        assert_eq!(first.metainfo.info.pieces.len(), 20);
    }
}

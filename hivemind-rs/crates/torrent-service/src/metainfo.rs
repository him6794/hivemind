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
    pub pieces: String, // binary blob of concatenated 20-byte SHA-1 hashes
    pub name: String,
    pub length: u64,
}

/// Result of creating a torrent metainfo
pub struct TorrentMetainfo {
    pub metainfo: Metainfo,
    pub info_hash: String,
    pub piece_count: usize,
    pub piece_size: u64,
}

/// Create a BitTorrent metainfo from raw data.
pub fn create_metainfo(data: &[u8], file_path: &std::path::Path, announce: &str) -> Result<TorrentMetainfo> {
    let piece_size = DEFAULT_PIECE_SIZE as u64;
    let file_name = file_path
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("task"))
        .to_string_lossy()
        .to_string();

    // Compute SHA-1 piece hashes
    let piece_hashes = compute_pieces(data, piece_size as usize);
    let pieces_binary: Vec<u8> = piece_hashes.iter().flat_map(|h| h.to_vec()).collect();
    let pieces_string = String::from_utf8(pieces_binary)
        .unwrap_or_else(|_| String::from_utf8_lossy(b"").to_string());

    let info = InfoDict {
        piece_length: piece_size,
        pieces: pieces_string,
        name: file_name,
        length: data.len() as u64,
    };

    // Compute the info-hash (SHA-1 of the bencoded info dict)
    let info_bytes = bendy::serde::to_bytes(&info)
        .map_err(|e| anyhow::anyhow!("Failed to bencode info dict: {}", e))?;
    let info_hash = hex::encode(Sha1::digest(&info_bytes));

    let metainfo = Metainfo {
        announce: announce.to_string(),
        created_by: "Hivemind Torrent Service".to_string(),
        creation_date: chrono::Utc::now().timestamp(),
        info,
    };

    Ok(TorrentMetainfo {
        metainfo,
        info_hash,
        piece_count: piece_hashes.len(),
        piece_size,
    })
}

/// Compute SHA-1 hashes for each piece of data
fn compute_pieces(data: &[u8], piece_size: usize) -> Vec<[u8; 20]> {
    data.chunks(piece_size).map(|chunk| {
        let mut hasher = Sha1::new();
        hasher.update(chunk);
        hasher.finalize().into()
    }).collect()
}

/// Verify a piece against its expected SHA-1 hash
pub fn verify_piece(data: &[u8], expected_hash: &[u8; 20]) -> bool {
    let mut hasher = Sha1::new();
    hasher.update(data);
    let actual: [u8; 20] = hasher.finalize().into();
    &actual == expected_hash
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
        assert!(pieces.len() >= 2, "data larger than piece_size should span multiple pieces");
    }

    #[test]
    fn test_create_metainfo() {
        let data = b"test payload for torrent metainfo";
        let path = std::path::Path::new("task_abc.zip");
        let result = create_metainfo(data, path, "http://tracker:6969/announce").unwrap();
        assert_eq!(result.metainfo.info.length, data.len() as u64);
        assert!(!result.info_hash.is_empty());
        assert!(result.piece_count >= 1);
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
}

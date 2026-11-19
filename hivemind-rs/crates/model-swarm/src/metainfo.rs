//! Single-file metainfo for a private swarm.
//!
//! The info dictionary follows BEP-3 and sets `private` to 1 (BEP-27), which
//! tells compliant clients to use only the trackers they were given and to
//! skip DHT and peer exchange. The infohash is SHA-1 over the bencoded info
//! dictionary, as BEP-3 specifies.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use sha1::{Digest, Sha1};

use crate::bencode::{self, BencodeError, Value};

/// Smallest piece length. Piece lengths must be a multiple of it.
pub const MIN_PIECE_LENGTH: u32 = 16 << 10;
/// Largest piece length.
pub const MAX_PIECE_LENGTH: u32 = 16 << 20;
/// Largest number of pieces. The peer bitfield is bounded by this value.
pub const MAX_PIECES: usize = 1 << 18;
/// Largest metainfo file accepted.
pub const MAX_METAINFO_BYTES: usize = 8 << 20;

/// Reasons a metainfo file or its source file is refused.
#[derive(Debug, thiserror::Error)]
pub enum MetainfoError {
    #[error("file error: {0}")]
    Io(#[from] std::io::Error),
    #[error("metainfo is not canonical bencode: {0}")]
    Bencode(#[from] BencodeError),
    #[error("metainfo field `{0}` is missing or invalid")]
    Field(&'static str),
    #[error("metainfo is larger than the accepted size")]
    TooLarge,
    #[error("metainfo does not set the private flag")]
    NotPrivate,
    #[error("piece count does not match the file length and piece length")]
    PieceCount,
    #[error("piece length is outside the accepted range")]
    PieceLength,
}

/// A parsed or generated single-file metainfo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metainfo {
    name: String,
    length: u64,
    piece_length: u32,
    pieces: Vec<[u8; 20]>,
    info_bytes: Vec<u8>,
    infohash: [u8; 20],
}

impl Metainfo {
    /// Hashes `path` into pieces of `piece_length` bytes.
    pub fn from_file(path: &Path, piece_length: u32) -> Result<Self, MetainfoError> {
        check_piece_length(piece_length)?;
        let name = file_name(path)?;
        let mut file = File::open(path)?;
        let length = file.metadata()?.len();
        if length == 0 || i64::try_from(length).is_err() {
            return Err(MetainfoError::Field("length"));
        }
        let piece_count = usize::try_from(length.div_ceil(u64::from(piece_length)))
            .map_err(|_| MetainfoError::PieceCount)?;
        if piece_count > MAX_PIECES {
            return Err(MetainfoError::PieceCount);
        }
        let mut pieces = Vec::with_capacity(piece_count);
        let mut buffer = vec![0u8; piece_length as usize];
        for _ in 0..piece_count {
            let filled = read_full(&mut file, &mut buffer)?;
            let digest: [u8; 20] = Sha1::digest(&buffer[..filled]).into();
            pieces.push(digest);
        }
        Ok(Self::assemble(name, length, piece_length, pieces))
    }

    /// Parses a metainfo file. Every field is checked against the same rules
    /// that `from_file` follows.
    pub fn parse(bytes: &[u8]) -> Result<Self, MetainfoError> {
        if bytes.len() > MAX_METAINFO_BYTES {
            return Err(MetainfoError::TooLarge);
        }
        let Value::Dict(top) = bencode::decode(bytes)? else {
            return Err(MetainfoError::Field("top level"));
        };
        let Some(Value::Dict(info)) = top.get(b"info".as_slice()) else {
            return Err(MetainfoError::Field("info"));
        };
        let name_bytes = dict_bytes(info, "name")?;
        let name =
            String::from_utf8(name_bytes.to_vec()).map_err(|_| MetainfoError::Field("name"))?;
        check_name(&name)?;
        let length = u64::try_from(dict_int(info, "length")?)
            .ok()
            .filter(|len| *len > 0)
            .ok_or(MetainfoError::Field("length"))?;
        let piece_length = u32::try_from(dict_int(info, "piece length")?)
            .map_err(|_| MetainfoError::PieceLength)?;
        check_piece_length(piece_length)?;
        let piece_bytes = dict_bytes(info, "pieces")?;
        if !piece_bytes.len().is_multiple_of(20) {
            return Err(MetainfoError::Field("pieces"));
        }
        // The length check above guarantees that no remainder is left over.
        let pieces: Vec<[u8; 20]> = piece_bytes.as_chunks::<20>().0.to_vec();
        let expected = usize::try_from(length.div_ceil(u64::from(piece_length)))
            .map_err(|_| MetainfoError::PieceCount)?;
        if pieces.len() != expected || pieces.len() > MAX_PIECES {
            return Err(MetainfoError::PieceCount);
        }
        if info.contains_key(b"files".as_slice()) {
            return Err(MetainfoError::Field("files"));
        }
        if dict_int(info, "private")? != 1 {
            return Err(MetainfoError::NotPrivate);
        }
        for key in info.keys() {
            if !matches!(
                key.as_slice(),
                b"name" | b"length" | b"piece length" | b"pieces" | b"private"
            ) {
                return Err(MetainfoError::Field("unknown info key"));
            }
        }
        let mut info_bytes = Vec::new();
        bencode::encode(&Value::Dict(info.clone()), &mut info_bytes);
        let infohash: [u8; 20] = Sha1::digest(&info_bytes).into();
        Ok(Self {
            name,
            length,
            piece_length,
            pieces,
            info_bytes,
            infohash,
        })
    }

    /// Serializes the metainfo file: a top-level dictionary with one `info` key.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.info_bytes.len() + 8);
        out.extend_from_slice(b"d4:info");
        out.extend_from_slice(&self.info_bytes);
        out.push(b'e');
        out
    }

    /// File name the content is stored under.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Length of the content in bytes.
    pub fn length(&self) -> u64 {
        self.length
    }

    /// Length of every piece except possibly the last.
    pub fn piece_length(&self) -> u32 {
        self.piece_length
    }

    /// Number of pieces.
    pub fn piece_count(&self) -> usize {
        self.pieces.len()
    }

    /// SHA-1 of the piece at `index`.
    pub fn piece_hash(&self, index: usize) -> Option<&[u8; 20]> {
        self.pieces.get(index)
    }

    /// Byte length of the piece at `index`. The last piece may be shorter.
    pub fn piece_size(&self, index: usize) -> Option<u32> {
        if index >= self.pieces.len() {
            return None;
        }
        let start = u64::try_from(index).ok()? * u64::from(self.piece_length);
        let remaining = self.length - start;
        Some(remaining.min(u64::from(self.piece_length)) as u32)
    }

    /// Byte offset of the piece at `index` within the content.
    pub fn piece_offset(&self, index: usize) -> u64 {
        index as u64 * u64::from(self.piece_length)
    }

    /// The BitTorrent v1 infohash.
    pub fn infohash(&self) -> [u8; 20] {
        self.infohash
    }

    /// The infohash as 40 lowercase hex characters.
    pub fn infohash_hex(&self) -> String {
        hex::encode(self.infohash)
    }

    fn assemble(name: String, length: u64, piece_length: u32, pieces: Vec<[u8; 20]>) -> Self {
        let mut info = BTreeMap::new();
        info.insert(b"length".to_vec(), Value::Int(length as i64));
        info.insert(b"name".to_vec(), Value::Bytes(name.as_bytes().to_vec()));
        info.insert(
            b"piece length".to_vec(),
            Value::Int(i64::from(piece_length)),
        );
        let flat: Vec<u8> = pieces.iter().flatten().copied().collect();
        info.insert(b"pieces".to_vec(), Value::Bytes(flat));
        info.insert(b"private".to_vec(), Value::Int(1));
        let mut info_bytes = Vec::new();
        bencode::encode(&Value::Dict(info), &mut info_bytes);
        let infohash: [u8; 20] = Sha1::digest(&info_bytes).into();
        Self {
            name,
            length,
            piece_length,
            pieces,
            info_bytes,
            infohash,
        }
    }
}

fn check_piece_length(piece_length: u32) -> Result<(), MetainfoError> {
    if (MIN_PIECE_LENGTH..=MAX_PIECE_LENGTH).contains(&piece_length)
        && piece_length.is_multiple_of(MIN_PIECE_LENGTH)
    {
        Ok(())
    } else {
        Err(MetainfoError::PieceLength)
    }
}

fn check_name(name: &str) -> Result<(), MetainfoError> {
    let forbidden = name
        .chars()
        .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'));
    if name.is_empty() || name.len() > 255 || name == "." || name == ".." || forbidden {
        return Err(MetainfoError::Field("name"));
    }
    Ok(())
}

fn file_name(path: &Path) -> Result<String, MetainfoError> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or(MetainfoError::Field("name"))?
        .to_owned();
    check_name(&name)?;
    Ok(name)
}

fn dict_int(info: &BTreeMap<Vec<u8>, Value>, key: &'static str) -> Result<i64, MetainfoError> {
    match info.get(key.as_bytes()) {
        Some(Value::Int(number)) => Ok(*number),
        _ => Err(MetainfoError::Field(key)),
    }
}

fn dict_bytes<'a>(
    info: &'a BTreeMap<Vec<u8>, Value>,
    key: &'static str,
) -> Result<&'a [u8], MetainfoError> {
    match info.get(key.as_bytes()) {
        Some(Value::Bytes(bytes)) => Ok(bytes.as_slice()),
        _ => Err(MetainfoError::Field(key)),
    }
}

/// Reads until `buffer` is full or the file ends. Returns the number of bytes read.
fn read_full(file: &mut File, buffer: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        let read = file.read(&mut buffer[filled..])?;
        if read == 0 {
            break;
        }
        filled += read;
    }
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Replaces the first occurrence of `needle`. Works on raw bytes because
    /// piece hashes are not UTF-8.
    fn replace_once(haystack: &[u8], needle: &[u8], replacement: &[u8]) -> Vec<u8> {
        let position = haystack
            .windows(needle.len())
            .position(|window| window == needle)
            .expect("needle is present");
        let mut out = Vec::with_capacity(haystack.len() + replacement.len());
        out.extend_from_slice(&haystack[..position]);
        out.extend_from_slice(replacement);
        out.extend_from_slice(&haystack[position + needle.len()..]);
        out
    }

    fn write_file(dir: &Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, bytes).expect("write fixture");
        path
    }

    #[test]
    fn pieces_hash_the_file_in_order_and_the_last_piece_is_shorter() {
        let dir = tempfile::tempdir().expect("tempdir");
        let piece = MIN_PIECE_LENGTH as usize;
        let content: Vec<u8> = (0..piece * 2 + 1000).map(|i| (i % 251) as u8).collect();
        let path = write_file(dir.path(), "model.gguf", &content);
        let meta = Metainfo::from_file(&path, MIN_PIECE_LENGTH).expect("metainfo");
        assert_eq!(meta.piece_count(), 3);
        assert_eq!(meta.piece_size(0), Some(MIN_PIECE_LENGTH));
        assert_eq!(meta.piece_size(2), Some(1000));
        assert_eq!(meta.piece_size(3), None);
        let expected: [u8; 20] = Sha1::digest(&content[piece..piece * 2]).into();
        assert_eq!(meta.piece_hash(1), Some(&expected));
        assert_eq!(meta.name(), "model.gguf");
    }

    #[test]
    fn a_metainfo_file_parses_back_to_the_same_infohash() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_file(dir.path(), "a.gguf", &vec![9u8; 40_000]);
        let generated = Metainfo::from_file(&path, MIN_PIECE_LENGTH).expect("metainfo");
        let parsed = Metainfo::parse(&generated.to_bytes()).expect("parses");
        assert_eq!(parsed, generated);
        assert_eq!(parsed.infohash_hex().len(), 40);
    }

    #[test]
    fn a_public_metainfo_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_file(dir.path(), "a.gguf", &[1u8; 100]);
        let meta = Metainfo::from_file(&path, MIN_PIECE_LENGTH).expect("metainfo");
        let public = replace_once(&meta.to_bytes(), b"7:privatei1e", b"");
        assert!(matches!(
            Metainfo::parse(&public),
            Err(MetainfoError::Field(_)) | Err(MetainfoError::NotPrivate)
        ));
    }

    #[test]
    fn multi_file_metainfo_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_file(dir.path(), "a.gguf", &[1u8; 100]);
        let meta = Metainfo::from_file(&path, MIN_PIECE_LENGTH).expect("metainfo");
        let multi = replace_once(
            &meta.to_bytes(),
            b"6:lengthi100e",
            b"5:filesle6:lengthi100e",
        );
        assert!(Metainfo::parse(&multi).is_err());
    }

    #[test]
    fn a_piece_count_that_does_not_match_the_length_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_file(dir.path(), "a.gguf", &[1u8; 100]);
        let meta = Metainfo::from_file(&path, MIN_PIECE_LENGTH).expect("metainfo");
        let longer = replace_once(&meta.to_bytes(), b"6:lengthi100e", b"6:lengthi99999999e");
        assert!(matches!(
            Metainfo::parse(&longer),
            Err(MetainfoError::PieceCount)
        ));
    }

    #[test]
    fn a_piece_length_off_the_grid_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_file(dir.path(), "a.gguf", &[1u8; 100]);
        assert!(matches!(
            Metainfo::from_file(&path, MIN_PIECE_LENGTH + 1),
            Err(MetainfoError::PieceLength)
        ));
    }
}

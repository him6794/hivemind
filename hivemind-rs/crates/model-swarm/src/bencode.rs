//! Strict bencode for BitTorrent metainfo.
//!
//! The decoder accepts only canonical forms: no leading zeros in integers or
//! lengths, no `-0`, and dictionary keys in strictly increasing order. Because
//! of that, decoding and re-encoding a value returns the same bytes, which the
//! metainfo parser relies on to hash the `info` dictionary.

use std::collections::BTreeMap;

/// Deepest nesting the decoder accepts.
pub const MAX_DEPTH: usize = 32;

/// A bencoded value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Int(i64),
    Bytes(Vec<u8>),
    List(Vec<Value>),
    Dict(BTreeMap<Vec<u8>, Value>),
}

/// Refusals from the decoder. Positions are byte offsets into the input.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BencodeError {
    #[error("input ends early at byte {0}")]
    Truncated(usize),
    #[error("malformed value at byte {0}")]
    Malformed(usize),
    #[error("nesting is deeper than the decoder allows")]
    TooDeep,
    #[error("bytes follow the top-level value")]
    Trailing,
}

/// Decodes exactly one value that spans the whole input.
pub fn decode(input: &[u8]) -> Result<Value, BencodeError> {
    let (value, end) = parse(input, 0, 0)?;
    if end != input.len() {
        return Err(BencodeError::Trailing);
    }
    Ok(value)
}

/// Appends the canonical encoding of `value` to `out`.
pub fn encode(value: &Value, out: &mut Vec<u8>) {
    match value {
        Value::Int(number) => {
            out.push(b'i');
            out.extend_from_slice(number.to_string().as_bytes());
            out.push(b'e');
        }
        Value::Bytes(bytes) => encode_bytes(bytes, out),
        Value::List(items) => {
            out.push(b'l');
            for item in items {
                encode(item, out);
            }
            out.push(b'e');
        }
        Value::Dict(entries) => {
            out.push(b'd');
            for (key, item) in entries {
                encode_bytes(key, out);
                encode(item, out);
            }
            out.push(b'e');
        }
    }
}

fn encode_bytes(bytes: &[u8], out: &mut Vec<u8>) {
    out.extend_from_slice(bytes.len().to_string().as_bytes());
    out.push(b':');
    out.extend_from_slice(bytes);
}

fn parse(input: &[u8], pos: usize, depth: usize) -> Result<(Value, usize), BencodeError> {
    if depth > MAX_DEPTH {
        return Err(BencodeError::TooDeep);
    }
    match input.get(pos).copied() {
        None => Err(BencodeError::Truncated(pos)),
        Some(b'i') => parse_int(input, pos),
        Some(b'l') => {
            let mut items = Vec::new();
            let mut cursor = pos + 1;
            loop {
                match input.get(cursor).copied() {
                    None => return Err(BencodeError::Truncated(cursor)),
                    Some(b'e') => return Ok((Value::List(items), cursor + 1)),
                    Some(_) => {
                        let (item, next) = parse(input, cursor, depth + 1)?;
                        items.push(item);
                        cursor = next;
                    }
                }
            }
        }
        Some(b'd') => {
            let mut entries = BTreeMap::new();
            let mut previous: Option<Vec<u8>> = None;
            let mut cursor = pos + 1;
            loop {
                match input.get(cursor).copied() {
                    None => return Err(BencodeError::Truncated(cursor)),
                    Some(b'e') => return Ok((Value::Dict(entries), cursor + 1)),
                    Some(b'0'..=b'9') => {
                        let (key, after_key) = parse_bytes(input, cursor)?;
                        if previous.as_ref().is_some_and(|last| key <= *last) {
                            return Err(BencodeError::Malformed(cursor));
                        }
                        let (item, next) = parse(input, after_key, depth + 1)?;
                        previous = Some(key.clone());
                        entries.insert(key, item);
                        cursor = next;
                    }
                    Some(_) => return Err(BencodeError::Malformed(cursor)),
                }
            }
        }
        Some(b'0'..=b'9') => {
            let (bytes, end) = parse_bytes(input, pos)?;
            Ok((Value::Bytes(bytes), end))
        }
        Some(_) => Err(BencodeError::Malformed(pos)),
    }
}

fn parse_int(input: &[u8], pos: usize) -> Result<(Value, usize), BencodeError> {
    // An i64 has at most 20 characters including the sign.
    let window_end = input.len().min(pos + 1 + 21);
    let window = &input[pos + 1..window_end];
    let terminator = window
        .iter()
        .position(|b| *b == b'e')
        .ok_or(BencodeError::Malformed(pos))?;
    let digits = &window[..terminator];
    let unsigned = digits.strip_prefix(b"-").unwrap_or(digits);
    let canonical = !unsigned.is_empty()
        && unsigned.iter().all(u8::is_ascii_digit)
        && (unsigned.len() == 1 || unsigned[0] != b'0')
        && digits != b"-0";
    if !canonical {
        return Err(BencodeError::Malformed(pos));
    }
    let text = std::str::from_utf8(digits).map_err(|_| BencodeError::Malformed(pos))?;
    let number: i64 = text.parse().map_err(|_| BencodeError::Malformed(pos))?;
    Ok((Value::Int(number), pos + 1 + terminator + 1))
}

fn parse_bytes(input: &[u8], pos: usize) -> Result<(Vec<u8>, usize), BencodeError> {
    // A length prefix is at most 10 digits.
    let window_end = input.len().min(pos + 11);
    let window = &input[pos..window_end];
    let colon = window
        .iter()
        .position(|b| *b == b':')
        .ok_or(BencodeError::Malformed(pos))?;
    let digits = &window[..colon];
    let canonical = !digits.is_empty()
        && digits.iter().all(u8::is_ascii_digit)
        && (digits.len() == 1 || digits[0] != b'0');
    if !canonical {
        return Err(BencodeError::Malformed(pos));
    }
    let text = std::str::from_utf8(digits).map_err(|_| BencodeError::Malformed(pos))?;
    let length: usize = text.parse().map_err(|_| BencodeError::Malformed(pos))?;
    let start = pos + colon + 1;
    let end = start
        .checked_add(length)
        .ok_or(BencodeError::Malformed(pos))?;
    let bytes = input
        .get(start..end)
        .ok_or(BencodeError::Truncated(input.len()))?;
    Ok((bytes.to_vec(), end))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(text: &[u8]) -> Value {
        let value = decode(text).expect("decodes");
        let mut out = Vec::new();
        encode(&value, &mut out);
        assert_eq!(out, text, "re-encoding must reproduce the input");
        value
    }

    #[test]
    fn canonical_values_round_trip_byte_for_byte() {
        roundtrip(b"i0e");
        roundtrip(b"i-42e");
        roundtrip(b"4:spam");
        roundtrip(b"le");
        roundtrip(b"l4:spami42ee");
        roundtrip(b"d3:bar4:spam3:fooi42ee");
    }

    #[test]
    fn non_canonical_integers_are_refused() {
        assert!(matches!(decode(b"i03e"), Err(BencodeError::Malformed(0))));
        assert!(matches!(decode(b"i-0e"), Err(BencodeError::Malformed(0))));
        assert!(matches!(decode(b"ie"), Err(BencodeError::Malformed(0))));
    }

    #[test]
    fn non_canonical_lengths_are_refused() {
        assert!(matches!(decode(b"01:a"), Err(BencodeError::Malformed(0))));
        assert!(matches!(decode(b":a"), Err(BencodeError::Malformed(0))));
    }

    #[test]
    fn unsorted_or_duplicate_dictionary_keys_are_refused() {
        assert!(matches!(
            decode(b"d1:bi1e1:ai2ee"),
            Err(BencodeError::Malformed(_))
        ));
        assert!(matches!(
            decode(b"d1:ai1e1:ai2ee"),
            Err(BencodeError::Malformed(_))
        ));
    }

    #[test]
    fn truncation_and_trailing_bytes_are_refused() {
        assert!(matches!(decode(b"4:sp"), Err(BencodeError::Truncated(_))));
        assert!(matches!(decode(b"li1e"), Err(BencodeError::Truncated(_))));
        assert_eq!(decode(b"i1ex"), Err(BencodeError::Trailing));
    }

    #[test]
    fn nesting_beyond_the_limit_is_refused() {
        let mut deep = vec![b'l'; MAX_DEPTH + 2];
        deep.extend(std::iter::repeat_n(b'e', MAX_DEPTH + 2));
        assert_eq!(decode(&deep), Err(BencodeError::TooDeep));
    }
}

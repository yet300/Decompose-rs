//! Internal child-`StateKeeper` framing (`DCS1`).
//!
//! A child `StateKeeper::save()` yields a whole `BTreeMap<String, Vec<u8>>`
//! but the parent provider slot holds a single `Vec<u8>`. `DCS1` bridges the
//! two with a tiny versioned container:
//!
//! ```text
//! magic "DCS1"              4 bytes
//! entry_count               u32 little-endian
//! repeated entry_count times:
//!     key_len               u32 little-endian
//!     key UTF-8 bytes
//!     value_len             u32 little-endian
//!     value bytes
//! ```
//!
//! Properties: deterministic `BTreeMap` ordering, no `serde`, versioned by
//! magic, checked cursor arithmetic (huge prefixes error, never panic),
//! duplicate encoded keys rejected, trailing bytes rejected, invalid UTF-8
//! rejected. Empty maps, empty keys, and arbitrary binary values are legal;
//! nesting works by framing a frame. Decode failures map into the
//! `StateKeeper` error path as `Decode` at the adapter boundary.
//!
//! This is internal child-`StateKeeper` framing only — not a public
//! navigation serialization API.

use std::collections::BTreeMap;
use std::fmt;

const MAGIC: &[u8; 4] = b"DCS1";

/// Internal framing failure. Surfaced to callers as
/// [`StateKeeperError::Decode`](essenty_state_keeper::StateKeeperError::Decode).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FramingError {
    BadMagic,
    Truncated,
    TrailingBytes,
    InvalidUtf8,
    DuplicateKey,
}

impl fmt::Display for FramingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FramingError::BadMagic => write!(f, "bad DCS1 magic"),
            FramingError::Truncated => write!(f, "truncated DCS1 frame"),
            FramingError::TrailingBytes => write!(f, "trailing bytes after DCS1 frame"),
            FramingError::InvalidUtf8 => write!(f, "invalid UTF-8 in DCS1 key"),
            FramingError::DuplicateKey => write!(f, "duplicate key in DCS1 frame"),
        }
    }
}

impl std::error::Error for FramingError {}

fn read_u32(bytes: &[u8], cursor: &mut usize) -> Result<u32, FramingError> {
    let start = cursor.checked_add(4).ok_or(FramingError::Truncated)?;
    if start > bytes.len() {
        return Err(FramingError::Truncated);
    }
    let mut buf = [0u8; 4];
    buf.copy_from_slice(&bytes[*cursor..start]);
    *cursor = start;
    Ok(u32::from_le_bytes(buf))
}

fn read_bytes<'a>(bytes: &'a [u8], cursor: &mut usize, len: u32) -> Result<&'a [u8], FramingError> {
    let len = len as usize;
    let end = cursor.checked_add(len).ok_or(FramingError::Truncated)?;
    if end > bytes.len() {
        return Err(FramingError::Truncated);
    }
    let slice = &bytes[*cursor..end];
    *cursor = end;
    Ok(slice)
}

/// Encodes a state map into one `DCS1` frame. Iteration follows `BTreeMap`
/// order, so encoding is deterministic.
pub(crate) fn encode(entries: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let mut out = Vec::with_capacity(8);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for (key, value) in entries {
        out.extend_from_slice(&(key.len() as u32).to_le_bytes());
        out.extend_from_slice(key.as_bytes());
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
        out.extend_from_slice(value);
    }
    out
}

/// Decodes one `DCS1` frame. Any structural violation errors; lengths use
/// checked arithmetic throughout, so hostile prefixes (including
/// `u32::MAX`) error instead of panicking.
pub(crate) fn decode(bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, FramingError> {
    if bytes.len() < MAGIC.len() || &bytes[..MAGIC.len()] != MAGIC {
        return Err(FramingError::BadMagic);
    }
    let mut cursor = MAGIC.len();
    let count = read_u32(bytes, &mut cursor)?;
    let mut entries = BTreeMap::new();
    for _ in 0..count {
        let key_len = read_u32(bytes, &mut cursor)?;
        let key_bytes = read_bytes(bytes, &mut cursor, key_len)?;
        let key = std::str::from_utf8(key_bytes).map_err(|_| FramingError::InvalidUtf8)?;
        let value_len = read_u32(bytes, &mut cursor)?;
        let value = read_bytes(bytes, &mut cursor, value_len)?.to_vec();
        if entries.insert(key.to_owned(), value).is_some() {
            return Err(FramingError::DuplicateKey);
        }
    }
    if cursor != bytes.len() {
        return Err(FramingError::TrailingBytes);
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> BTreeMap<String, Vec<u8>> {
        BTreeMap::from([
            ("b".to_owned(), vec![1, 2, 3]),
            ("a".to_owned(), vec![]),
            ("".to_owned(), vec![0xff, 0x00]),
        ])
    }

    #[test]
    fn round_trip_and_deterministic_order() {
        let first = encode(&sample());
        let second = encode(&sample());
        assert_eq!(first, second);
        // BTreeMap order: "" < "a" < "b" — keys appear sorted on the wire.
        let decoded = decode(&first).unwrap();
        assert_eq!(decoded, sample());
        // Positional check: empty key first ("": klen 0, vlen 2).
        let body = &first[8..];
        assert_eq!(&body[..8], &[0, 0, 0, 0, 2, 0, 0, 0]);
        assert_eq!(&body[8..10], &[0xff, 0x00]);
    }

    #[test]
    fn empty_map_is_legal() {
        let frame = encode(&BTreeMap::new());
        assert_eq!(frame, b"DCS1\x00\x00\x00\x00");
        assert_eq!(decode(&frame).unwrap(), BTreeMap::new());
    }

    #[test]
    fn empty_key_and_binary_payload_round_trip() {
        let entries = BTreeMap::from([("".to_owned(), vec![0, 159, 146, 150])]);
        assert_eq!(decode(&encode(&entries)).unwrap(), entries);
    }

    #[test]
    fn nested_frame_in_frame_round_trip() {
        let inner = encode(&sample());
        let outer = BTreeMap::from([("child".to_owned(), inner.clone())]);
        let decoded_outer = decode(&encode(&outer)).unwrap();
        assert_eq!(decode(&decoded_outer["child"]).unwrap(), sample());
    }

    #[test]
    fn corrupt_magic_rejected() {
        assert_eq!(decode(b"XXXX\x00\x00\x00\x00"), Err(FramingError::BadMagic));
        assert_eq!(decode(b""), Err(FramingError::BadMagic));
        assert_eq!(decode(b"DCS"), Err(FramingError::BadMagic));
    }

    #[test]
    fn truncation_at_every_prefix_length_errors() {
        let frame = encode(&sample());
        assert!(frame.len() > 8);
        for len in 0..frame.len() {
            assert!(
                decode(&frame[..len]).is_err(),
                "truncation at {len} must error, not panic"
            );
        }
    }

    #[test]
    fn trailing_bytes_rejected() {
        let mut frame = encode(&sample());
        frame.push(0x00);
        assert_eq!(decode(&frame), Err(FramingError::TrailingBytes));
    }

    #[test]
    fn duplicate_encoded_key_rejected() {
        // Hand-built frame with the same key twice (BTreeMap cannot hold it).
        let mut frame = Vec::new();
        frame.extend_from_slice(b"DCS1");
        frame.extend_from_slice(&2u32.to_le_bytes());
        for _ in 0..2 {
            frame.extend_from_slice(&1u32.to_le_bytes());
            frame.extend_from_slice(b"k");
            frame.extend_from_slice(&1u32.to_le_bytes());
            frame.extend_from_slice(b"v");
        }
        assert_eq!(decode(&frame), Err(FramingError::DuplicateKey));
    }

    #[test]
    fn invalid_utf8_key_rejected() {
        let mut frame = Vec::new();
        frame.extend_from_slice(b"DCS1");
        frame.extend_from_slice(&1u32.to_le_bytes());
        frame.extend_from_slice(&2u32.to_le_bytes());
        frame.extend_from_slice(&[0xff, 0xfe]);
        frame.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(decode(&frame), Err(FramingError::InvalidUtf8));
    }

    #[test]
    fn huge_length_prefix_errors_without_panic() {
        let mut frame = Vec::new();
        frame.extend_from_slice(b"DCS1");
        frame.extend_from_slice(&1u32.to_le_bytes());
        frame.extend_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(decode(&frame), Err(FramingError::Truncated));
        // Huge value length with a valid key behaves the same.
        let mut frame = Vec::new();
        frame.extend_from_slice(b"DCS1");
        frame.extend_from_slice(&1u32.to_le_bytes());
        frame.extend_from_slice(&1u32.to_le_bytes());
        frame.extend_from_slice(b"k");
        frame.extend_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(decode(&frame), Err(FramingError::Truncated));
    }
}

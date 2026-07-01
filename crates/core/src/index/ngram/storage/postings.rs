//! Contiguous delta-varint encoded file-id payloads referenced by the lexicon.

use std::path::Path;

use crate::index::ngram::storage::format::POSTINGS_MAGIC;
use crate::index::snapshot::ArtifactData;

use super::read_u32_le;
use crate::index::mmap::mmap_open;

#[derive(Debug)]
pub struct Postings {
    data: ArtifactData,
    payload_len: usize,
}

impl Postings {
    fn bytes(&self) -> &[u8] {
        self.data.as_ref()
    }

    /// Validate and wrap in-memory or mmap artifact bytes as postings.
    ///
    /// # Errors
    ///
    /// Returns an error if the header or payload length is invalid.
    pub fn from_artifact(data: ArtifactData) -> std::io::Result<Self> {
        let bytes = data.as_ref();
        let payload_len = Self::validate(bytes)?;
        Ok(Self { data, payload_len })
    }

    /// Encode a postings payload into bytes (magic + length prefix + payload).
    ///
    /// # Errors
    ///
    /// Returns an error if the payload length exceeds `u32::MAX`.
    pub fn encode(payload: &[u8]) -> std::io::Result<Vec<u8>> {
        let mut data = Vec::with_capacity(POSTINGS_MAGIC.len() + 4 + payload.len());
        data.extend_from_slice(&POSTINGS_MAGIC);
        let plen = u32::try_from(payload.len()).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "postings payload exceeds u32::MAX",
            )
        })?;
        data.extend_from_slice(&plen.to_le_bytes());
        data.extend_from_slice(payload);
        Ok(data)
    }

    /// Write a postings file and return an mmap-backed instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written or reopened.
    pub fn create(path: &Path, payload: &[u8]) -> std::io::Result<Self> {
        let data = Self::encode(payload)?;
        std::fs::write(path, &data)?;
        Self::open(path)
    }

    /// Open postings from a memory-mapped file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file is malformed.
    pub fn open(path: &Path) -> std::io::Result<Self> {
        let mmap = mmap_open(path)?;
        Self::from_artifact(ArtifactData::Mmap(mmap))
    }

    fn validate(bytes: &[u8]) -> std::io::Result<usize> {
        let magic_len = POSTINGS_MAGIC.len();
        if bytes.len() < magic_len + 4 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "postings too short for magic+len",
            ));
        }
        if bytes[..magic_len] != POSTINGS_MAGIC {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "unexpected postings magic",
            ));
        }
        let plen = read_u32_le(bytes, magic_len) as usize;
        if bytes.len() < magic_len + 4 + plen {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "postings payload shorter than declared length",
            ));
        }
        if bytes.len() > magic_len + 4 + plen {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "postings has trailing bytes after declared payload",
            ));
        }
        Ok(plen)
    }

    /// Walk an individual delta-encoded posting list without allocating.
    ///
    /// Returns the count of decoded values.  Rejects malformed varints, non-monotonic
    /// deltas, overflow, and values exceeding `u32::MAX`.
    pub(crate) fn validate_list(bytes: &[u8]) -> std::io::Result<usize> {
        let mut pos = 0usize;
        let mut prev = 0u64;
        let mut count = 0usize;
        while pos < bytes.len() {
            let (raw, remaining) = unsigned_varint::decode::u64(&bytes[pos..]).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "malformed varint in posting list",
                )
            })?;
            let consumed = bytes[pos..].len().saturating_sub(remaining.len());
            let value = if count == 0 {
                raw
            } else {
                prev.checked_add(raw).ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "delta overflow in posting list",
                    )
                })?
            };
            if value > u64::from(u32::MAX) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "posting value exceeds u32::MAX",
                ));
            }
            prev = value;
            pos += consumed;
            count += 1;
        }
        Ok(count)
    }

    #[must_use]
    pub const fn payload_len(&self) -> usize {
        self.payload_len
    }

    #[must_use]
    pub fn slice(&self, start: usize, len: usize) -> &[u8] {
        let payload_start = POSTINGS_MAGIC.len() + 4;
        let start = payload_start + start;
        self.bytes().get(start..start + len).unwrap_or(&[])
    }

    pub(crate) fn decode_sorted(bytes: &[u8]) -> std::io::Result<Vec<u32>> {
        let mut out = Vec::new();
        let mut pos = 0usize;
        let mut prev = 0u64;
        while pos < bytes.len() {
            let (raw, remaining) = unsigned_varint::decode::u64(&bytes[pos..]).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "malformed varint in posting list",
                )
            })?;
            let consumed = bytes[pos..].len().saturating_sub(remaining.len());
            pos += consumed;
            let value = if out.is_empty() {
                raw
            } else {
                prev.checked_add(raw).ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "delta overflow in posting list",
                    )
                })?
            };
            if value > u64::from(u32::MAX) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "posting value exceeds u32::MAX",
                ));
            }
            out.push(u32::try_from(value).expect("value bounded above"));
            prev = value;
        }
        Ok(out)
    }

    pub(crate) fn intersect_sorted(ids: &[u32], encoded: &[u8]) -> std::io::Result<Vec<u32>> {
        let mut i = 0usize;
        let mut pos = 0usize;
        let mut prev = 0u64;
        let mut first = true;
        let mut out = Vec::new();

        while i < ids.len() && pos < encoded.len() {
            let (raw, remaining) = unsigned_varint::decode::u64(&encoded[pos..]).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "malformed varint in posting list",
                )
            })?;
            pos += encoded[pos..].len().saturating_sub(remaining.len());
            let value = if first {
                first = false;
                raw
            } else {
                prev.checked_add(raw).ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "delta overflow in posting list",
                    )
                })?
            };
            if value > u64::from(u32::MAX) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "posting value exceeds u32::MAX",
                ));
            }
            let v = u32::try_from(value).expect("value bounded above");
            prev = value;

            while i < ids.len() && u64::from(ids[i]) < value {
                i += 1;
            }
            if i < ids.len() && ids[i] == v {
                out.push(v);
                i += 1;
            }
        }

        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn encode_sorted(values: &[u32]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut prev = 0u64;
        for (i, &value) in values.iter().enumerate() {
            let raw = if i == 0 {
                u64::from(value)
            } else {
                u64::from(value) - prev
            };
            let mut buf = unsigned_varint::encode::u64_buffer();
            let encoded = unsigned_varint::encode::u64(raw, &mut buf);
            out.extend_from_slice(encoded);
            prev = u64::from(value);
        }
        out
    }

    #[test]
    fn create_and_open_roundtrips() {
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("postings.bin");
        let payload = vec![1u8, 2, 3, 4, 5, 6, 7, 8];
        let postings = Postings::create(&path, &payload).expect("create");
        assert_eq!(postings.payload_len(), payload.len());
        let slice = postings.slice(0, payload.len());
        assert_eq!(slice, payload.as_slice());
    }

    #[test]
    fn slice_returns_requested_range() {
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("postings.bin");
        let payload: Vec<u8> = (0..16).collect();
        let postings = Postings::create(&path, &payload).expect("create");
        let slice = postings.slice(4, 8);
        assert_eq!(slice, &payload[4..12]);
    }

    #[test]
    fn open_rejects_bad_magic() {
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("postings.bin");
        std::fs::write(&path, b"BADMAGIC").expect("write");
        let result = Postings::open(&path);
        assert!(result.is_err());
    }

    #[test]
    fn open_rejects_declared_payload_longer_than_file() {
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("postings.bin");
        let mut data = POSTINGS_MAGIC.to_vec();
        data.extend_from_slice(&100u32.to_le_bytes());
        data.extend_from_slice(&[0u8; 4]);
        std::fs::write(&path, &data).expect("write");
        let result = Postings::open(&path);
        assert!(result.is_err());
    }

    #[test]
    fn encode_decode_roundtrips() {
        let ids = vec![0u32, 1, 5, 100, 10_000];
        let encoded = encode_sorted(&ids);
        let decoded = Postings::decode_sorted(&encoded).expect("decode");
        assert_eq!(decoded, ids);
    }

    #[test]
    fn intersect_works() {
        let left = vec![1u32, 3, 5, 7];
        let encoded = encode_sorted(&[2u32, 3, 6, 7]);
        let result = Postings::intersect_sorted(&left, &encoded).expect("intersect");
        assert_eq!(result, vec![3, 7]);
    }

    #[test]
    fn decode_rejects_malformed_varint() {
        let result = Postings::decode_sorted(&[0xff]);
        assert!(result.is_err());
    }

    #[test]
    fn open_rejects_trailing_bytes() {
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("postings.bin");
        let mut data = POSTINGS_MAGIC.to_vec();
        data.extend_from_slice(&4u32.to_le_bytes()); // declares payload length 4
        data.extend_from_slice(b"abcd");
        data.extend_from_slice(b"TRAILING"); // extra bytes
        std::fs::write(&path, &data).expect("write");
        let result = Postings::open(&path);
        assert!(result.is_err());
    }

    #[test]
    fn validate_list_rejects_truncated_varint() {
        let result = Postings::validate_list(&[0x80, 0x80, 0x80]);
        assert!(result.is_err());
    }

    #[test]
    fn validate_list_returns_count() {
        let buf = encode_sorted(&[0u32, 2, 5]);
        let count = Postings::validate_list(&buf).expect("validate");
        assert_eq!(count, 3);
    }

    #[test]
    fn validate_list_rejects_value_exceeding_u32_max() {
        // First value 0, then a delta that would produce value > u32::MAX.
        let mut buf = vec![0u8]; // single-byte varint for 0
        let mut buffer = unsigned_varint::encode::u64_buffer();
        let encoded = unsigned_varint::encode::u64(u64::from(u32::MAX) + 1, &mut buffer);
        buf.extend_from_slice(encoded);
        let result = Postings::validate_list(&buf);
        assert!(result.is_err());
    }
}

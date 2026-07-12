//! File-id payloads referenced by the lexicon.
//!
//! Each posting list is a self-describing blob: a varint element count, then
//! whole 128-value blocks SIMD delta-bitpacked via [`BitPacker4x`], then a
//! delta-varint tail for the remaining `< 128` values.

use std::path::Path;

use bitpacking::{BitPacker, BitPacker4x};
use integer_encoding::VarInt;

use crate::index::ngram::storage::format::POSTINGS_MAGIC;
use crate::index::snapshot::ArtifactData;

use super::read_u32_le;
use crate::index::mmap::mmap_open;

/// Values per SIMD-bitpacked block.
const BLOCK_LEN: usize = BitPacker4x::BLOCK_LEN;

#[derive(Debug)]
pub struct Postings {
    data: ArtifactData,
    payload_len: usize,
}

impl Postings {
    fn bytes(&self) -> &[u8] {
        self.data.as_ref()
    }

    fn malformed(msg: &'static str) -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::InvalidData, msg)
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

    /// Encode a strictly increasing file-id list into a self-describing blob.
    ///
    /// Full 128-value blocks are delta-bitpacked; the remainder is delta-varint
    /// encoded. Callers guarantee `ids` is sorted and unique (posting lists are
    /// assembled from the sorted `(gram, file id)` pairs).
    pub(crate) fn encode_list(ids: &[u32]) -> Vec<u8> {
        let mut out = Vec::with_capacity(ids.len() + 8);
        let mut buf = [0u8; 10];
        let count = u64::try_from(ids.len()).expect("list length fits u64");
        let n = count.encode_var(&mut buf);
        out.extend_from_slice(&buf[..n]);
        if ids.is_empty() {
            return out;
        }

        let bitpacker = BitPacker4x::new();
        let full = ids.len() / BLOCK_LEN;
        let mut initial = 0u32;
        let mut block_buf = [0u8; BLOCK_LEN * 4];
        for b in 0..full {
            let block = &ids[b * BLOCK_LEN..(b + 1) * BLOCK_LEN];
            let num_bits = bitpacker.num_bits_sorted(initial, block);
            out.push(num_bits);
            let written = bitpacker.compress_sorted(initial, block, &mut block_buf, num_bits);
            out.extend_from_slice(&block_buf[..written]);
            initial = block[BLOCK_LEN - 1];
        }

        let mut prev = u64::from(initial);
        for &value in &ids[full * BLOCK_LEN..] {
            let raw = u64::from(value)
                .checked_sub(prev)
                .expect("posting ids strictly increasing");
            let n = raw.encode_var(&mut buf);
            out.extend_from_slice(&buf[..n]);
            prev = u64::from(value);
        }
        out
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

    /// Decode a posting list into its sorted values.
    ///
    /// Rejects truncated blocks, oversized `num_bits`, malformed tail varints,
    /// delta overflow, values exceeding `u32::MAX`, and trailing bytes.
    pub(crate) fn decode_sorted(bytes: &[u8]) -> std::io::Result<Vec<u32>> {
        PostingValues::new(bytes)?.collect()
    }

    pub(crate) fn intersect_sorted(ids: &[u32], encoded: &[u8]) -> std::io::Result<Vec<u32>> {
        let mut decoded = PostingValues::new(encoded)?;
        let mut out = Vec::with_capacity(ids.len());
        let mut i = 0usize;
        while let Some(value) = decoded.next()? {
            while i < ids.len() && ids[i] < value {
                i += 1;
            }
            if i < ids.len() && ids[i] == value {
                out.push(value);
                i += 1;
            }
        }
        Ok(out)
    }
}

struct PostingValues<'a> {
    bytes: &'a [u8],
    pos: usize,
    remaining: usize,
    full_blocks_remaining: usize,
    tail_remaining: usize,
    bitpacker: BitPacker4x,
    block_initial: u32,
    block_buf: [u32; BLOCK_LEN],
    block_cursor: usize,
    in_tail: bool,
    tail_prev: u64,
}

impl<'a> PostingValues<'a> {
    fn new(bytes: &'a [u8]) -> std::io::Result<Self> {
        let (count, pos) =
            u64::decode_var(bytes).ok_or_else(|| Postings::malformed("malformed varint"))?;
        let count = usize::try_from(count)
            .map_err(|_| Postings::malformed("posting count exceeds usize"))?;
        if count == 0 {
            if pos != bytes.len() {
                return Err(Postings::malformed(
                    "trailing bytes after empty posting list",
                ));
            }
            return Ok(Self {
                bytes,
                pos,
                remaining: 0,
                full_blocks_remaining: 0,
                tail_remaining: 0,
                bitpacker: BitPacker4x::new(),
                block_initial: 0,
                block_buf: [0; BLOCK_LEN],
                block_cursor: BLOCK_LEN,
                in_tail: false,
                tail_prev: 0,
            });
        }

        let full_blocks_remaining = count / BLOCK_LEN;
        let tail_remaining = count % BLOCK_LEN;
        if full_blocks_remaining + tail_remaining > bytes.len() - pos {
            return Err(Postings::malformed("posting count exceeds payload size"));
        }

        Ok(Self {
            bytes,
            pos,
            remaining: count,
            full_blocks_remaining,
            tail_remaining,
            bitpacker: BitPacker4x::new(),
            block_initial: 0,
            block_buf: [0; BLOCK_LEN],
            block_cursor: BLOCK_LEN,
            in_tail: false,
            tail_prev: 0,
        })
    }

    fn load_block(&mut self) -> std::io::Result<()> {
        let num_bits = *self
            .bytes
            .get(self.pos)
            .ok_or_else(|| Postings::malformed("truncated posting block header"))?;
        self.pos += 1;
        if num_bits > 32 {
            return Err(Postings::malformed("posting block num_bits exceeds 32"));
        }
        let block_bytes = num_bits as usize * BLOCK_LEN / 8;
        let compressed = self
            .bytes
            .get(self.pos..self.pos + block_bytes)
            .ok_or_else(|| Postings::malformed("truncated posting block"))?;
        self.pos += block_bytes;
        self.bitpacker.decompress_sorted(
            self.block_initial,
            compressed,
            &mut self.block_buf,
            num_bits,
        );
        self.block_initial = self.block_buf[BLOCK_LEN - 1];
        self.block_cursor = 0;
        self.full_blocks_remaining -= 1;
        Ok(())
    }

    fn next(&mut self) -> std::io::Result<Option<u32>> {
        if self.remaining == 0 {
            if self.pos != self.bytes.len() {
                return Err(Postings::malformed("trailing bytes after posting list"));
            }
            return Ok(None);
        }

        if self.block_cursor < BLOCK_LEN {
            let value = self.block_buf[self.block_cursor];
            self.block_cursor += 1;
            self.remaining -= 1;
            return Ok(Some(value));
        }

        if self.full_blocks_remaining > 0 {
            self.load_block()?;
            return self.next();
        }

        if self.tail_remaining > 0 {
            if !self.in_tail {
                self.in_tail = true;
                self.tail_prev = u64::from(self.block_initial);
            }
            let (raw, consumed) = u64::decode_var(&self.bytes[self.pos..])
                .ok_or_else(|| Postings::malformed("malformed varint"))?;
            self.pos += consumed;
            let value = self
                .tail_prev
                .checked_add(raw)
                .ok_or_else(|| Postings::malformed("delta overflow in posting list"))?;
            if value > u64::from(u32::MAX) {
                return Err(Postings::malformed("posting value exceeds u32::MAX"));
            }
            self.tail_prev = value;
            self.tail_remaining -= 1;
            self.remaining -= 1;
            return Ok(Some(u32::try_from(value).expect("value bounded above")));
        }

        Err(Postings::malformed("truncated posting list"))
    }

    fn collect(mut self) -> std::io::Result<Vec<u32>> {
        let mut out = Vec::with_capacity(self.remaining);
        while let Some(value) = self.next()? {
            out.push(value);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

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
        let encoded = Postings::encode_list(&ids);
        let decoded = Postings::decode_sorted(&encoded).expect("decode");
        assert_eq!(decoded, ids);
    }

    #[test]
    fn encode_decode_roundtrips_across_blocks() {
        // Exercises full bitpacked blocks plus a varint tail (300 = 2*128 + 44).
        let ids: Vec<u32> = (0..300u32).map(|i| i * 3 + 7).collect();
        let encoded = Postings::encode_list(&ids);
        let decoded = Postings::decode_sorted(&encoded).expect("decode");
        assert_eq!(decoded, ids);
    }

    #[test]
    fn encode_decode_roundtrips_exact_block_multiple() {
        let ids: Vec<u32> = (0..256u32).map(|i| i * 5).collect();
        let encoded = Postings::encode_list(&ids);
        assert_eq!(Postings::decode_sorted(&encoded).expect("decode"), ids);
    }

    #[test]
    fn encode_decode_empty() {
        let encoded = Postings::encode_list(&[]);
        assert!(
            Postings::decode_sorted(&encoded)
                .expect("decode")
                .is_empty()
        );
    }

    #[test]
    fn intersect_works() {
        let left = vec![1u32, 3, 5, 7];
        let encoded = Postings::encode_list(&[2u32, 3, 6, 7]);
        let result = Postings::intersect_sorted(&left, &encoded).expect("intersect");
        assert_eq!(result, vec![3, 7]);
    }

    #[test]
    fn intersect_works_across_blocks() {
        let left: Vec<u32> = (0..400u32).filter(|i| i % 2 == 0).collect();
        let encoded =
            Postings::encode_list(&(0..400u32).filter(|i| i % 3 == 0).collect::<Vec<_>>());
        let result = Postings::intersect_sorted(&left, &encoded).expect("intersect");
        let expected: Vec<u32> = (0..400u32).filter(|i| i % 6 == 0).collect();
        assert_eq!(result, expected);
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
    fn decode_rejects_truncated_varint() {
        let result = Postings::decode_sorted(&[0x80, 0x80, 0x80]);
        assert!(result.is_err());
    }

    #[test]
    fn decode_rejects_count_larger_than_payload() {
        // A corrupt blob declaring a huge count with no payload must be rejected
        // cheaply rather than reserving for that many values.
        let buf = u64::MAX.encode_var_vec();
        let result = Postings::decode_sorted(&buf);
        assert!(result.is_err());
    }

    #[test]
    fn decode_rejects_value_exceeding_u32_max() {
        // count = 1 (all tail), then a tail delta producing value > u32::MAX.
        let mut buf = Vec::new();
        buf.extend_from_slice(&1u64.encode_var_vec());
        buf.extend_from_slice(&(u64::from(u32::MAX) + 1).encode_var_vec());
        let result = Postings::decode_sorted(&buf);
        assert!(result.is_err());
    }
}

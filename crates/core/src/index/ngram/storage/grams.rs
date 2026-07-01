//! Per-file gram sets: file id -> sorted unique N-grams.

use std::path::Path;

use super::format::GRAMS_MAGIC;
use super::{read_u32_le, read_u64_le};
use crate::index::mmap::mmap_open;
use crate::index::ngram::gram::{Gram, GramWidth, GramWindows};
use crate::index::snapshot::ArtifactData;

/// A single sorted unique gram set for one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GramSet {
    grams: Vec<Gram>,
}

impl GramSet {
    /// Build a gram set from strictly sorted unique grams.
    ///
    /// # Errors
    ///
    /// Returns an error if `grams` is not sorted and unique.
    pub fn new(grams: Vec<Gram>) -> std::io::Result<Self> {
        let mut prev: Option<Gram> = None;
        for gram in &grams {
            if let Some(prev) = prev
                && *gram <= prev
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "gram set is not sorted and unique",
                ));
            }
            prev = Some(*gram);
        }
        Ok(Self { grams })
    }

    #[must_use]
    pub fn collect(width: GramWidth, bytes: &[u8]) -> Self {
        if bytes.len() < width.get() {
            return Self { grams: Vec::new() };
        }
        let mut grams: Vec<Gram> = GramWindows::new(bytes, width).collect();
        grams.sort_unstable();
        grams.dedup();
        Self { grams }
    }

    #[must_use]
    pub fn as_slice(&self) -> &[Gram] {
        &self.grams
    }

    pub(crate) fn encode_into(&self, out: &mut Vec<u8>) -> std::io::Result<()> {
        let mut prev = 0u64;
        for (i, gram) in self.grams.iter().enumerate() {
            let val = gram.ordinal();
            let raw = if i == 0 {
                val
            } else {
                val.checked_sub(prev).ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, "non-monotonic gram set")
                })?
            };
            let mut buf = unsigned_varint::encode::u64_buffer();
            let encoded = unsigned_varint::encode::u64(raw, &mut buf);
            out.extend_from_slice(encoded);
            prev = val;
        }
        Ok(())
    }

    /// Decode a delta-varint encoded gram set.
    ///
    /// # Errors
    ///
    /// Returns an error if the payload is malformed or contains non-monotonic values.
    pub fn decode(width: GramWidth, bytes: &[u8]) -> std::io::Result<Self> {
        let mut out = Vec::new();
        let mut pos = 0usize;
        let mut prev = 0u64;
        while pos < bytes.len() {
            let (raw, remaining) = unsigned_varint::decode::u64(&bytes[pos..]).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "malformed varint in gram set",
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
                        "delta overflow in gram set",
                    )
                })?
            };
            out.push(Gram::from_ordinal(width, value)?);
            prev = value;
        }
        Self::new(out)
    }
}

/// Memory-mapped view of per-file gram sets.
#[derive(Debug)]
pub struct GramSets {
    width: GramWidth,
    data: ArtifactData,
    count: usize,
    offset_table_start: usize,
}

impl GramSets {
    /// Encode gram sets into the on-disk file format.
    ///
    /// # Errors
    ///
    /// Returns an error if offsets overflow or per-set encoding fails.
    pub fn encode(width: GramWidth, sets: &[GramSet]) -> std::io::Result<Vec<u8>> {
        let count = sets.len();
        let offset_table_start = GRAMS_MAGIC.len() + 12;
        let blob_start = offset_table_start + count * 8;

        let mut offsets = Vec::<u64>::with_capacity(count);
        let mut blob = Vec::<u8>::new();

        for set in sets {
            let abs_off = u64::try_from(blob_start + blob.len()).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "gram sets blob offset exceeds u64::MAX",
                )
            })?;
            offsets.push(abs_off);
            let blob_len = blob.len();
            set.encode_into(&mut blob).inspect_err(|_| {
                blob.truncate(blob_len);
            })?;
        }

        let mut file_bytes = Vec::with_capacity(blob_start + blob.len());
        file_bytes.extend_from_slice(&GRAMS_MAGIC);
        file_bytes.extend_from_slice(&width.as_u32().to_le_bytes());
        let count = u32::try_from(count).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "gram sets count exceeds u32::MAX",
            )
        })?;
        file_bytes.extend_from_slice(&count.to_le_bytes());
        file_bytes.extend_from_slice(&0u32.to_le_bytes());
        for off in &offsets {
            file_bytes.extend_from_slice(&off.to_le_bytes());
        }
        file_bytes.extend_from_slice(&blob);
        Ok(file_bytes)
    }

    /// Write gram sets to a file and return a memory-mapped instance.
    ///
    /// # Errors
    ///
    /// Returns an error if encoding, writing, or reopening fails.
    pub fn create(path: &Path, width: GramWidth, sets: &[GramSet]) -> std::io::Result<Self> {
        let data = Self::encode(width, sets)?;
        std::fs::write(path, &data)?;
        Self::open(path, width)
    }

    /// Open gram sets from a memory-mapped file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened or the format is invalid.
    pub fn open(path: &Path, width: GramWidth) -> std::io::Result<Self> {
        let mmap = mmap_open(path)?;
        Self::from_artifact(ArtifactData::Mmap(mmap), width)
    }

    /// Validate and wrap in-memory or mmap artifact bytes as gram sets.
    ///
    /// # Errors
    ///
    /// Returns an error if the header or offset table is invalid.
    pub fn from_artifact(data: ArtifactData, width: GramWidth) -> std::io::Result<Self> {
        let bytes = data.as_ref();
        let (count, offset_table_start) = Self::validate(bytes, width)?;
        Ok(Self {
            width,
            data,
            count,
            offset_table_start,
        })
    }

    fn validate(bytes: &[u8], width: GramWidth) -> std::io::Result<(usize, usize)> {
        let magic_len = GRAMS_MAGIC.len();
        if bytes.len() < magic_len + 12 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "gram sets too short for header",
            ));
        }
        if bytes[..magic_len] != GRAMS_MAGIC {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "unexpected gram sets magic",
            ));
        }
        let stored_width = read_u32_le(bytes, magic_len);
        if stored_width != width.as_u32() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "gram set width {stored_width} does not match expected {}",
                    width.get()
                ),
            ));
        }
        let count = read_u32_le(bytes, magic_len + 4) as usize;
        let offset_table_start = magic_len + 12;
        let blob_start = offset_table_start + count * 8;
        if bytes.len() < blob_start {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "gram sets too short for offset table",
            ));
        }
        let mut prev_off: Option<u64> = None;
        for i in 0..count {
            let off = read_u64_le(bytes, offset_table_start + i * 8);
            if off < u64::try_from(blob_start).unwrap_or(u64::MAX)
                || usize::try_from(off).map_or(true, |o| o > bytes.len())
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "gram set offset out of bounds",
                ));
            }
            if let Some(prev) = prev_off
                && off < prev
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "gram set offsets are not monotonic",
                ));
            }
            prev_off = Some(off);
        }
        Ok((count, offset_table_start))
    }

    /// Look up the gram set for `file_id`.
    ///
    /// # Errors
    ///
    /// Returns an error if `file_id` is out of bounds or the stored set is malformed.
    pub fn get(&self, file_id: usize) -> std::io::Result<GramSet> {
        if file_id >= self.count {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "file id out of bounds for gram sets",
            ));
        }
        let bytes = self.data.as_ref();
        let off = usize::try_from(read_u64_le(bytes, self.offset_table_start + file_id * 8))
            .map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "offset too large")
            })?;
        let end = if file_id + 1 < self.count {
            usize::try_from(read_u64_le(
                bytes,
                self.offset_table_start + (file_id + 1) * 8,
            ))
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "offset too large"))?
        } else {
            bytes.len()
        };
        if off > end || end > bytes.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "gram set range out of bounds",
            ));
        }
        GramSet::decode(self.width, &bytes[off..end])
    }
}

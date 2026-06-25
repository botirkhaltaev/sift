//! Per-file gram sets: file id -> sorted unique N-grams.

use std::cell::RefCell;
use std::marker::PhantomData;
use std::path::Path;

use super::format::GRAMS_MAGIC;
use super::{read_u32_le, read_u64_le};
use crate::index::mmap::mmap_open;
use crate::index::ngram::gram::{GramKey, Trigram};
use crate::index::snapshot::ArtifactData;

/// A single sorted unique gram set for one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GramSet<G: GramKey> {
    grams: Vec<G>,
}

impl<G: GramKey> GramSet<G> {
    pub fn new(grams: Vec<G>) -> std::io::Result<Self> {
        let mut prev: Option<G> = None;
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

    pub(crate) fn collect(bytes: &[u8]) -> Self {
        let width = G::WIDTH.get();
        if bytes.len() < width {
            return Self { grams: Vec::new() };
        }
        let mut grams: Vec<G> = (0..=bytes.len() - width)
            .map(|offset| G::from_window(&bytes[offset..offset + width]))
            .collect();
        grams.sort_unstable();
        grams.dedup();
        Self { grams }
    }

    pub fn as_slice(&self) -> &[G] {
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

    pub fn decode(bytes: &[u8]) -> std::io::Result<Self> {
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
            out.push(G::from_ordinal(value)?);
            prev = value;
        }
        Self::new(out)
    }
}

impl GramSet<Trigram> {
    /// Optimized trigram extraction using a thread-local 24-bit bitset.
    pub(crate) fn collect_trigrams(bytes: &[u8]) -> Self {
        if bytes.len() < 3 {
            return Self { grams: Vec::new() };
        }

        thread_local! {
            static SEEN: RefCell<Vec<u64>> = RefCell::new(vec![0u64; 1 << 18]);
        }

        SEEN.with(|cell| {
            let mut seen = cell.borrow_mut();
            let mut grams = Vec::new();
            let mut key =
                (u32::from(bytes[0]) << 16) | (u32::from(bytes[1]) << 8) | u32::from(bytes[2]);
            {
                let idx = key as usize;
                seen[idx >> 6] |= 1u64 << (idx & 63);
                grams.push(Trigram::from_u24(key));
            }
            for &b in &bytes[3..] {
                key = ((key & 0x0000_FFFF) << 8) | u32::from(b);
                let idx = key as usize;
                let word = &mut seen[idx >> 6];
                let bit = 1u64 << (idx & 63);
                if *word & bit == 0 {
                    *word |= bit;
                    grams.push(Trigram::from_u24(key));
                }
            }
            for gram in &grams {
                let idx = gram.as_u24() as usize;
                seen[idx >> 6] &= !(1u64 << (idx & 63));
            }
            grams.sort_unstable();
            Self { grams }
        })
    }
}

/// Memory-mapped view of per-file gram sets.
#[derive(Debug)]
pub struct GramSets<G: GramKey> {
    data: ArtifactData,
    count: usize,
    offset_table_start: usize,
    gram_type: PhantomData<fn() -> G>,
}

impl<G: GramKey> GramSets<G> {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            data: ArtifactData::Memory(Vec::new().into()),
            count: 0,
            offset_table_start: 0,
            gram_type: PhantomData,
        }
    }

    pub fn encode(sets: &[GramSet<G>]) -> std::io::Result<Vec<u8>> {
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
        file_bytes.extend_from_slice(&G::WIDTH.as_u32().to_le_bytes());
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

    pub fn create(path: &Path, sets: &[GramSet<G>]) -> std::io::Result<Self> {
        let data = Self::encode(sets)?;
        std::fs::write(path, &data)?;
        Self::open(path)
    }

    pub fn open(path: &Path) -> std::io::Result<Self> {
        let mmap = mmap_open(path)?;
        Self::from_artifact(ArtifactData::Mmap(mmap))
    }

    pub fn from_artifact(data: ArtifactData) -> std::io::Result<Self> {
        let bytes = data.as_ref();
        let (count, offset_table_start) = Self::validate(bytes)?;
        Ok(Self {
            data,
            count,
            offset_table_start,
            gram_type: PhantomData,
        })
    }

    fn validate(bytes: &[u8]) -> std::io::Result<(usize, usize)> {
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
        if stored_width != G::WIDTH.as_u32() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "gram set width {stored_width} does not match expected {}",
                    G::WIDTH.get()
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
            let off_usize = usize::try_from(off).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("gram set offset[{i}] exceeds address space"),
                )
            })?;
            if off_usize > bytes.len() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("offset table[{i}] points past end"),
                ));
            }
            if off_usize < blob_start {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("offset table[{i}] points before blob start"),
                ));
            }
            if let Some(prev) = prev_off
                && off < prev
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("offset table[{i}] not monotonic (prev={prev}, cur={off})"),
                ));
            }
            prev_off = Some(off);
        }
        Ok((count, offset_table_start))
    }

    fn blob_end(&self, id: usize) -> usize {
        let bytes = self.bytes();
        if id + 1 < self.count {
            let off_start = self.offset_table_start + (id + 1) * 8;
            usize::try_from(read_u64_le(bytes, off_start)).unwrap_or(bytes.len())
        } else {
            bytes.len()
        }
    }

    fn bytes(&self) -> &[u8] {
        self.data.as_ref()
    }

    pub fn get(&self, id: usize) -> std::io::Result<GramSet<G>> {
        if id >= self.count {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("gram set index {id} out of range (count={})", self.count),
            ));
        }
        let bytes = self.bytes();
        let off_start = self.offset_table_start + id * 8;
        let off = usize::try_from(read_u64_le(bytes, off_start)).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("gram set {id} offset exceeds address space"),
            )
        })?;
        let end = self.blob_end(id);
        if off > end || end > bytes.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("gram set {id} data extends past end"),
            ));
        }
        GramSet::decode(&bytes[off..end])
    }
}

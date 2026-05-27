//! Per-file trigram sets: file id → sorted unique trigrams (delta-varint encoded).
//!
//! Format (SIFTTRI2):
//!   magic (8) | count (4) | offsets[count] (8*count) | blob
//!
//! Each entry in the blob:
//!   delta-varint encoded sorted 24-bit trigram values

use std::path::Path;

use memmap2::Mmap;

use super::delta::SortedDeltaCodec;
use super::format::TRIGRAMS_MAGIC;
use super::mmap::open_mmap;
use crate::index::trigram::Trigram;

struct TrigramSetCodec;

impl SortedDeltaCodec for TrigramSetCodec {
    type Item = Trigram;

    fn encode_sorted(out: &mut Vec<u8>, values: &[Trigram]) -> std::io::Result<()> {
        let mut prev = 0u64;
        for (i, tri) in values.iter().enumerate() {
            let val = u64::from(tri.as_u24());
            let raw = if i == 0 {
                val
            } else {
                val.checked_sub(prev).ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "non-monotonic trigram set",
                    )
                })?
            };
            let mut buf = unsigned_varint::encode::u64_buffer();
            let encoded = unsigned_varint::encode::u64(raw, &mut buf);
            out.extend_from_slice(encoded);
            prev = val;
        }
        Ok(())
    }

    fn decode_sorted(bytes: &[u8]) -> std::io::Result<Vec<Trigram>> {
        let mut out = Vec::new();
        let mut pos = 0usize;
        let mut prev = 0u64;
        while pos < bytes.len() {
            let (raw, remaining) = unsigned_varint::decode::u64(&bytes[pos..]).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "malformed varint in trigram set",
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
                        "delta overflow in trigram set",
                    )
                })?
            };
            let value_u32 = u32::try_from(value).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "trigram value exceeds 32-bit range",
                )
            })?;
            if value_u32 > 0x00FF_FFFF {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "trigram value exceeds 24-bit range",
                ));
            }
            let b = value_u32.to_be_bytes();
            out.push(Trigram::from_bytes([b[1], b[2], b[3]]));
            prev = value;
        }
        Ok(out)
    }
}

/// Memory-mapped view of per-file trigram sets.
#[derive(Debug)]
pub struct TrigramSets {
    mmap: Mmap,
    count: usize,
    offset_table_start: usize,
}

fn build_sets_bytes(sets: &[Vec<Trigram>]) -> std::io::Result<Vec<u8>> {
    let count = sets.len();
    let offset_table_start = TRIGRAMS_MAGIC.len() + 4;
    let blob_start = offset_table_start + count * 8;

    let mut offsets = Vec::<u64>::with_capacity(count);
    let mut blob = Vec::<u8>::new();

    for tris in sets {
        let abs_off = u64::try_from(blob_start + blob.len()).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "trigram sets blob offset exceeds u64::MAX",
            )
        })?;
        offsets.push(abs_off);
        TrigramSetCodec::encode_sorted(&mut blob, tris).expect("encode trigram set");
    }

    let mut file_bytes = Vec::with_capacity(blob_start + blob.len());
    file_bytes.extend_from_slice(&TRIGRAMS_MAGIC);
    file_bytes.extend_from_slice(
        &u32::try_from(count)
            .map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "trigram sets count exceeds u32::MAX",
                )
            })?
            .to_le_bytes(),
    );
    for off in &offsets {
        file_bytes.extend_from_slice(&off.to_le_bytes());
    }
    file_bytes.extend_from_slice(&blob);
    Ok(file_bytes)
}

impl TrigramSets {
    /// Write a trigram-sets file and return an mmap-backed instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written or reopened.
    pub fn create(path: &Path, sets: &[Vec<Trigram>]) -> std::io::Result<Self> {
        let data = build_sets_bytes(sets)?;
        std::fs::write(path, &data)?;
        Self::open(path)
    }

    pub fn open(path: &Path) -> std::io::Result<Self> {
        let mmap = open_mmap(path)?;
        let bytes = mmap.as_ref();
        let (count, offset_table_start) = Self::validate(bytes)?;
        Ok(Self {
            mmap,
            count,
            offset_table_start,
        })
    }

    fn validate(bytes: &[u8]) -> std::io::Result<(usize, usize)> {
        let magic_len = TRIGRAMS_MAGIC.len();
        if bytes.len() < magic_len + 4 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "trigram sets too short for magic+count",
            ));
        }
        if bytes[..magic_len] != TRIGRAMS_MAGIC {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "unexpected trigram sets magic",
            ));
        }
        let count =
            u32::from_le_bytes(bytes[magic_len..magic_len + 4].try_into().unwrap()) as usize;
        let offset_table_start = magic_len + 4;
        let blob_start = offset_table_start + count * 8;
        if bytes.len() < blob_start {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "trigram sets too short for offset table",
            ));
        }
        let mut prev_off: Option<u64> = None;
        for i in 0..count {
            let off = u64::from_le_bytes(
                bytes[offset_table_start + i * 8..offset_table_start + (i + 1) * 8]
                    .try_into()
                    .unwrap(),
            );
            let off_usize = usize::try_from(off).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("trigram set offset[{i}] exceeds address space"),
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
        // Validate each set's payload: decode and verify 24-bit bounds.
        for i in 0..count {
            let start_off = u64::from_le_bytes(
                bytes[offset_table_start + i * 8..offset_table_start + (i + 1) * 8]
                    .try_into()
                    .unwrap(),
            );
            let start = usize::try_from(start_off).expect("validated above");
            let end = if i + 1 < count {
                let next_off = u64::from_le_bytes(
                    bytes[offset_table_start + (i + 1) * 8..offset_table_start + (i + 2) * 8]
                        .try_into()
                        .unwrap(),
                );
                usize::try_from(next_off).expect("validated above")
            } else {
                bytes.len()
            };
            let set_bytes = bytes.get(start..end).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("trigram set {i} data out of range"),
                )
            })?;
            Self::validate_set(set_bytes)?;
        }
        Ok((count, offset_table_start))
    }

    /// Walk a single delta-encoded trigram set slice without allocating.
    /// Rejects malformed varints and values exceeding 24-bit range.
    fn validate_set(bytes: &[u8]) -> std::io::Result<()> {
        let mut pos = 0usize;
        let mut prev = 0u64;
        let mut count = 0usize;
        while pos < bytes.len() {
            let (raw, remaining) = unsigned_varint::decode::u64(&bytes[pos..]).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "malformed varint in trigram set",
                )
            })?;
            let consumed = bytes[pos..].len().saturating_sub(remaining.len());
            let value = if count == 0 {
                raw
            } else {
                prev.checked_add(raw).ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "delta overflow in trigram set",
                    )
                })?
            };
            if value > 0x00FF_FFFF {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "trigram value exceeds 24-bit range",
                ));
            }
            prev = value;
            pos += consumed;
            count += 1;
        }
        Ok(())
    }

    fn blob_end(&self, id: usize) -> usize {
        let bytes = self.bytes();
        if id + 1 < self.count {
            let off_start = self.offset_table_start + (id + 1) * 8;
            usize::try_from(u64::from_le_bytes(
                bytes[off_start..off_start + 8].try_into().unwrap(),
            ))
            .unwrap_or(bytes.len())
        } else {
            bytes.len()
        }
    }

    fn bytes(&self) -> &[u8] {
        self.mmap.as_ref()
    }

    pub fn to_sets(&self) -> std::io::Result<Vec<Vec<Trigram>>> {
        let mut out = Vec::with_capacity(self.count);
        let bytes = self.bytes();
        for id in 0..self.count {
            let off_start = self.offset_table_start + id * 8;
            let off = usize::try_from(u64::from_le_bytes(
                bytes[off_start..off_start + 8].try_into().unwrap(),
            ))
            .map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("trigram set {id} offset exceeds address space"),
                )
            })?;
            let end = self.blob_end(id);
            if off > end || end > bytes.len() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("trigram set {id} data extends past end"),
                ));
            }
            let tris = TrigramSetCodec::decode_sorted(&bytes[off..end])?;
            out.push(tris);
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
        let path = tmp.path().join("trigrams.bin");
        let sets = vec![
            vec![Trigram::from_bytes(*b"abc"), Trigram::from_bytes(*b"def")],
            vec![Trigram::from_bytes(*b"xyz")],
            vec![],
        ];
        let ts = TrigramSets::create(&path, &sets).expect("create");
        let round_tripped = ts.to_sets().expect("decode sets");
        assert_eq!(round_tripped, sets);
    }

    #[test]
    fn empty_sets_roundtrips() {
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("trigrams.bin");
        let ts = TrigramSets::create(&path, &[]).expect("create");
        let round_tripped = ts.to_sets().expect("decode sets");
        assert!(round_tripped.is_empty());
    }

    #[test]
    fn open_rejects_bad_magic() {
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("trigrams.bin");
        let mut file_bytes = b"BADMAGIC".to_vec();
        file_bytes.extend_from_slice(&0u32.to_le_bytes());
        std::fs::write(&path, &file_bytes).expect("write file");

        let result = TrigramSets::open(&path);
        assert!(result.is_err());
    }

    #[test]
    fn open_rejects_offset_before_blob_start() {
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("trigrams.bin");
        let mut data = TRIGRAMS_MAGIC.to_vec();
        // count=1, offset=0 (before blob_start which is magic+4+8=20)
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&0u64.to_le_bytes());
        std::fs::write(&path, &data).expect("write");
        let result = TrigramSets::open(&path);
        assert!(result.is_err());
    }

    #[test]
    fn open_rejects_non_monotonic_offsets() {
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("trigrams.bin");
        let mut data = TRIGRAMS_MAGIC.to_vec();
        data.extend_from_slice(&2u32.to_le_bytes()); // count=2
        data.extend_from_slice(&100u64.to_le_bytes()); // off[0]=100
        data.extend_from_slice(&50u64.to_le_bytes()); // off[1]=50 (before 100!)
        // pad to make file at least 100 bytes so offsets are in bounds
        data.resize(150, 0);
        std::fs::write(&path, &data).expect("write");
        let result = TrigramSets::open(&path);
        assert!(result.is_err());
    }

    #[test]
    fn open_rejects_offset_past_end() {
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("trigrams.bin");
        let mut data = TRIGRAMS_MAGIC.to_vec();
        data.extend_from_slice(&1u32.to_le_bytes()); // count=1
        data.extend_from_slice(&999u64.to_le_bytes()); // offset past end
        data.resize(100, 0);
        std::fs::write(&path, &data).expect("write");
        let result = TrigramSets::open(&path);
        assert!(result.is_err());
    }

    #[test]
    fn open_rejects_truncated_varint_in_set_payload() {
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("trigrams.bin");
        let blob_start = TRIGRAMS_MAGIC.len() + 4 + 8; // magic + count + 1 offset
        let mut data = TRIGRAMS_MAGIC.to_vec();
        data.extend_from_slice(&1u32.to_le_bytes()); // count=1
        data.extend_from_slice(&u64::try_from(blob_start).unwrap().to_le_bytes());
        data.extend_from_slice(&[0x80, 0x80]); // truncated varint
        std::fs::write(&path, &data).expect("write");
        let result = TrigramSets::open(&path);
        assert!(result.is_err());
    }

    #[test]
    fn open_rejects_value_exceeding_24bit_in_set_payload() {
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("trigrams.bin");
        let blob_start = TRIGRAMS_MAGIC.len() + 4 + 8;
        let mut data = TRIGRAMS_MAGIC.to_vec();
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&u64::try_from(blob_start).unwrap().to_le_bytes());
        let mut buffer = unsigned_varint::encode::u64_buffer();
        let encoded = unsigned_varint::encode::u64(0x01_00_00_00, &mut buffer);
        data.extend_from_slice(encoded);
        std::fs::write(&path, &data).expect("write");
        let result = TrigramSets::open(&path);
        assert!(result.is_err());
    }
}

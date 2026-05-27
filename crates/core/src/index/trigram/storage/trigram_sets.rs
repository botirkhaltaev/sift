//! Per-file trigram sets: file id → sorted unique trigrams (delta-varint encoded).
//!
//! Format (SIFTTRI2):
//!   magic (8) | count (4) | offsets[count] (8*count) | blob
//!
//! Each entry in the blob:
//!   delta-varint encoded sorted 24-bit trigram values

use std::path::Path;

use memmap2::Mmap;

use super::format::TRIGRAMS_MAGIC;
use super::mmap::open_mmap;
use super::varint;
use crate::index::trigram::types::Trigram;

/// Memory-mapped view of per-file trigram sets.
#[derive(Debug)]
pub struct MappedTrigramSets {
    backing: Backing,
    count: usize,
    offset_table_start: usize,
}

#[derive(Debug)]
enum Backing {
    Mmap(Mmap),
    Owned(Vec<u8>),
}

impl MappedTrigramSets {
    fn bytes(&self) -> &[u8] {
        match &self.backing {
            Backing::Mmap(m) => m.as_ref(),
            Backing::Owned(v) => v.as_slice(),
        }
    }

    pub fn open(path: &Path) -> std::io::Result<Self> {
        let mmap = open_mmap(path)?;
        let bytes = mmap.as_ref();
        let (count, offset_table_start) = Self::validate(bytes)?;
        Ok(Self {
            backing: Backing::Mmap(mmap),
            count,
            offset_table_start,
        })
    }

    pub fn from_sets(sets: &[Vec<Trigram>]) -> Self {
        let count = sets.len();
        let offset_table_start = TRIGRAMS_MAGIC.len() + 4;
        let blob_start = offset_table_start + count * 8;

        let mut offsets = Vec::<u64>::with_capacity(count);
        let mut blob = Vec::<u8>::new();

        for tris in sets {
            let abs_off = u64::try_from(blob_start + blob.len()).unwrap_or(u64::MAX);
            offsets.push(abs_off);
            let values: Vec<u32> = tris.iter().map(|t| t.as_u24()).collect();
            varint::encode_sorted_deltas(&mut blob, &values);
        }

        let mut file_bytes = Vec::with_capacity(blob_start + blob.len());
        file_bytes.extend_from_slice(&TRIGRAMS_MAGIC);
        file_bytes.extend_from_slice(&u32::try_from(count).unwrap().to_le_bytes());
        for off in &offsets {
            file_bytes.extend_from_slice(&off.to_le_bytes());
        }
        file_bytes.extend_from_slice(&blob);

        Self {
            backing: Backing::Owned(file_bytes),
            count,
            offset_table_start,
        }
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
        Ok((count, offset_table_start))
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
            let slice = &bytes[off..end];
            let values = varint::decode_sorted_deltas::<u32>(slice);
            let tris: Vec<Trigram> = values
                .into_iter()
                .map(|v| {
                    let b = v.to_be_bytes();
                    Trigram::from_bytes([b[1], b[2], b[3]])
                })
                .collect();
            out.push(tris);
        }
        Ok(out)
    }

    pub fn backing_slice(&self) -> &[u8] {
        self.bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_sets_round_trips() {
        let sets = vec![
            vec![Trigram::from_bytes(*b"abc"), Trigram::from_bytes(*b"def")],
            vec![Trigram::from_bytes(*b"xyz")],
            vec![],
        ];
        let mapped = MappedTrigramSets::from_sets(&sets);
        let round_tripped = mapped.to_sets().expect("decode sets");
        assert_eq!(round_tripped, sets);
    }

    #[test]
    fn empty_sets_round_trips() {
        let mapped = MappedTrigramSets::from_sets(&[]);
        let round_tripped = mapped.to_sets().expect("decode sets");
        assert!(round_tripped.is_empty());
    }

    #[test]
    fn backing_slice_starts_with_magic() {
        let sets = vec![vec![Trigram::from_bytes(*b"abc")]];
        let mapped = MappedTrigramSets::from_sets(&sets);
        let slice = mapped.backing_slice();
        assert_eq!(&slice[..8], &TRIGRAMS_MAGIC);
    }

    #[test]
    fn open_rejects_bad_magic() {
        let tmp = tempfile::TempDir::new().expect("create temp dir");
        let path = tmp.path().join("trigrams.bin");
        let mut file_bytes = b"BADMAGIC".to_vec();
        file_bytes.extend_from_slice(&0u32.to_le_bytes());
        std::fs::write(&path, &file_bytes).expect("write file");

        let result = MappedTrigramSets::open(&path);
        assert!(result.is_err());
    }
}

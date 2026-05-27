//! Contiguous `u32` LE file-id payloads referenced by the lexicon.

use std::path::Path;

use memmap2::Mmap;

use crate::index::trigram::storage::format::POSTINGS_MAGIC;
use crate::index::trigram::storage::mmap::open_mmap;

#[derive(Debug)]
pub struct MappedPostings {
    backing: Backing,
}

#[derive(Debug)]
enum Backing {
    Mmap(Mmap),
    Owned(Vec<u8>),
}

impl MappedPostings {
    fn bytes(&self) -> &[u8] {
        match &self.backing {
            Backing::Mmap(m) => m.as_ref(),
            Backing::Owned(v) => v.as_slice(),
        }
    }

    #[must_use]
    pub fn from_bytes(payload: &[u8]) -> Self {
        let mut data = Vec::with_capacity(POSTINGS_MAGIC.len() + 4 + payload.len());
        data.extend_from_slice(&POSTINGS_MAGIC);
        let plen = u32::try_from(payload.len()).unwrap_or(u32::MAX);
        data.extend_from_slice(&plen.to_le_bytes());
        data.extend_from_slice(payload);
        Self {
            backing: Backing::Owned(data),
        }
    }

    /// Open postings from a memory-mapped file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file is malformed.
    pub fn open(path: &Path) -> std::io::Result<Self> {
        let mmap = open_mmap(path)?;
        let bytes = mmap.as_ref();
        Self::validate(bytes)?;
        Ok(Self {
            backing: Backing::Mmap(mmap),
        })
    }

    fn validate(bytes: &[u8]) -> std::io::Result<()> {
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
        let plen = u32::from_le_bytes(bytes[magic_len..magic_len + 4].try_into().unwrap()) as usize;
        if bytes.len() < magic_len + 4 + plen {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "postings payload shorter than declared length",
            ));
        }
        Ok(())
    }

    #[must_use]
    pub fn payload_len(&self) -> usize {
        let payload_start = POSTINGS_MAGIC.len() + 4;
        self.bytes().len().saturating_sub(payload_start)
    }

    #[must_use]
    pub fn slice(&self, start: usize, len: usize) -> &[u8] {
        let payload_start = POSTINGS_MAGIC.len() + 4;
        let start = payload_start + start;
        self.bytes().get(start..start + len).unwrap_or(&[])
    }

    #[must_use]
    pub fn backing_slice(&self) -> &[u8] {
        self.bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn from_bytes_stores_payload_after_header() {
        let payload = vec![1, 2, 3, 4, 5, 6, 7, 8];
        let postings = MappedPostings::from_bytes(&payload);
        let slice = postings.slice(0, payload.len());
        assert_eq!(slice, payload.as_slice());
    }

    #[test]
    fn slice_returns_requested_range() {
        let payload: Vec<u8> = (0..16).collect();
        let postings = MappedPostings::from_bytes(&payload);
        let slice = postings.slice(4, 8);
        assert_eq!(slice, &payload[4..12]);
    }

    #[test]
    fn slice_returns_empty_for_out_of_range() {
        let payload = vec![1, 2, 3, 4];
        let postings = MappedPostings::from_bytes(&payload);
        let slice = postings.slice(100, 10);
        assert!(slice.is_empty());
    }

    #[test]
    fn backing_slice_starts_with_postings_magic() {
        let postings = MappedPostings::from_bytes(&[0, 0, 0, 0]);
        let slice = postings.backing_slice();
        assert_eq!(&slice[..POSTINGS_MAGIC.len()], POSTINGS_MAGIC);
    }

    #[test]
    fn open_rejects_bad_magic() {
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("postings.bin");
        let mut file = std::fs::File::create(&path).expect("create file");
        file.write_all(b"BADMAGIC").expect("write bad magic");
        file.write_all(&0u32.to_le_bytes()).expect("write length");

        let result = MappedPostings::open(&path);
        assert!(result.is_err());
    }

    #[test]
    fn open_rejects_declared_payload_longer_than_file() {
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("postings.bin");
        let mut file = std::fs::File::create(&path).expect("create file");
        file.write_all(&POSTINGS_MAGIC).expect("write magic");
        file.write_all(&100u32.to_le_bytes())
            .expect("write length 100");
        file.write_all(&[0u8; 4]).expect("write only 4 bytes");

        let result = MappedPostings::open(&path);
        assert!(result.is_err());
    }

    #[test]
    fn open_rejects_truncated_magic() {
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("postings.bin");
        std::fs::write(&path, b"SHORT").expect("write short file");

        let result = MappedPostings::open(&path);
        assert!(result.is_err());
    }
}

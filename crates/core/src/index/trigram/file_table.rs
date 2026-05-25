//! File table: sequential file id → relative path + fingerprint.
//!
//! Format (SIFTFIL1):
//!   magic (8) | count (4) | offsets[count] (4*count) | blob
//!
//! Each entry in the blob:
//!   `path_len` (4) | `path_bytes` (`path_len`) | `mtime_secs` (8, i64 LE) | `size` (8, u64 LE)
//!
//! `get(id)` is O(1) — two array indexing ops and one slice decode.

use std::path::{Path, PathBuf};

use memmap2::Mmap;

use crate::index::trigram::storage::format::FILES_MAGIC;
use crate::index::trigram::storage::mmap::open_mmap;

/// Per-file fingerprint for change detection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileFingerprint {
    pub path: PathBuf,
    pub mtime_secs: i64,
    pub size: u64,
}

#[derive(Debug)]
pub struct MappedFilesView {
    backing: Backing,
    count: usize,
    offset_table_start: usize,
}

#[derive(Debug)]
enum Backing {
    Mmap(Mmap),
    Owned(Vec<u8>),
}

impl MappedFilesView {
    const FINGERPRINT_LEN: usize = 16; // i64 mtime + u64 size

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

    pub fn from_fingerprints(fingerprints: &[FileFingerprint]) -> Self {
        let count = fingerprints.len();
        let offset_table_start = FILES_MAGIC.len() + 4;
        let blob_start = offset_table_start + count * 4;

        let mut offsets = Vec::<u32>::with_capacity(count);
        let mut blob = Vec::<u8>::new();

        for fp in fingerprints {
            let s = fp.path.to_string_lossy();
            let path_bytes = s.as_bytes();
            let path_len = u32::try_from(path_bytes.len()).unwrap_or(u32::MAX);
            let abs_off = u32::try_from(blob_start + blob.len()).unwrap_or(u32::MAX);
            offsets.push(abs_off);
            blob.extend_from_slice(&path_len.to_le_bytes());
            blob.extend_from_slice(path_bytes);
            blob.extend_from_slice(&fp.mtime_secs.to_le_bytes());
            blob.extend_from_slice(&fp.size.to_le_bytes());
        }

        let mut file_bytes = Vec::with_capacity(blob_start + blob.len());
        file_bytes.extend_from_slice(&FILES_MAGIC);
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
        let magic_len = FILES_MAGIC.len();
        if bytes.len() < magic_len + 4 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "files table too short for magic+count",
            ));
        }
        if bytes[..magic_len] != FILES_MAGIC {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "unexpected files table magic",
            ));
        }
        let count =
            u32::from_le_bytes(bytes[magic_len..magic_len + 4].try_into().unwrap()) as usize;
        let offset_table_start = magic_len + 4;
        let blob_start = offset_table_start + count * 4;
        if bytes.len() < blob_start {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "files table too short for offset table",
            ));
        }
        for i in 0..count {
            let off = u32::from_le_bytes(
                bytes[offset_table_start + i * 4..offset_table_start + (i + 1) * 4]
                    .try_into()
                    .unwrap(),
            ) as usize;
            if off < blob_start {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("offset table[{i}] points before blob start"),
                ));
            }
            if off + 4 > bytes.len() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("offset table[{i}] path_len prefix extends past end"),
                ));
            }
            let path_len = u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()) as usize;
            let entry_end = off + 4 + path_len + Self::FINGERPRINT_LEN;
            if entry_end > bytes.len() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("offset table[{i}] entry extends past end"),
                ));
            }
        }
        Ok((count, offset_table_start))
    }

    #[cfg(test)]
    pub const fn len(&self) -> usize {
        self.count
    }

    pub fn to_fingerprints(&self) -> std::io::Result<Vec<FileFingerprint>> {
        let mut out = Vec::with_capacity(self.count);
        let bytes = self.bytes();
        for id in 0..self.count {
            let off = u32::from_le_bytes(
                bytes[self.offset_table_start + id * 4..self.offset_table_start + (id + 1) * 4]
                    .try_into()
                    .unwrap(),
            ) as usize;
            let path_len = u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()) as usize;
            let path_start = off + 4;
            let path_end = path_start + path_len;
            let path_bytes = bytes.get(path_start..path_end).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("path {id} extends past files table end"),
                )
            })?;
            let path = std::str::from_utf8(path_bytes).map_err(|err| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("path {id} is not valid UTF-8: {err}"),
                )
            })?;

            let fp_start = path_end;
            let mtime_secs = i64::from_le_bytes(bytes[fp_start..fp_start + 8].try_into().unwrap());
            let size = u64::from_le_bytes(bytes[fp_start + 8..fp_start + 16].try_into().unwrap());

            out.push(FileFingerprint {
                path: PathBuf::from(path),
                mtime_secs,
                size,
            });
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
    use std::io::Write;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn sample_fingerprints() -> Vec<FileFingerprint> {
        vec![
            FileFingerprint {
                path: PathBuf::from("a.txt"),
                mtime_secs: 1_000_000,
                size: 42,
            },
            FileFingerprint {
                path: PathBuf::from("sub/b.txt"),
                mtime_secs: 2_000_000,
                size: 100,
            },
            FileFingerprint {
                path: PathBuf::from("sub/deep/c.txt"),
                mtime_secs: 3_000_000,
                size: 0,
            },
        ]
    }

    #[test]
    fn from_fingerprints_round_trips() {
        let fps = sample_fingerprints();
        let table = MappedFilesView::from_fingerprints(&fps);
        let round_tripped = table.to_fingerprints().expect("decode fingerprints");
        assert_eq!(round_tripped, fps);
    }

    #[test]
    fn empty_fingerprint_list_round_trips() {
        let table = MappedFilesView::from_fingerprints(&[]);
        assert_eq!(table.len(), 0);
        let round_tripped = table.to_fingerprints().expect("decode fingerprints");
        assert!(round_tripped.is_empty());
    }

    #[test]
    fn len_returns_count() {
        let fps = vec![
            FileFingerprint {
                path: PathBuf::from("a.txt"),
                mtime_secs: 0,
                size: 0,
            },
            FileFingerprint {
                path: PathBuf::from("b.txt"),
                mtime_secs: 0,
                size: 0,
            },
        ];
        let table = MappedFilesView::from_fingerprints(&fps);
        assert_eq!(table.len(), 2);
    }

    #[test]
    fn backing_slice_starts_with_file_magic() {
        use crate::index::trigram::storage::format::FILES_MAGIC;
        let fp = FileFingerprint {
            path: PathBuf::from("a.txt"),
            mtime_secs: 0,
            size: 0,
        };
        let table = MappedFilesView::from_fingerprints(&[fp]);
        let slice = table.backing_slice();
        assert_eq!(&slice[..FILES_MAGIC.len()], FILES_MAGIC);
    }

    #[test]
    fn open_rejects_bad_magic() {
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("files.bin");
        let mut file = std::fs::File::create(&path).expect("create file");
        file.write_all(b"BADMAGIC").expect("write bad magic");
        file.write_all(&0u32.to_le_bytes()).expect("write count");

        let result = MappedFilesView::open(&path);
        assert!(result.is_err());
    }

    #[test]
    fn open_rejects_truncated_offset_table() {
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("files.bin");
        let mut file = std::fs::File::create(&path).expect("create file");
        file.write_all(b"SIFTFIL1").expect("write magic");
        file.write_all(&1u32.to_le_bytes()).expect("write count 1");
        file.write_all(&[0u8; 2])
            .expect("write only 2 of 4 offset bytes");

        let result = MappedFilesView::open(&path);
        assert!(result.is_err());
    }

    #[test]
    fn open_rejects_entry_extending_past_end() {
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("files.bin");
        let mut file = std::fs::File::create(&path).expect("create file");
        file.write_all(b"SIFTFIL1").expect("write magic");
        file.write_all(&1u32.to_le_bytes()).expect("write count 1");
        file.write_all(&16u32.to_le_bytes())
            .expect("write offset pointing past end");
        file.write_all(&100u32.to_le_bytes())
            .expect("write path_len 100 but no data");

        let result = MappedFilesView::open(&path);
        assert!(result.is_err());
    }

    #[test]
    fn open_rejects_truncated_magic() {
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("files.bin");
        std::fs::write(&path, b"SHORT").expect("write short file");

        let result = MappedFilesView::open(&path);
        assert!(result.is_err());
    }
}

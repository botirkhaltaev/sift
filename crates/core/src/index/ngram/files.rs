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

use crate::index::mmap::mmap_open;
use crate::index::ngram::storage::format::FILES_MAGIC;
use crate::index::ngram::storage::{read_i64_le, read_u32_le, read_u64_le};
use crate::index::snapshot::ArtifactData;

/// Per-file fingerprint for change detection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileFingerprint {
    pub path: PathBuf,
    pub mtime_secs: i64,
    pub size: u64,
}

/// Borrowed file row from the on-disk table (no `PathBuf` allocation).
#[derive(Debug, Clone, Copy)]
pub struct FileRow<'a> {
    pub path: &'a str,
    pub size: u64,
}

/// Raw on-disk entry fields used by path/row accessors.
struct FileEntry<'a> {
    path: &'a [u8],
    size: u64,
}

#[derive(Debug)]
pub struct FileTable {
    data: ArtifactData,
    count: usize,
    offset_table_start: usize,
}

impl FileTable {
    const FINGERPRINT_LEN: usize = 16;

    pub fn encode(fingerprints: &[FileFingerprint]) -> std::io::Result<Vec<u8>> {
        let count = fingerprints.len();
        let offset_table_start = FILES_MAGIC.len() + 4;
        let blob_start = offset_table_start + count * 4;

        let mut offsets = Vec::<u32>::with_capacity(count);
        let mut blob = Vec::<u8>::new();

        for fp in fingerprints {
            let path_bytes = fp.path.to_string_lossy();
            let path_bytes = path_bytes.as_bytes();
            let path_len = u32::try_from(path_bytes.len()).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "file path exceeds u32::MAX",
                )
            })?;
            let abs_off = u32::try_from(blob_start + blob.len()).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "files blob offset exceeds u32::MAX",
                )
            })?;
            offsets.push(abs_off);
            blob.extend_from_slice(&path_len.to_le_bytes());
            blob.extend_from_slice(path_bytes);
            blob.extend_from_slice(&fp.mtime_secs.to_le_bytes());
            blob.extend_from_slice(&fp.size.to_le_bytes());
        }

        let mut file_bytes = Vec::with_capacity(blob_start + blob.len());
        file_bytes.extend_from_slice(&FILES_MAGIC);
        let count = u32::try_from(count).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "files count exceeds u32::MAX",
            )
        })?;
        file_bytes.extend_from_slice(&count.to_le_bytes());
        for off in &offsets {
            file_bytes.extend_from_slice(&off.to_le_bytes());
        }
        file_bytes.extend_from_slice(&blob);
        Ok(file_bytes)
    }

    fn bytes(&self) -> &[u8] {
        self.data.as_ref()
    }

    pub fn from_artifact(data: ArtifactData) -> std::io::Result<Self> {
        let bytes = data.as_ref();
        let (count, offset_table_start) = Self::validate(bytes)?;
        Ok(Self {
            data,
            count,
            offset_table_start,
        })
    }

    /// Write a file table and return an mmap-backed instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written or reopened.
    pub fn create(path: &Path, fingerprints: &[FileFingerprint]) -> std::io::Result<Self> {
        let data = Self::encode(fingerprints)?;
        std::fs::write(path, &data)?;
        Self::open(path)
    }

    pub fn open(path: &Path) -> std::io::Result<Self> {
        let mmap = mmap_open(path)?;
        Self::from_artifact(ArtifactData::Mmap(mmap))
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
        let count = read_u32_le(bytes, magic_len) as usize;
        let offset_table_start = magic_len + 4;
        let blob_start = offset_table_start + count * 4;
        if bytes.len() < blob_start {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "files table too short for offset table",
            ));
        }
        for i in 0..count {
            let off = read_u32_le(bytes, offset_table_start + i * 4) as usize;
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
            let path_len = read_u32_le(bytes, off) as usize;
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

    pub const fn len(&self) -> usize {
        self.count
    }

    /// Borrow the UTF-8 path bytes for `id` without allocating a `PathBuf`.
    pub fn path_bytes(&self, id: usize) -> std::io::Result<&[u8]> {
        Ok(self.entry(id)?.path)
    }

    /// Borrow path and size for `id` without decoding the full fingerprint table.
    pub fn row(&self, id: usize) -> std::io::Result<FileRow<'_>> {
        let entry = self.entry(id)?;
        let path = std::str::from_utf8(entry.path).map_err(|err| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("path {id} is not valid UTF-8: {err}"),
            )
        })?;
        Ok(FileRow {
            path,
            size: entry.size,
        })
    }

    fn entry(&self, id: usize) -> std::io::Result<FileEntry<'_>> {
        if id >= self.count {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "file id out of range",
            ));
        }
        let bytes = self.bytes();
        let off = read_u32_le(bytes, self.offset_table_start + id * 4) as usize;
        let path_len = read_u32_le(bytes, off) as usize;
        let path_start = off + 4;
        let path_end = path_start + path_len;
        let path = bytes.get(path_start..path_end).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("path {id} extends past files table end"),
            )
        })?;
        let size = read_u64_le(bytes, path_end + 8);
        Ok(FileEntry { path, size })
    }

    /// Validate stored paths without decoding them into `PathBuf`s.
    pub fn validate_paths(&self) -> std::io::Result<()> {
        for id in 0..self.count {
            let path = self.path_bytes(id)?;
            if path.is_empty()
                || path.starts_with(b"/")
                || path
                    .split(|&b| b == b'/' || b == b'\\')
                    .any(|component| component == b"..")
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("invalid file path in index at id {id}"),
                ));
            }
            std::str::from_utf8(path).map_err(|err| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("path {id} is not valid UTF-8: {err}"),
                )
            })?;
        }
        Ok(())
    }

    pub fn to_fingerprints(&self) -> std::io::Result<Vec<FileFingerprint>> {
        let mut out = Vec::with_capacity(self.count);
        let bytes = self.bytes();
        for id in 0..self.count {
            let off = read_u32_le(bytes, self.offset_table_start + id * 4) as usize;
            let path_len = read_u32_le(bytes, off) as usize;
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
            let mtime_secs = read_i64_le(bytes, fp_start);
            let size = read_u64_le(bytes, fp_start + 8);

            out.push(FileFingerprint {
                path: PathBuf::from(path),
                mtime_secs,
                size,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
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
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("files.bin");
        let fps = sample_fingerprints();
        let table = FileTable::create(&path, &fps).expect("create");
        let round_tripped = table.to_fingerprints().expect("decode fingerprints");
        assert_eq!(round_tripped, fps);
    }

    #[test]
    fn empty_fingerprint_list_round_trips() {
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("files.bin");
        let table = FileTable::create(&path, &[]).expect("create");
        assert_eq!(table.len(), 0);
        let round_tripped = table.to_fingerprints().expect("decode fingerprints");
        assert!(round_tripped.is_empty());
    }

    #[test]
    fn len_returns_count() {
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("files.bin");
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
        let table = FileTable::create(&path, &fps).expect("create");
        assert_eq!(table.len(), 2);
    }

    #[test]
    fn open_rejects_bad_magic() {
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("files.bin");
        let mut file = std::fs::File::create(&path).expect("create file");
        file.write_all(b"BADMAGIC").expect("write bad magic");
        file.write_all(&0u32.to_le_bytes()).expect("write count");

        let result = FileTable::open(&path);
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

        let result = FileTable::open(&path);
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

        let result = FileTable::open(&path);
        assert!(result.is_err());
    }

    #[test]
    fn open_rejects_truncated_magic() {
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("files.bin");
        std::fs::write(&path, b"SHORT").expect("write short file");

        let result = FileTable::open(&path);
        assert!(result.is_err());
    }
}

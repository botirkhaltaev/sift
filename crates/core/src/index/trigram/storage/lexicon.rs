//! Sorted trigram → postings slice descriptor.

use std::path::Path;

use memmap2::Mmap;

use crate::index::trigram::storage::format::LEXICON_MAGIC;
use crate::index::trigram::storage::mmap::open_mmap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexiconEntry {
    pub trigram: [u8; 3],
    pub offset: u64,
    pub len: u32,
}

/// Memory-mapped lexicon.
///
/// Binary search over on-disk sorted trigram table.
#[derive(Debug)]
pub struct Lexicon {
    mmap: Mmap,
    count: usize,
}

fn build_lexicon_bytes(entries: &[LexiconEntry]) -> std::io::Result<Vec<u8>> {
    let count = entries.len();
    let size = LEXICON_MAGIC.len() + 4 + count * Lexicon::ENTRY_SIZE;
    let mut data = vec![0u8; size];
    let mut cursor = 0;

    data[cursor..cursor + LEXICON_MAGIC.len()].copy_from_slice(&LEXICON_MAGIC);
    cursor += LEXICON_MAGIC.len();

    data[cursor..cursor + 4].copy_from_slice(
        &u32::try_from(count)
            .map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "lexicon entry count exceeds u32::MAX",
                )
            })?
            .to_le_bytes(),
    );
    cursor += 4;

    for e in entries {
        data[cursor..cursor + 3].copy_from_slice(&e.trigram);
        cursor += 3;
        data[cursor..cursor + 8].copy_from_slice(&e.offset.to_le_bytes());
        cursor += 8;
        data[cursor..cursor + 4].copy_from_slice(&e.len.to_le_bytes());
        cursor += 4;
    }
    Ok(data)
}

impl Lexicon {
    const ENTRY_SIZE: usize = 15;

    fn bytes(&self) -> &[u8] {
        self.mmap.as_ref()
    }

    /// Write a lexicon file and return an mmap-backed instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written or reopened.
    pub fn create(path: &Path, entries: &[LexiconEntry]) -> std::io::Result<Self> {
        let data = build_lexicon_bytes(entries)?;
        std::fs::write(path, &data)?;
        Self::open(path)
    }

    /// Open a lexicon from a memory-mapped file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file is malformed.
    pub fn open(path: &Path) -> std::io::Result<Self> {
        let mmap = open_mmap(path)?;
        let bytes = mmap.as_ref();
        let count = Self::validate(bytes)?;
        Ok(Self { mmap, count })
    }

    fn validate(bytes: &[u8]) -> std::io::Result<usize> {
        let magic_len = LEXICON_MAGIC.len();
        if bytes.len() < magic_len + 4 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "lexicon too short for magic+count",
            ));
        }
        if bytes[..magic_len] != LEXICON_MAGIC {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "unexpected lexicon magic",
            ));
        }
        let n = u32::from_le_bytes(bytes[magic_len..magic_len + 4].try_into().unwrap()) as usize;
        let expected_bytes = n * Self::ENTRY_SIZE;
        if bytes.len() < magic_len + 4 + expected_bytes {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "lexicon truncated",
            ));
        }
        let data_start = magic_len + 4;
        for i in 0..n {
            let offset = data_start + i * Self::ENTRY_SIZE;
            let tri: [u8; 3] = bytes[offset..offset + 3].try_into().unwrap();
            let posting_off =
                u64::from_le_bytes(bytes[offset + 3..offset + 11].try_into().unwrap());
            if i > 0 {
                let prev_offset = data_start + (i - 1) * Self::ENTRY_SIZE;
                let prev_tri: [u8; 3] = bytes[prev_offset..prev_offset + 3].try_into().unwrap();
                if tri <= prev_tri {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("lexicon trigram {tri:?} out of order (prev {prev_tri:?})",),
                    ));
                }
                let prev_posting_off = u64::from_le_bytes(
                    bytes[prev_offset + 3..prev_offset + 11].try_into().unwrap(),
                );
                if posting_off < prev_posting_off {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!(
                            "lexicon entry {tri:?} posting offset {posting_off} less than previous {prev_posting_off}",
                        ),
                    ));
                }
            }
        }
        Ok(n)
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.count
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Binary search for `tri` in the mapped lexicon. Returns the entry if found.
    ///
    /// # Panics
    ///
    /// Panics if the internal data layout is corrupted (out of bounds read).
    #[must_use]
    pub fn get(&self, tri: [u8; 3]) -> Option<LexiconEntry> {
        if self.count == 0 {
            return None;
        }
        let bytes = self.bytes();
        let magic_len = LEXICON_MAGIC.len();
        let data_start = magic_len + 4;

        let mut lo = 0;
        let mut hi = self.count;

        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let offset = data_start + mid * Self::ENTRY_SIZE;
            let entry_tri: [u8; 3] = bytes[offset..offset + 3].try_into().unwrap();

            match entry_tri.cmp(&tri) {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => {
                    let off =
                        u64::from_le_bytes(bytes[offset + 3..offset + 11].try_into().unwrap());
                    let len =
                        u32::from_le_bytes(bytes[offset + 11..offset + 15].try_into().unwrap());
                    return Some(LexiconEntry {
                        trigram: entry_tri,
                        offset: off,
                        len,
                    });
                }
            }
        }
        None
    }

    /// Byte offset in the postings payload where the list after `offset` ends.
    #[must_use]
    pub fn posting_byte_end(&self, offset: u64, payload_len: usize) -> usize {
        if self.count == 0 {
            return payload_len;
        }
        let bytes = self.bytes();
        let data_start = LEXICON_MAGIC.len() + 4;
        let mut lo = 0usize;
        let mut hi = self.count;
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let off = data_start + mid * Self::ENTRY_SIZE + 3;
            let entry_off =
                u64::from_le_bytes(bytes[off..off + 8].try_into().expect("entry offset"));
            if entry_off <= offset {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo < self.count {
            let off = data_start + lo * Self::ENTRY_SIZE + 3;
            usize::try_from(u64::from_le_bytes(
                bytes[off..off + 8].try_into().expect("entry offset"),
            ))
            .unwrap_or(payload_len)
        } else {
            payload_len
        }
    }

    #[must_use]
    pub const fn iter(&self) -> LexiconIter<'_> {
        LexiconIter {
            lexicon: self,
            pos: 0,
        }
    }
}

impl<'a> IntoIterator for &'a Lexicon {
    type Item = LexiconEntry;
    type IntoIter = LexiconIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        LexiconIter {
            lexicon: self,
            pos: 0,
        }
    }
}

pub struct LexiconIter<'_a> {
    lexicon: &'_a Lexicon,
    pos: usize,
}

impl Iterator for LexiconIter<'_> {
    type Item = LexiconEntry;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.lexicon.count {
            return None;
        }
        let bytes = self.lexicon.bytes();
        let magic_len = LEXICON_MAGIC.len();
        let offset = magic_len + 4 + self.pos * Lexicon::ENTRY_SIZE;
        let tri: [u8; 3] = bytes[offset..offset + 3].try_into().unwrap();
        let off = u64::from_le_bytes(bytes[offset + 3..offset + 11].try_into().unwrap());
        let len = u32::from_le_bytes(bytes[offset + 11..offset + 15].try_into().unwrap());
        self.pos += 1;
        Some(LexiconEntry {
            trigram: tri,
            offset: off,
            len,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn make_entry(tri: [u8; 3], offset: u64, len: u32) -> LexiconEntry {
        LexiconEntry {
            trigram: tri,
            offset,
            len,
        }
    }

    fn create_lexicon(entries: &[LexiconEntry]) -> Lexicon {
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("lexicon.bin");
        Lexicon::create(&path, entries).expect("create lexicon")
    }

    #[test]
    fn create_sets_len() {
        let entries = vec![make_entry(*b"abc", 0, 4), make_entry(*b"def", 4, 8)];
        let lexicon = create_lexicon(&entries);
        assert_eq!(lexicon.len(), 2);
    }

    #[test]
    fn empty_lexicon_reports_is_empty() {
        let lexicon = create_lexicon(&[]);
        assert!(lexicon.is_empty());
        assert_eq!(lexicon.len(), 0);
    }

    #[test]
    fn get_finds_first_middle_and_last() {
        let entries = vec![
            make_entry(*b"aaa", 0, 4),
            make_entry(*b"bbb", 4, 4),
            make_entry(*b"ccc", 8, 4),
        ];
        let lexicon = create_lexicon(&entries);
        assert!(lexicon.get(*b"aaa").is_some());
        assert!(lexicon.get(*b"bbb").is_some());
        assert!(lexicon.get(*b"ccc").is_some());
    }

    #[test]
    fn get_returns_none_for_absent_trigram() {
        let entries = vec![make_entry(*b"abc", 0, 4)];
        let lexicon = create_lexicon(&entries);
        assert!(lexicon.get(*b"xyz").is_none());
    }

    #[test]
    fn get_returns_none_for_empty_lexicon() {
        let lexicon = create_lexicon(&[]);
        assert!(lexicon.get(*b"abc").is_none());
    }

    #[test]
    fn open_rejects_duplicate_trigrams() {
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("lexicon.bin");
        // Manually write duplicate trigrams
        let mut data = LEXICON_MAGIC.to_vec();
        data.extend_from_slice(&2u32.to_le_bytes()); // count=2
        data.extend_from_slice(b"aaa");
        data.extend_from_slice(&0u64.to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(b"aaa"); // same trigram
        data.extend_from_slice(&1u64.to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes());
        std::fs::write(&path, &data).expect("write");
        let result = Lexicon::open(&path);
        assert!(result.is_err());
    }

    #[test]
    fn open_rejects_out_of_order_trigrams() {
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("lexicon.bin");
        let mut data = LEXICON_MAGIC.to_vec();
        data.extend_from_slice(&2u32.to_le_bytes()); // count=2
        data.extend_from_slice(b"bbb");
        data.extend_from_slice(&0u64.to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(b"aaa"); // out of order
        data.extend_from_slice(&1u64.to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes());
        std::fs::write(&path, &data).expect("write");
        let result = Lexicon::open(&path);
        assert!(result.is_err());
    }

    #[test]
    fn open_rejects_non_monotonic_posting_offsets() {
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("lexicon.bin");
        let mut data = LEXICON_MAGIC.to_vec();
        data.extend_from_slice(&2u32.to_le_bytes()); // count=2
        // First entry: trigram "aaa", offset 10
        data.extend_from_slice(b"aaa");
        data.extend_from_slice(&10u64.to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes());
        // Second entry: trigram "bbb", offset 5 (less than 10)
        data.extend_from_slice(b"bbb");
        data.extend_from_slice(&5u64.to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes());
        std::fs::write(&path, &data).expect("write");
        let result = Lexicon::open(&path);
        assert!(result.is_err());
    }

    #[test]
    fn iter_yields_entries_in_stored_order() {
        let entries = vec![
            make_entry(*b"aaa", 0, 4),
            make_entry(*b"bbb", 4, 4),
            make_entry(*b"ccc", 8, 4),
        ];
        let lexicon = create_lexicon(&entries);
        let collected: Vec<_> = lexicon.iter().collect();
        assert_eq!(collected.len(), 3);
        assert_eq!(collected[0].trigram, *b"aaa");
        assert_eq!(collected[1].trigram, *b"bbb");
        assert_eq!(collected[2].trigram, *b"ccc");
    }

    #[test]
    fn into_iterator_for_ref_works() {
        let entries = vec![make_entry(*b"abc", 0, 4)];
        let lexicon = create_lexicon(&entries);
        let collected: Vec<_> = (&lexicon).into_iter().collect();
        assert_eq!(collected.len(), 1);
        assert_eq!(collected[0].trigram, *b"abc");
    }

    #[test]
    fn open_rejects_bad_magic() {
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("lexicon.bin");
        let mut file = std::fs::File::create(&path).expect("create file");
        file.write_all(b"BADMAGIC").expect("write bad magic");
        file.write_all(&0u32.to_le_bytes()).expect("write count");

        let result = Lexicon::open(&path);
        assert!(result.is_err());
    }

    #[test]
    fn open_rejects_truncated_entry_data() {
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("lexicon.bin");
        let mut file = std::fs::File::create(&path).expect("create file");
        file.write_all(&LEXICON_MAGIC).expect("write magic");
        file.write_all(&1u32.to_le_bytes()).expect("write count 1");
        file.write_all(&[0u8; 8]).expect("write only 8 of 15 bytes");

        let result = Lexicon::open(&path);
        assert!(result.is_err());
    }

    #[test]
    fn open_rejects_truncated_magic() {
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("lexicon.bin");
        std::fs::write(&path, b"SHORT").expect("write short file");

        let result = Lexicon::open(&path);
        assert!(result.is_err());
    }

    #[test]
    fn get_returns_correct_offset_and_len() {
        let entries = vec![make_entry(*b"aaa", 100, 12), make_entry(*b"bbb", 200, 8)];
        let lexicon = create_lexicon(&entries);
        let entry = lexicon.get(*b"aaa").expect("find aaa");
        assert_eq!(entry.offset, 100);
        assert_eq!(entry.len, 12);
        let entry = lexicon.get(*b"bbb").expect("find bbb");
        assert_eq!(entry.offset, 200);
        assert_eq!(entry.len, 8);
    }
}

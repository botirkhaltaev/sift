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

/// Memory-mapped lexicon view with owning storage.
///
/// Binary search over on-disk sorted trigram table.
#[derive(Debug)]
pub struct MappedLexicon {
    backing: Backing,
    count: usize,
}

#[derive(Debug)]
enum Backing {
    Mmap(Mmap),
    Owned(Vec<u8>),
}

impl MappedLexicon {
    const ENTRY_SIZE: usize = 15;

    fn bytes(&self) -> &[u8] {
        match &self.backing {
            Backing::Mmap(m) => m.as_ref(),
            Backing::Owned(v) => v.as_slice(),
        }
    }

    /// Construct from a list of lexicon entries (used by the builder for in-memory index).
    ///
    /// # Panics
    ///
    /// Panics if `entries.len()` exceeds `u32::MAX`.
    #[must_use]
    pub fn from_entries(entries: &[LexiconEntry]) -> Self {
        let count = entries.len();
        let size = LEXICON_MAGIC.len() + 4 + count * Self::ENTRY_SIZE;
        let mut data = vec![0u8; size];
        let mut cursor = 0;

        data[cursor..cursor + LEXICON_MAGIC.len()].copy_from_slice(&LEXICON_MAGIC);
        cursor += LEXICON_MAGIC.len();

        data[cursor..cursor + 4].copy_from_slice(&u32::try_from(count).unwrap().to_le_bytes());
        cursor += 4;

        for e in entries {
            data[cursor..cursor + 3].copy_from_slice(&e.trigram);
            cursor += 3;
            data[cursor..cursor + 8].copy_from_slice(&e.offset.to_le_bytes());
            cursor += 8;
            data[cursor..cursor + 4].copy_from_slice(&e.len.to_le_bytes());
            cursor += 4;
        }

        Self {
            backing: Backing::Owned(data),
            count,
        }
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
        Ok(Self {
            backing: Backing::Mmap(mmap),
            count,
        })
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

    #[must_use]
    pub const fn iter(&self) -> MappedLexiconIter<'_> {
        MappedLexiconIter {
            lexicon: self,
            pos: 0,
        }
    }

    #[must_use]
    pub fn backing_slice(&self) -> &[u8] {
        self.bytes()
    }
}

impl<'a> IntoIterator for &'a MappedLexicon {
    type Item = LexiconEntry;
    type IntoIter = MappedLexiconIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        MappedLexiconIter {
            lexicon: self,
            pos: 0,
        }
    }
}

pub struct MappedLexiconIter<'_a> {
    lexicon: &'_a MappedLexicon,
    pos: usize,
}

impl Iterator for MappedLexiconIter<'_> {
    type Item = LexiconEntry;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.lexicon.count {
            return None;
        }
        let bytes = self.lexicon.bytes();
        let magic_len = LEXICON_MAGIC.len();
        let offset = magic_len + 4 + self.pos * MappedLexicon::ENTRY_SIZE;
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

    #[test]
    fn from_entries_sets_len() {
        let entries = vec![make_entry(*b"abc", 0, 4), make_entry(*b"def", 4, 8)];
        let lexicon = MappedLexicon::from_entries(&entries);
        assert_eq!(lexicon.len(), 2);
    }

    #[test]
    fn empty_lexicon_reports_is_empty() {
        let lexicon = MappedLexicon::from_entries(&[]);
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
        let lexicon = MappedLexicon::from_entries(&entries);
        assert!(lexicon.get(*b"aaa").is_some());
        assert!(lexicon.get(*b"bbb").is_some());
        assert!(lexicon.get(*b"ccc").is_some());
    }

    #[test]
    fn get_returns_none_for_absent_trigram() {
        let entries = vec![make_entry(*b"abc", 0, 4)];
        let lexicon = MappedLexicon::from_entries(&entries);
        assert!(lexicon.get(*b"xyz").is_none());
    }

    #[test]
    fn get_returns_none_for_empty_lexicon() {
        let lexicon = MappedLexicon::from_entries(&[]);
        assert!(lexicon.get(*b"abc").is_none());
    }

    #[test]
    fn iter_yields_entries_in_stored_order() {
        let entries = vec![
            make_entry(*b"aaa", 0, 4),
            make_entry(*b"bbb", 4, 4),
            make_entry(*b"ccc", 8, 4),
        ];
        let lexicon = MappedLexicon::from_entries(&entries);
        let collected: Vec<_> = lexicon.iter().collect();
        assert_eq!(collected.len(), 3);
        assert_eq!(collected[0].trigram, *b"aaa");
        assert_eq!(collected[1].trigram, *b"bbb");
        assert_eq!(collected[2].trigram, *b"ccc");
    }

    #[test]
    fn into_iterator_for_ref_works() {
        let entries = vec![make_entry(*b"abc", 0, 4)];
        let lexicon = MappedLexicon::from_entries(&entries);
        let collected: Vec<_> = (&lexicon).into_iter().collect();
        assert_eq!(collected.len(), 1);
        assert_eq!(collected[0].trigram, *b"abc");
    }

    #[test]
    fn backing_slice_starts_with_lexicon_magic() {
        let lexicon = MappedLexicon::from_entries(&[]);
        let slice = lexicon.backing_slice();
        assert_eq!(&slice[..LEXICON_MAGIC.len()], LEXICON_MAGIC);
    }

    #[test]
    fn open_rejects_bad_magic() {
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("lexicon.bin");
        let mut file = std::fs::File::create(&path).expect("create file");
        file.write_all(b"BADMAGIC").expect("write bad magic");
        file.write_all(&0u32.to_le_bytes()).expect("write count");

        let result = MappedLexicon::open(&path);
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

        let result = MappedLexicon::open(&path);
        assert!(result.is_err());
    }

    #[test]
    fn open_rejects_truncated_magic() {
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("lexicon.bin");
        std::fs::write(&path, b"SHORT").expect("write short file");

        let result = MappedLexicon::open(&path);
        assert!(result.is_err());
    }

    #[test]
    fn get_returns_correct_offset_and_len() {
        let entries = vec![make_entry(*b"aaa", 100, 12), make_entry(*b"bbb", 200, 8)];
        let lexicon = MappedLexicon::from_entries(&entries);
        let entry = lexicon.get(*b"aaa").expect("find aaa");
        assert_eq!(entry.offset, 100);
        assert_eq!(entry.len, 12);
        let entry = lexicon.get(*b"bbb").expect("find bbb");
        assert_eq!(entry.offset, 200);
        assert_eq!(entry.len, 8);
    }
}

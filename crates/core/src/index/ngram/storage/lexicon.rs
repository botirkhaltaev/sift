//! Sorted N-gram key -> postings slice descriptor.

use std::marker::PhantomData;
use std::path::Path;

use crate::index::mmap::mmap_open;
use crate::index::ngram::gram::GramKey;
use crate::index::ngram::storage::format::LEXICON_MAGIC;
use crate::index::snapshot::ArtifactData;

use super::{read_u32_le, read_u64_le};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexiconEntry<G: GramKey> {
    pub gram: G,
    pub offset: u64,
    pub len: u32,
}

/// Memory-mapped lexicon.
#[derive(Debug)]
pub struct Lexicon<G: GramKey> {
    data: ArtifactData,
    count: usize,
    gram_type: PhantomData<fn() -> G>,
}

impl<G: GramKey> Lexicon<G> {
    const HEADER_SIZE: usize = 12;

    #[must_use]
    pub fn empty() -> Self {
        Self {
            data: ArtifactData::Memory(Vec::new().into()),
            count: 0,
            gram_type: PhantomData,
        }
    }

    const fn entry_size() -> usize {
        G::WIDTH.get() + 12
    }

    pub fn encode(entries: &[LexiconEntry<G>]) -> std::io::Result<Vec<u8>> {
        let mut data = Vec::with_capacity(
            LEXICON_MAGIC.len() + Self::HEADER_SIZE + entries.len() * Self::entry_size(),
        );
        data.extend_from_slice(&LEXICON_MAGIC);
        data.extend_from_slice(&G::WIDTH.as_u32().to_le_bytes());
        let count = u32::try_from(entries.len()).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "lexicon entry count exceeds u32::MAX",
            )
        })?;
        data.extend_from_slice(&count.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        for e in entries {
            e.gram.write_bytes(&mut data);
            data.extend_from_slice(&e.offset.to_le_bytes());
            data.extend_from_slice(&e.len.to_le_bytes());
        }
        Ok(data)
    }

    fn bytes(&self) -> &[u8] {
        self.data.as_ref()
    }

    pub fn from_artifact(data: ArtifactData) -> std::io::Result<Self> {
        let bytes = data.as_ref();
        let count = Self::validate(bytes)?;
        Ok(Self {
            data,
            count,
            gram_type: PhantomData,
        })
    }

    pub fn create(path: &Path, entries: &[LexiconEntry<G>]) -> std::io::Result<Self> {
        let data = Self::encode(entries)?;
        std::fs::write(path, &data)?;
        Self::open(path)
    }

    pub fn open(path: &Path) -> std::io::Result<Self> {
        let mmap = mmap_open(path)?;
        Self::from_artifact(ArtifactData::Mmap(mmap))
    }

    fn validate(bytes: &[u8]) -> std::io::Result<usize> {
        let magic_len = LEXICON_MAGIC.len();
        if bytes.len() < magic_len + Self::HEADER_SIZE {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "lexicon too short for header",
            ));
        }
        if bytes[..magic_len] != LEXICON_MAGIC {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "unexpected lexicon magic",
            ));
        }
        let stored_width = read_u32_le(bytes, magic_len);
        if stored_width != G::WIDTH.as_u32() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "lexicon gram width {stored_width} does not match expected {}",
                    G::WIDTH.get()
                ),
            ));
        }
        let n = read_u32_le(bytes, magic_len + 4) as usize;
        let expected_bytes = n * Self::entry_size();
        if bytes.len() < magic_len + Self::HEADER_SIZE + expected_bytes {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "lexicon truncated",
            ));
        }
        let entries = &bytes[magic_len + Self::HEADER_SIZE..];
        let mut prev: Option<(G, u64)> = None;
        for chunk in entries.chunks_exact(Self::entry_size()) {
            let gram = G::read_bytes(&chunk[..G::WIDTH.get()])?;
            let posting_off = read_u64_le(chunk, G::WIDTH.get());
            if let Some((prev_gram, prev_off)) = prev {
                if gram <= prev_gram {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "lexicon grams out of order",
                    ));
                }
                if posting_off < prev_off {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "lexicon posting offsets are not monotonic",
                    ));
                }
            }
            prev = Some((gram, posting_off));
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

    #[must_use]
    pub fn get(&self, gram: G) -> Option<LexiconEntry<G>> {
        if self.count == 0 {
            return None;
        }
        let bytes = self.bytes();
        let data_start = LEXICON_MAGIC.len() + Self::HEADER_SIZE;
        let gram_width = G::WIDTH.get();
        let entry_size = Self::entry_size();

        let mut lo = 0;
        let mut hi = self.count;

        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let offset = data_start + mid * entry_size;
            let entry_gram = G::read_bytes(&bytes[offset..offset + gram_width])
                .expect("gram key validated at open");

            match entry_gram.cmp(&gram) {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => {
                    let off = read_u64_le(bytes, offset + gram_width);
                    let len = read_u32_le(bytes, offset + gram_width + 8);
                    return Some(LexiconEntry {
                        gram: entry_gram,
                        offset: off,
                        len,
                    });
                }
            }
        }
        None
    }

    #[must_use]
    pub fn posting_byte_end(&self, offset: u64, payload_len: usize) -> usize {
        if self.count == 0 {
            return payload_len;
        }
        let bytes = self.bytes();
        let data_start = LEXICON_MAGIC.len() + Self::HEADER_SIZE;
        let entry_size = Self::entry_size();
        let off_delta = G::WIDTH.get();
        let mut lo = 0usize;
        let mut hi = self.count;
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let off = data_start + mid * entry_size + off_delta;
            let entry_off = read_u64_le(bytes, off);
            if entry_off <= offset {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo < self.count {
            let off = data_start + lo * entry_size + off_delta;
            usize::try_from(read_u64_le(bytes, off)).unwrap_or(payload_len)
        } else {
            payload_len
        }
    }

    #[must_use]
    pub const fn iter(&self) -> LexiconIter<'_, G> {
        LexiconIter {
            lexicon: self,
            pos: 0,
        }
    }
}

impl<'a, G: GramKey> IntoIterator for &'a Lexicon<G> {
    type Item = LexiconEntry<G>;
    type IntoIter = LexiconIter<'a, G>;

    fn into_iter(self) -> Self::IntoIter {
        LexiconIter {
            lexicon: self,
            pos: 0,
        }
    }
}

pub struct LexiconIter<'a, G: GramKey> {
    lexicon: &'a Lexicon<G>,
    pos: usize,
}

impl<G: GramKey> Iterator for LexiconIter<'_, G> {
    type Item = LexiconEntry<G>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.lexicon.count {
            return None;
        }
        let bytes = self.lexicon.bytes();
        let offset =
            LEXICON_MAGIC.len() + Lexicon::<G>::HEADER_SIZE + self.pos * Lexicon::<G>::entry_size();
        let gram_width = G::WIDTH.get();
        let gram =
            G::read_bytes(&bytes[offset..offset + gram_width]).expect("gram key validated at open");
        let off = read_u64_le(bytes, offset + gram_width);
        let len = read_u32_le(bytes, offset + gram_width + 8);
        self.pos += 1;
        Some(LexiconEntry {
            gram,
            offset: off,
            len,
        })
    }
}

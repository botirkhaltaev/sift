//! Per-file trigram sets: file id → sorted unique trigrams (delta-varint encoded).
//!
//! Format (SIFTTRI2):
//!   magic (8) | count (4) | offsets[count] (8*count) | blob
//!
//! Each entry in the blob:
//!   delta-varint encoded sorted 24-bit trigram values

use crate::index::trigram::Trigram;

/// A single sorted unique set of trigrams for one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrigramSet {
    trigrams: Vec<Trigram>,
}

impl TrigramSet {
    pub fn new(trigrams: Vec<Trigram>) -> std::io::Result<Self> {
        let mut prev: Option<Trigram> = None;
        for t in &trigrams {
            if let Some(p) = prev
                && *t <= p
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "trigram set is not sorted and unique",
                ));
            }
            prev = Some(*t);
        }
        Ok(Self { trigrams })
    }

    pub fn as_slice(&self) -> &[Trigram] {
        &self.trigrams
    }
}

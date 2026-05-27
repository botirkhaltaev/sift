//! On-disk tables for the trigram index.

pub(super) mod delta;
pub mod format;
pub mod lexicon;
pub mod mmap;
pub mod postings;
pub mod trigram_sets;

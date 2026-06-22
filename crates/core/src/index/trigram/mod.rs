pub mod builder;
pub mod file_table;
pub mod key;
mod lifecycle;
mod search;
pub mod storage;

use std::path::{Path, PathBuf};

use crate::index::{CorpusKind, FileId};

use self::file_table::FileFingerprint;
pub use key::Trigram;

/// Errors specific to opening or persisting a trigram index.
#[derive(Debug, thiserror::Error)]
pub enum TrigramIndexError {
    #[error("index component missing: {0}")]
    MissingComponent(PathBuf),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Opened trigram index with memory-mapped posting lists.
///
/// A trigram index is an inverted index mapping every 3-byte sequence found
/// in the corpus to the set of files that contain it. At query time, required
/// literals are extracted from the regex pattern, decomposed into trigrams,
/// and intersected against the posting lists to produce a narrow candidate set.
///
/// This is the first shipped index type in Sift's composable index architecture.
/// It sits alongside future index types (AST, dependency graph, vector) as a
/// peer in the `Index` enum.
#[derive(Debug)]
pub struct TrigramIndex {
    root: PathBuf,
    pub(crate) fingerprints: Vec<FileFingerprint>,
    trigram_sets: storage::trigram_sets::TrigramSets,
    lexicon: storage::lexicon::Lexicon,
    postings: storage::postings::Postings,
    corpus_kind: CorpusKind,
}

impl TrigramIndex {
    #[must_use]
    pub fn file_path(&self, id: FileId) -> Option<&Path> {
        self.fingerprints.get(id.get()).map(|fp| fp.path.as_path())
    }

    #[must_use]
    pub fn file_abs_path(&self, id: FileId) -> Option<PathBuf> {
        self.fingerprints
            .get(id.get())
            .map(|fp| self.root.join(&fp.path))
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub const fn corpus_kind(&self) -> CorpusKind {
        self.corpus_kind
    }
}

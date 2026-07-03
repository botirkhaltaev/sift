use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::config::CorpusKind;
use super::error::IndexError;
use super::kinds::Index;
use super::paths::IndexedPaths;
use super::snapshot::{Snapshot, SnapshotId};
use super::store;

/// Registry of opened indexes read from a snapshot store.
///
/// Opens all index kinds found in the current snapshot and provides
/// query-time candidate narrowing by intersecting results from each index.
/// Multiple indexes together produce tighter narrowing than any single
/// index alone.
///
/// Owns a [`Snapshot`] that holds a reader lease, preventing the snapshot
/// from being garbage-collected while searches are active.
pub struct Indexes {
    snapshot: Snapshot,
}

impl Indexes {
    /// Create an Indexes registry from a single index and its root.
    ///
    /// Useful for testing and benchmarking.
    #[must_use]
    pub fn from_single(index: Index, root: PathBuf) -> Self {
        Self {
            snapshot: Snapshot::from_indexes(root, vec![index]),
        }
    }

    /// Open all indexes found under `sift_dir`.
    ///
    /// # Errors
    ///
    /// Returns [`IndexError::InvalidManifest`] if a snapshot manifest is
    /// malformed, or [`IndexError::Trigram`] if a trigram index is malformed.
    ///
    /// Returns an empty registry if no current snapshot exists (walk fallback).
    pub fn open(sift_dir: &Path) -> Result<Self, IndexError> {
        let store = store::IndexStore::open(sift_dir).map_err(|e| match e {
            crate::Error::Index(ie) => ie,
            crate::Error::Io(io) => IndexError::Io {
                path: sift_dir.to_path_buf(),
                source: io,
            },
            _ => IndexError::Io {
                path: sift_dir.to_path_buf(),
                source: std::io::Error::other(e.to_string()),
            },
        })?;

        let snapshot = store.open_current().map_err(|e| match e {
            crate::Error::Index(ie) => ie,
            crate::Error::Io(io) => IndexError::Io {
                path: sift_dir.to_path_buf(),
                source: io,
            },
            _ => IndexError::Io {
                path: sift_dir.to_path_buf(),
                source: std::io::Error::other(e.to_string()),
            },
        })?;

        Ok(Self { snapshot })
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.snapshot.is_empty()
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        self.snapshot.root()
    }

    #[must_use]
    pub const fn snapshot_id(&self) -> Option<&SnapshotId> {
        self.snapshot.id()
    }

    /// Produce narrowed candidates from all indexes that can narrow the query.
    ///
    /// Returns `None` if no index could narrow. When at least one index
    /// narrows, all narrowed candidate sets are intersected.
    #[must_use]
    pub fn candidates(
        &self,
        query: &crate::candidates::CandidateSpec<'_>,
    ) -> Option<Vec<crate::Candidate>> {
        let indexes = self.snapshot.indexes();
        match indexes.len() {
            0 => None,
            1 => indexes[0].candidates(query),
            _ => Self::candidates_multi(indexes, query),
        }
    }

    /// Corpus-relative paths present in the current snapshot.
    #[must_use]
    pub fn indexed_rel_paths(&self) -> HashSet<PathBuf> {
        self.indexed_paths().into_set()
    }

    #[must_use]
    pub(crate) fn indexed_paths(&self) -> IndexedPaths {
        IndexedPaths::from_indexes(self.snapshot.indexes())
    }

    /// Corpus-relative search hits not yet present in the current snapshot.
    #[must_use]
    pub fn unindexed_hits(&self, hits: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
        let indexed = self.indexed_paths();
        hits.into_iter()
            .filter(|path| !indexed.contains(path))
            .collect()
    }

    /// Return all indexed candidates across all registered indexes.
    #[must_use]
    pub(crate) fn complete_candidates(&self) -> Vec<crate::Candidate> {
        let indexes = self.snapshot.indexes();
        let mut iter = indexes.iter();
        let Some(first) = iter.next() else {
            return Vec::new();
        };

        let mut files = first.all_files();

        for index in iter {
            let next: HashSet<PathBuf> = index
                .all_files()
                .into_iter()
                .map(|c| c.rel_path().to_path_buf())
                .collect();
            files.retain(|c| next.contains(c.rel_path()));
            if files.is_empty() {
                break;
            }
        }

        files
    }

    /// Intersect candidates from multiple indexes.
    fn candidates_multi(
        indexes: &[Index],
        query: &crate::candidates::CandidateSpec<'_>,
    ) -> Option<Vec<crate::Candidate>> {
        use rayon::prelude::*;

        let sets: Vec<Vec<crate::Candidate>> = indexes
            .par_iter()
            .filter_map(|idx| idx.candidates(query))
            .collect();

        if sets.is_empty() {
            return None;
        }

        let mut result = sets.into_iter();
        let mut current = result.next()?;

        for next in result {
            let lookup: HashSet<&Path> = next.iter().map(crate::Candidate::rel_path).collect();
            current.retain(|c| lookup.contains(c.rel_path()));
            if current.is_empty() {
                break;
            }
        }

        Some(current)
    }

    #[must_use]
    pub fn first(&self) -> Option<&Index> {
        self.snapshot.indexes().first()
    }

    #[must_use]
    pub fn corpus_kind(&self) -> Option<CorpusKind> {
        let indexes = self.snapshot.indexes();
        let kind = indexes.first()?.corpus_kind();
        if indexes.iter().any(|idx| idx.corpus_kind() != kind) {
            return None;
        }
        Some(kind)
    }
}

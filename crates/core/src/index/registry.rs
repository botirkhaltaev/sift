use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::config::CorpusKind;
use super::error::IndexError;
use super::kinds::{CandidatePlan, Index};
use super::paths::IndexedCorpus;
use super::snapshot::{Snapshot, SnapshotId};
use super::store;
use crate::corpus::filter::CandidateFilter;
use crate::corpus::walk::FileWalk;

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

    /// Plan candidate coverage from all indexes that can narrow the query.
    #[must_use]
    pub fn plan(&self, query: &crate::candidates::CandidateSpec<'_>) -> CandidatePlan {
        let indexes = self.snapshot.indexes();
        match indexes.len() {
            0 => CandidatePlan::Unavailable,
            1 => indexes[0].plan(query),
            _ => Self::plan_multi(indexes, query),
        }
    }

    /// Corpus-relative paths present in the current snapshot.
    #[must_use]
    pub fn indexed_rel_paths(&self) -> HashSet<PathBuf> {
        self.indexed_paths().into_set()
    }

    fn indexed_paths(&self) -> IndexedCorpus {
        IndexedCorpus::from_indexes(self.snapshot.indexes())
    }

    /// Corpus-relative search hits not yet present in the current snapshot.
    #[must_use]
    pub fn unindexed_hits(&self, hits: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
        let indexed = self.indexed_paths();
        hits.into_iter()
            .filter(|path| !indexed.contains(path))
            .collect()
    }

    /// Candidates under `filter` whose paths are not present in the current snapshot.
    pub(crate) fn unindexed_candidates(
        &self,
        filter: &CandidateFilter,
    ) -> crate::Result<Vec<crate::Candidate>> {
        let indexed = self.indexed_paths();
        FileWalk::from_filter(filter).candidates_matching(indexed.unindexed_files())
    }

    /// Return all indexed candidates across all registered indexes.
    #[must_use]
    pub(crate) fn all_indexed_candidates(&self) -> Vec<crate::Candidate> {
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
    fn plan_multi(
        indexes: &[Index],
        query: &crate::candidates::CandidateSpec<'_>,
    ) -> CandidatePlan {
        use rayon::prelude::*;

        let plans: Vec<CandidatePlan> = indexes
            .par_iter()
            .map(|idx| idx.plan(query))
            .filter(|plan| !plan.is_unavailable())
            .collect();

        if plans.is_empty() {
            return CandidatePlan::Unavailable;
        }

        let coverage = IndexedCorpus::from_indexes(indexes);
        let mut narrowed = plans.into_iter().filter_map(|plan| match plan {
            CandidatePlan::Narrowed { candidates, .. } => Some(candidates),
            CandidatePlan::AllIndexed { .. } | CandidatePlan::Unavailable => None,
        });
        let Some(mut current) = narrowed.next() else {
            return CandidatePlan::AllIndexed { coverage };
        };

        for next in narrowed {
            let lookup: HashSet<&Path> = next.iter().map(crate::Candidate::rel_path).collect();
            current.retain(|c| lookup.contains(c.rel_path()));
            if current.is_empty() {
                break;
            }
        }

        CandidatePlan::Narrowed {
            candidates: current,
            coverage,
        }
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

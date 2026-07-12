use std::collections::HashSet;
use std::path::{Path, PathBuf};

use rayon::prelude::*;

use super::config::CorpusKind;
use super::error::IndexError;
use super::kinds::{Index, NarrowingResult};
use super::paths::IndexedCorpus;
use super::snapshot::{Snapshot, SnapshotId};
use super::store;
use crate::Searcher;
use crate::candidates::Candidates;
use crate::candidates::resolved::IndexFileIds;
use crate::corpus::filter::{CandidateFilter, FilterAdmission};
use crate::corpus::walk::FileWalk;
use crate::search::SearchQuery;

/// Identity of a usable index: everything needed to trust or validate it.
pub struct IndexAvailability<'a> {
    pub root: &'a Path,
    pub corpus: CorpusKind,
    pub snapshot: SnapshotId,
}

/// Registry of opened indexes read from a snapshot store.
///
/// Opens all index kinds found in the current snapshot and provides
/// query-time candidate narrowing by intersecting results from each index.
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

    /// Whether this registry has a usable snapshot for candidate discovery.
    #[must_use]
    pub fn availability(&self) -> Option<IndexAvailability<'_>> {
        if self.snapshot.is_empty() {
            return None;
        }
        let root = self.snapshot.root();
        let indexes = self.snapshot.indexes();
        let first = indexes.first()?;
        let corpus = first.corpus_kind();
        if indexes.iter().any(|idx| idx.corpus_kind() != corpus) {
            return None;
        }
        let snapshot = self.snapshot.id()?.clone();
        Some(IndexAvailability {
            root,
            corpus,
            snapshot,
        })
    }

    /// Narrow by the search query and return lazy index-backed candidates.
    ///
    /// # Errors
    ///
    /// Returns an error if the search query cannot be compiled.
    #[must_use = "resolved candidates are consumed by search"]
    pub fn candidates<'a>(
        &'a self,
        query: &SearchQuery,
        filter: &'a CandidateFilter,
        admission: FilterAdmission,
    ) -> crate::Result<Candidates<'a>> {
        let searcher = Searcher::new(query.clone())?;
        let candidate_query = crate::candidates::narrowing::CandidateQuery::new(
            query,
            searcher.prefilter_compatibility(),
        );
        let narrowing = self.narrow(&candidate_query);
        Ok(Candidates::from(
            self.index_file_ids(narrowing, filter, admission),
        ))
    }

    pub(crate) fn index_file_ids<'a>(
        &'a self,
        narrowing: NarrowingResult,
        filter: &'a CandidateFilter,
        admission: FilterAdmission,
    ) -> IndexFileIds<'a> {
        let file_ids = match narrowing {
            NarrowingResult::Unavailable | NarrowingResult::AllIndexed => {
                self.all_indexed_file_ids()
            }
            NarrowingResult::Narrowed { file_ids } => file_ids,
        };
        IndexFileIds::new(self, file_ids, filter, admission)
    }

    #[must_use]
    pub(crate) fn narrow(
        &self,
        query: &crate::candidates::narrowing::CandidateQuery<'_>,
    ) -> NarrowingResult {
        let indexes = self.snapshot.indexes();
        match indexes.len() {
            0 => NarrowingResult::Unavailable,
            1 => indexes[0].narrow(query),
            _ => Self::narrow_multi(indexes, query),
        }
    }

    #[must_use]
    pub(crate) fn materialize_row(
        &self,
        id: u32,
        filter: &CandidateFilter,
        admission: FilterAdmission,
    ) -> Option<crate::Candidate> {
        let index = self.primary_index()?;
        index.materialize_row(id, filter, admission)
    }

    #[must_use]
    pub(crate) fn materialize_rows(
        &self,
        file_ids: &[u32],
        filter: &CandidateFilter,
        admission: FilterAdmission,
    ) -> Vec<crate::Candidate> {
        let Some(index) = self.primary_index() else {
            return Vec::new();
        };
        index.materialize_rows(file_ids, filter, admission)
    }

    pub(crate) fn unindexed_walk_candidates(
        &self,
        filter: &CandidateFilter,
    ) -> crate::Result<Vec<crate::Candidate>> {
        let indexed = self.indexed_paths();
        FileWalk::from_filter(filter).candidates_matching(indexed.unindexed_files())
    }

    /// Corpus-relative paths present in the current snapshot.
    #[must_use]
    pub(crate) fn indexed_rel_paths(&self) -> HashSet<PathBuf> {
        self.indexed_paths().into_set()
    }

    /// Filter hit paths to those not yet indexed in the current snapshot.
    #[must_use]
    pub(crate) fn unindexed_hit_paths(
        &self,
        hits: impl IntoIterator<Item = PathBuf>,
    ) -> Vec<PathBuf> {
        let indexed = self.indexed_paths();
        hits.into_iter()
            .filter(|path| !indexed.contains(path))
            .collect()
    }

    fn indexed_paths(&self) -> IndexedCorpus {
        IndexedCorpus::from_indexes(self.snapshot.indexes())
    }

    fn primary_index(&self) -> Option<&Index> {
        self.snapshot.indexes().first()
    }

    fn all_indexed_file_ids(&self) -> Vec<u32> {
        let indexes = self.snapshot.indexes();
        let mut iter = indexes.iter();
        let Some(first) = iter.next() else {
            return Vec::new();
        };

        let mut file_ids = first.all_file_ids();

        for index in iter {
            let next: HashSet<PathBuf> = index
                .all_file_ids()
                .into_iter()
                .filter_map(|id| index.rel_path(id))
                .collect();
            file_ids.retain(|id| first.rel_path(*id).is_some_and(|path| next.contains(&path)));
            if file_ids.is_empty() {
                break;
            }
        }

        file_ids
    }

    fn narrow_multi(
        indexes: &[Index],
        query: &crate::candidates::narrowing::CandidateQuery<'_>,
    ) -> NarrowingResult {
        let plans: Vec<NarrowingResult> = indexes
            .par_iter()
            .map(|idx| idx.narrow(query))
            .filter(|plan| !plan.is_unavailable())
            .collect();

        if plans.is_empty() {
            return NarrowingResult::Unavailable;
        }

        let mut narrowed = plans.into_iter().filter_map(|plan| match plan {
            NarrowingResult::Narrowed { file_ids } => Some(file_ids),
            NarrowingResult::AllIndexed | NarrowingResult::Unavailable => None,
        });
        let Some(mut current) = narrowed.next() else {
            return NarrowingResult::AllIndexed;
        };

        for next in narrowed {
            let mut out = Vec::with_capacity(current.len().min(next.len()));
            let (mut i, mut j) = (0usize, 0usize);
            while i < current.len() && j < next.len() {
                match current[i].cmp(&next[j]) {
                    std::cmp::Ordering::Less => i += 1,
                    std::cmp::Ordering::Greater => j += 1,
                    std::cmp::Ordering::Equal => {
                        out.push(current[i]);
                        i += 1;
                        j += 1;
                    }
                }
            }
            current = out;
            if current.is_empty() {
                break;
            }
        }

        NarrowingResult::Narrowed { file_ids: current }
    }
}

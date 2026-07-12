use std::collections::HashSet;
use std::path::{Path, PathBuf};

use rayon::prelude::*;

use super::config::CorpusKind;
use super::error::IndexError;
use super::kinds::{Index, IndexQueryResult};
use super::paths::IndexedCorpus;
use super::snapshot::{Snapshot, SnapshotId};

use crate::candidates::Candidates;
use crate::corpus::Candidate;
use crate::corpus::filter::{CandidateFilter, FilterAdmission};

/// Read-only view of an opened snapshot usable for search.
///
/// `snapshot` is `None` when the opened snapshot has no committed id.
pub struct IndexSession<'a> {
    pub root: &'a Path,
    pub corpus: CorpusKind,
    pub snapshot: Option<SnapshotId>,
}

/// Opened snapshot indexes for query-time candidate resolution.
pub struct Indexes {
    snapshot: Snapshot,
}

impl Indexes {
    /// Open the current committed snapshot under `sift_dir`.
    ///
    /// # Errors
    ///
    /// Returns [`IndexError::InvalidManifest`] if a snapshot manifest is
    /// malformed, or [`IndexError::Trigram`] if a trigram index is malformed.
    ///
    /// Returns an empty `Indexes` when no current snapshot exists (walk fallback).
    pub fn open(sift_dir: &Path) -> Result<Self, IndexError> {
        let snapshot = Snapshot::open_current(sift_dir).map_err(|e| match e {
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
        Ok(Self::from_snapshot(snapshot))
    }

    /// Wrap an already-opened snapshot for search.
    #[must_use]
    pub const fn from_snapshot(snapshot: Snapshot) -> Self {
        Self { snapshot }
    }

    /// Opened snapshot identity when indexes are usable for candidate discovery.
    #[must_use]
    pub fn session(&self) -> Option<IndexSession<'_>> {
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
        let snapshot = self.snapshot.id().cloned();
        Some(IndexSession {
            root,
            corpus,
            snapshot,
        })
    }

    /// Corpus-relative paths covered by every opened index in this snapshot.
    #[must_use]
    pub fn indexed_corpus(&self) -> IndexedCorpus {
        IndexedCorpus::from_indexes(self.snapshot.indexes())
    }

    #[must_use]
    pub(crate) fn query(
        &self,
        query: &crate::candidates::query::CandidateQuery<'_>,
    ) -> IndexQueryResult {
        let indexes = self.snapshot.indexes();
        match indexes.len() {
            0 => IndexQueryResult::Unavailable,
            1 => indexes[0].query(query),
            _ => Self::query_multi(indexes, query),
        }
    }

    #[must_use]
    pub(crate) fn file_ids(&self, result: IndexQueryResult) -> Vec<u32> {
        match result {
            IndexQueryResult::Unavailable | IndexQueryResult::AllIndexed => {
                self.all_indexed_file_ids()
            }
            IndexQueryResult::Matched { file_ids } => file_ids,
        }
    }

    pub(crate) fn indexed_candidates<'a>(
        &'a self,
        result: IndexQueryResult,
        filter: &'a CandidateFilter,
        admission: FilterAdmission,
    ) -> Candidates<'a> {
        Candidates::index(self, self.file_ids(result), filter, admission)
    }

    pub(crate) fn hydrate_row(
        &self,
        id: u32,
        filter: &CandidateFilter,
        admission: FilterAdmission,
    ) -> Option<Candidate> {
        self.lead_index()?.hydrate_row(id, filter, admission)
    }

    pub(crate) fn hydrate_rows(
        &self,
        file_ids: &[u32],
        filter: &CandidateFilter,
        admission: FilterAdmission,
    ) -> Vec<Candidate> {
        let Some(index) = self.lead_index() else {
            return Vec::new();
        };
        index.hydrate_rows(file_ids, filter, admission)
    }

    /// Manifest-first index: owns the file table used to hydrate candidates.
    fn lead_index(&self) -> Option<&Index> {
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

    fn query_multi(
        indexes: &[Index],
        query: &crate::candidates::query::CandidateQuery<'_>,
    ) -> IndexQueryResult {
        let plans: Vec<IndexQueryResult> = indexes
            .par_iter()
            .map(|idx| idx.query(query))
            .filter(|plan| !plan.is_unavailable())
            .collect();

        if plans.is_empty() {
            return IndexQueryResult::Unavailable;
        }

        let mut matched = plans.into_iter().filter_map(|plan| match plan {
            IndexQueryResult::Matched { file_ids } => Some(file_ids),
            IndexQueryResult::AllIndexed | IndexQueryResult::Unavailable => None,
        });
        let Some(mut current) = matched.next() else {
            return IndexQueryResult::AllIndexed;
        };

        for next in matched {
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

        IndexQueryResult::Matched { file_ids: current }
    }
}

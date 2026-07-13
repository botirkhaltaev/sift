use std::path::Path;

use rayon::prelude::*;

use super::config::CorpusKind;
use super::error::IndexError;
use super::kinds::{Index, IndexQueryResult};
use super::paths::IndexedCorpus;
use super::snapshot::{Snapshot, SnapshotId};

use crate::candidates::Candidates;
use crate::corpus::Candidate;
use crate::corpus::filter::{CandidateFilter, FilterAdmission};

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
        Ok(Self::new(snapshot))
    }

    /// Wrap an already-opened snapshot for search.
    #[must_use]
    pub(crate) const fn new(snapshot: Snapshot) -> Self {
        Self { snapshot }
    }

    /// Whether opened indexes are usable for candidate discovery.
    #[must_use]
    pub fn usable(&self) -> bool {
        if self.snapshot.is_empty() {
            return false;
        }
        let indexes = self.snapshot.indexes();
        let Some(first) = indexes.first() else {
            return false;
        };
        let corpus = first.corpus_kind();
        indexes.iter().all(|idx| idx.corpus_kind() == corpus)
    }

    /// Corpus root when indexes are usable.
    #[must_use]
    pub fn corpus_root(&self) -> Option<&Path> {
        self.usable().then_some(self.snapshot.root())
    }

    /// Corpus kind when indexes are usable.
    #[must_use]
    pub fn corpus_kind(&self) -> Option<CorpusKind> {
        let first = self.snapshot.indexes().first()?;
        self.usable().then_some(first.corpus_kind())
    }

    /// Committed snapshot id when present.
    #[must_use]
    pub fn snapshot_id(&self) -> Option<&SnapshotId> {
        self.usable().then(|| self.snapshot.id()).flatten()
    }

    /// Corpus-relative paths covered by every opened index in this snapshot.
    #[must_use]
    pub fn indexed_corpus(&self) -> IndexedCorpus {
        IndexedCorpus::intersection(self.snapshot.indexes().iter().map(Index::coverage))
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
    pub(crate) fn file_ids(&self, result: IndexQueryResult, corpus: &IndexedCorpus) -> Vec<u32> {
        match result {
            IndexQueryResult::Unavailable | IndexQueryResult::AllIndexed => {
                self.all_indexed_file_ids(corpus)
            }
            IndexQueryResult::Matched { file_ids } => file_ids,
        }
    }

    pub(crate) fn indexed_candidates<'a>(
        &'a self,
        result: IndexQueryResult,
        corpus: &IndexedCorpus,
        filter: &'a CandidateFilter,
        admission: FilterAdmission,
    ) -> Candidates<'a> {
        Candidates::index(self, self.file_ids(result, corpus), filter, admission)
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

    fn all_indexed_file_ids(&self, corpus: &IndexedCorpus) -> Vec<u32> {
        let Some(lead) = self.lead_index() else {
            return Vec::new();
        };
        // Single opened index: coverage is that index's file set, so every lead
        // file id is in-corpus. Skip per-id PathBuf + HashSet lookup (heaptrack
        // hot path on AllIndexed / case-insensitive full-cover narrowings).
        if self.snapshot.indexes().len() == 1 {
            return lead.all_file_ids();
        }
        lead.all_file_ids()
            .into_iter()
            .filter(|id| {
                lead.rel_path(*id)
                    .is_some_and(|path| corpus.contains(path.as_path()))
            })
            .collect()
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

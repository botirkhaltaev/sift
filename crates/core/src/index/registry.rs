use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::config::CorpusKind;
use super::error::IndexError;
use super::kinds::Index;
use super::store;

/// Registry of opened indexes read from a snapshot store.
pub struct Indexes {
    inner: Vec<Index>,
    root: PathBuf,
}

impl Indexes {
    /// Create an Indexes registry from a single index and its root.
    ///
    /// Useful for testing and benchmarking.
    #[must_use]
    pub fn from_single(index: Index, root: PathBuf) -> Self {
        Self {
            inner: vec![index],
            root,
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

        let inner = store.open_current().map_err(|e| match e {
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

        let root = super::meta::StoreMeta::read(sift_dir)
            .ok()
            .map(|m| m.root)
            .unwrap_or_default();

        Ok(Self { inner, root })
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Produce narrowed candidates from all indexes that can narrow the query.
    ///
    /// Returns `None` if no index could narrow. When at least one index
    /// narrows, all narrowed candidate sets are intersected.
    #[must_use]
    pub fn candidates(&self, query: &crate::query::QuerySpec<'_>) -> Option<Vec<crate::Candidate>> {
        match self.inner.len() {
            0 => None,
            1 => self.inner[0].candidates(query),
            _ => self.candidates_multi(query),
        }
    }

    /// Return all indexed candidates across all registered indexes.
    #[must_use]
    pub(crate) fn complete_candidates(&self) -> Vec<crate::Candidate> {
        let mut iter = self.inner.iter();
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
        &self,
        query: &crate::query::QuerySpec<'_>,
    ) -> Option<Vec<crate::Candidate>> {
        use rayon::prelude::*;

        let sets: Vec<Vec<crate::Candidate>> = self
            .inner
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
        self.inner.first()
    }

    #[must_use]
    pub fn corpus_kind(&self) -> Option<CorpusKind> {
        let kind = self.inner.first()?.corpus_kind();
        if self.inner.iter().any(|idx| idx.corpus_kind() != kind) {
            return None;
        }
        Some(kind)
    }
}

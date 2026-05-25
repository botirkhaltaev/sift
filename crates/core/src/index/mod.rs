pub mod store;
pub mod trigram;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::search::output::mode::CandidateCoverage;

pub use trigram::TrigramIndexError;

/// How an index query plan resolves candidates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PlanMode {
    /// The query was narrowed using trigram candidates from the index.
    #[default]
    IndexedCandidates,
    /// No trigrams were usable — all indexed files must be scanned.
    FullScan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryPlanOutput {
    pub pattern: String,
    pub mode: PlanMode,
}

/// Whether the index was built from a directory or a single file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CorpusKind {
    /// Built from a directory path — all discovered files were indexed.
    #[default]
    Directory,
    /// Built from a single file path — only that file was indexed.
    SingleFile,
}

/// Configuration for building an index over a corpus.
pub struct IndexBuildConfig<'a> {
    pub root: &'a Path,
    pub follow_links: bool,
    pub exclude_paths: &'a [PathBuf],
    pub include_paths: &'a [PathBuf],
    pub corpus_kind: CorpusKind,
}

/// Errors specific to the index registry layer.
#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("invalid index layout: {path}")]
    InvalidLayout { path: PathBuf },

    #[error(transparent)]
    Trigram(#[from] TrigramIndexError),

    #[error("IO error inspecting index path {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("unknown index kind: {0}")]
    UnknownIndexKind(String),

    #[error("invalid snapshot manifest at {path}: {source}")]
    InvalidManifest {
        path: PathBuf,
        source: serde_json::Error,
    },
}

/// A searchable index that can also be built and opened from disk.
///
/// Object-safe surface: [`root`](Index::root), [`corpus_kind`](Index::corpus_kind),
/// [`candidates`](Index::candidates), [`all_files`](Index::all_files).
///
/// Lifecycle methods ([`kind_name`](Index::kind_name), [`build`](Index::build),
/// [`open`](Index::open)) require `Self: Sized` and are only callable on
/// concrete types or in generic contexts.
pub trait Index: Sync + Send {
    fn root(&self) -> &Path;
    fn corpus_kind(&self) -> CorpusKind;
    fn candidates(&self, query: &crate::query::QuerySpec<'_>) -> Vec<crate::Candidate>;
    fn all_files(&self) -> Vec<crate::Candidate>;

    /// Short identifier for the index kind (e.g. `"trigram"`).
    fn kind_name() -> &'static str
    where
        Self: Sized;

    /// Build a new index over the corpus described in `config`, writing
    /// persistence files into `output_dir`.
    ///
    /// # Errors
    ///
    /// Returns an error if corpus walking, extraction, or file I/O fails.
    fn build(config: &IndexBuildConfig<'_>, output_dir: &Path) -> crate::Result<Self>
    where
        Self: Sized;

    /// Open an index that was previously persisted to `index_dir`.
    ///
    /// # Errors
    ///
    /// Returns an error if persistence files are missing or malformed.
    fn open(index_dir: &Path, root: &Path, corpus_kind: CorpusKind) -> crate::Result<Self>
    where
        Self: Sized;
}

/// Registry of opened indexes read from a snapshot store.
pub struct Indexes {
    inner: Vec<Box<dyn Index>>,
    root: PathBuf,
}

impl Indexes {
    /// Create an Indexes registry from a single index and its root.
    ///
    /// Useful for testing and benchmarking.
    #[must_use]
    pub fn from_single(index: impl Index + 'static, root: PathBuf) -> Self {
        Self {
            inner: vec![Box::new(index)],
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

        let root = inner
            .first()
            .map(|idx| idx.root().to_path_buf())
            .unwrap_or_default();

        Ok(Self { inner, root })
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolve candidates for a query across all registered indexes.
    #[must_use]
    pub fn resolve_candidates(&self, query: &crate::query::QuerySpec<'_>) -> Vec<crate::Candidate> {
        let mut iter = self.inner.iter();
        let Some(first) = iter.next() else {
            return Vec::new();
        };

        let mut candidates = first.candidates(query);

        for index in iter {
            let next: std::collections::HashSet<PathBuf> = index
                .candidates(query)
                .into_iter()
                .map(|c| c.abs_path().to_path_buf())
                .collect();
            candidates.retain(|c| next.contains(c.abs_path()));
            if candidates.is_empty() {
                break;
            }
        }

        candidates
    }

    /// Resolve candidates for a query, selecting narrowed or complete coverage.
    #[must_use]
    pub fn candidates(
        &self,
        query: &crate::query::QuerySpec<'_>,
        coverage: CandidateCoverage,
    ) -> Vec<crate::Candidate> {
        match coverage {
            CandidateCoverage::Narrowed => self.resolve_candidates(query),
            CandidateCoverage::Complete => self.resolve_all_files(),
        }
    }

    /// Return all indexed files across all registered indexes.
    #[must_use]
    pub fn resolve_all_files(&self) -> Vec<crate::Candidate> {
        let mut iter = self.inner.iter();
        let Some(first) = iter.next() else {
            return Vec::new();
        };

        let mut files = first.all_files();

        for index in iter {
            let next: std::collections::HashSet<PathBuf> = index
                .all_files()
                .into_iter()
                .map(|c| c.abs_path().to_path_buf())
                .collect();
            files.retain(|c| next.contains(c.abs_path()));
            if files.is_empty() {
                break;
            }
        }

        files
    }

    #[must_use]
    pub fn first(&self) -> Option<&dyn Index> {
        self.inner.first().map(AsRef::as_ref)
    }

    /// Returns the corpus kind if all indexes agree, or `None` for mixed/empty.
    #[must_use]
    pub fn corpus_kind(&self) -> Option<CorpusKind> {
        let kind = self.inner.first()?.corpus_kind();
        if self.inner.iter().any(|idx| idx.corpus_kind() != kind) {
            return None;
        }
        Some(kind)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileId(usize);

impl FileId {
    #[must_use]
    pub const fn new(value: usize) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IndexId(usize);

impl IndexId {
    #[must_use]
    pub const fn new(value: usize) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexMeta {
    pub root: PathBuf,
    #[serde(default)]
    pub corpus_kind: CorpusKind,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn file_id_new_and_get() {
        let id = FileId::new(42);
        assert_eq!(id.get(), 42);
    }

    #[test]
    fn index_id_new_and_get() {
        let id = IndexId::new(7);
        assert_eq!(id.get(), 7);
    }

    #[test]
    fn indexes_open_empty_when_no_current_file() {
        let tmp = TempDir::new().expect("create temp dir");
        let sift_dir = tmp.path().join(".sift");
        fs::create_dir_all(&sift_dir).expect("create sift dir");
        let indexes = Indexes::open(&sift_dir).expect("open indexes");
        assert!(indexes.is_empty());
        assert!(indexes.root().as_os_str().is_empty());
    }

    #[test]
    fn indexes_first_returns_none_when_empty() {
        let tmp = TempDir::new().expect("create temp dir");
        let sift_dir = tmp.path().join(".sift");
        fs::create_dir_all(&sift_dir).expect("create sift dir");
        let indexes = Indexes::open(&sift_dir).expect("open indexes");
        assert!(indexes.first().is_none());
    }
}

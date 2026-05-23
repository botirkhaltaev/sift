pub mod trigram;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use trigram::TrigramIndex;
pub use trigram::TrigramIndexError;

const INDEX_KINDS: &[&str] = &["trigram"];

/// Errors specific to the index registry layer.
///
/// These cover layout discovery, root validation, and wrapping of concrete
/// index implementation errors.
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
}

/// Registry of opened indexes for a single `.sift` directory.
///
/// Owns index initialization, validates that all indexes share one root,
/// and exposes only what callers need through `is_empty()`, `root()`, and `refs()`.
pub struct Indexes {
    inner: Vec<Box<dyn SearchIndex>>,
    root: PathBuf,
}

impl Indexes {
    /// Open all indexes found under `sift_dir` (e.g. `.sift/trigram/`).
    ///
    /// # Errors
    ///
    /// Returns [`IndexError::InvalidLayout`] if a known index-kind path exists
    /// but is not a directory, [`IndexError::Trigram`] if a concrete trigram
    /// index is malformed, or [`IndexError::Io`] for filesystem inspection
    /// failures.
    ///
    /// Returns an empty registry if no index kind directory is found.
    pub fn open(sift_dir: &Path) -> Result<Self, IndexError> {
        let mut indexes: Vec<Box<dyn SearchIndex>> = Vec::new();
        let mut root: Option<PathBuf> = None;

        for kind in INDEX_KINDS {
            let kind_dir = sift_dir.join(kind);
            match std::fs::metadata(&kind_dir) {
                Ok(meta) if meta.is_dir() => {}
                Ok(_) => {
                    return Err(IndexError::InvalidLayout { path: kind_dir });
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    continue;
                }
                Err(e) => {
                    return Err(IndexError::Io {
                        path: kind_dir,
                        source: e,
                    });
                }
            }

            let index = TrigramIndex::open(&kind_dir).map_err(IndexError::Trigram)?;
            let this_root = index.root().to_path_buf();
            if let Some(existing_root) = &root {
                if *existing_root != this_root {
                    return Err(IndexError::InvalidLayout { path: kind_dir });
                }
            } else {
                root = Some(this_root);
            }
            indexes.push(Box::new(index));
        }

        Ok(Self {
            inner: indexes,
            root: root.unwrap_or_default(),
        })
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns a vector of trait object references for search execution.
    ///
    /// Allocates a new `Vec` on each call. The returned value borrows from
    /// `self` and becomes invalid when `self` is dropped.
    #[must_use]
    pub fn refs(&self) -> Vec<&dyn SearchIndex> {
        self.inner.iter().map(AsRef::as_ref).collect()
    }

    /// Returns a reference to the first index, if any.
    #[must_use]
    pub fn first(&self) -> Option<&dyn SearchIndex> {
        self.inner.first().map(AsRef::as_ref)
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

pub trait SearchIndex: Sync + Send {
    fn root(&self) -> &Path;
    fn file_count(&self) -> usize;
    fn file_path(&self, id: FileId) -> Option<&Path>;
    fn file_abs_path(&self, id: FileId) -> Option<PathBuf>;
    fn candidates(&self, query: &crate::query::QuerySpec<'_>) -> Vec<FileId>;
    fn is_single_file(&self) -> bool;
    fn explain(&self, query: &crate::query::QuerySpec<'_>) -> QueryPlanOutput {
        use crate::query::{CandidatePlan, QueryPlanner};
        let mode = match QueryPlanner::plan(query) {
            CandidatePlan::FullScan => "full_scan",
            CandidatePlan::Trigram(_) => "indexed_candidates",
        };
        QueryPlanOutput {
            pattern: query.patterns.to_vec().join("|"),
            mode,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexMeta {
    pub root: PathBuf,
    #[serde(default)]
    pub is_single_file_corpus: bool,
}

impl IndexMeta {
    /// # Errors
    ///
    /// Returns `InvalidMeta` if `root` is not an absolute path.
    pub fn validate(self, meta_path: &Path) -> Result<Self, TrigramIndexError> {
        if !self.root.is_absolute() {
            return Err(TrigramIndexError::InvalidMeta(meta_path.to_path_buf()));
        }
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
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
    fn index_meta_validate_accepts_absolute_root() {
        let abs = std::env::current_dir().unwrap();
        let meta = IndexMeta {
            root: abs,
            is_single_file_corpus: false,
        };
        let result = meta.validate(Path::new("/fake/meta.json"));
        assert!(result.is_ok());
    }

    #[test]
    fn index_meta_validate_rejects_relative_root() {
        let meta = IndexMeta {
            root: PathBuf::from("relative/path"),
            is_single_file_corpus: true,
        };
        let result = meta.validate(Path::new("/fake/meta.json"));
        assert!(matches!(result, Err(TrigramIndexError::InvalidMeta(_))));
    }

    #[test]
    fn indexes_open_empty_when_no_index_kind_exists() {
        let tmp = TempDir::new().expect("create temp dir");
        let sift_dir = tmp.path().join(".sift");
        fs::create_dir_all(&sift_dir).expect("create sift dir");
        let indexes = Indexes::open(&sift_dir).expect("open indexes");
        assert!(indexes.is_empty());
        assert!(indexes.root().as_os_str().is_empty());
    }

    #[test]
    fn indexes_open_invalid_layout_when_trigram_is_file() {
        let tmp = TempDir::new().expect("create temp dir");
        let sift_dir = tmp.path().join(".sift");
        fs::create_dir_all(&sift_dir).expect("create sift dir");
        let trigram_path = sift_dir.join("trigram");
        fs::write(&trigram_path, "not a directory").expect("write file");

        let result = Indexes::open(&sift_dir);
        assert!(matches!(result, Err(IndexError::InvalidLayout { path }) if path == trigram_path));
    }

    #[test]
    fn indexes_first_returns_none_when_empty() {
        let tmp = TempDir::new().expect("create temp dir");
        let sift_dir = tmp.path().join(".sift");
        fs::create_dir_all(&sift_dir).expect("create sift dir");
        let indexes = Indexes::open(&sift_dir).expect("open indexes");
        assert!(indexes.first().is_none());
    }

    #[test]
    fn indexes_refs_returns_empty_vec_when_empty() {
        let tmp = TempDir::new().expect("create temp dir");
        let sift_dir = tmp.path().join(".sift");
        fs::create_dir_all(&sift_dir).expect("create sift dir");
        let indexes = Indexes::open(&sift_dir).expect("open indexes");
        assert!(indexes.refs().is_empty());
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryPlanOutput {
    pub pattern: String,
    pub mode: &'static str,
}

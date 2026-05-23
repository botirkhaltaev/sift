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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexMeta {
    pub root: PathBuf,
    #[serde(default)]
    pub is_single_file_corpus: bool,
}

impl IndexMeta {
    pub fn validate(self, meta_path: &Path) -> Result<Self, TrigramIndexError> {
        if !self.root.is_absolute() {
            return Err(TrigramIndexError::InvalidMeta(meta_path.to_path_buf()));
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryPlanOutput {
    pub pattern: String,
    pub mode: &'static str,
}

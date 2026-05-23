pub mod trigram;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use trigram::TrigramIndex;

const INDEX_KINDS: &[&str] = &["trigram"];

/// Registry of opened indexes for a single `.sift` directory.
///
/// Owns index initialization, validates that all indexes share one root,
/// and exposes only what callers need through `is_empty()`, `root()`, and `as_refs()`.
pub struct Indexes {
    inner: Vec<Box<dyn SearchIndex>>,
    root: PathBuf,
}

impl Indexes {
    /// Open all indexes found under `sift_dir` (e.g. `.sift/trigram/`).
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::MissingComponent`] if an index kind directory exists
    /// but its trigram tables are incomplete, or [`crate::Error::InvalidMeta`]
    /// if metadata is missing, malformed, or indexes have conflicting roots.
    ///
    /// Returns an empty registry if no index kind directory is found.
    pub fn open(sift_dir: &Path) -> crate::Result<Self> {
        let mut indexes: Vec<Box<dyn SearchIndex>> = Vec::new();
        let mut root: Option<PathBuf> = None;

        for kind in INDEX_KINDS {
            let kind_dir = sift_dir.join(kind);
            if !kind_dir.is_dir() {
                continue;
            }
            let index = TrigramIndex::open(&kind_dir)?;
            let this_root = index.root().to_path_buf();
            if let Some(existing_root) = &root {
                if *existing_root != this_root {
                    return Err(crate::Error::InvalidMeta(
                        kind_dir.join(crate::META_FILENAME),
                    ));
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
    /// The returned value borrows from `self` and becomes invalid when `self` is dropped.
    #[must_use]
    pub fn as_slice(&self) -> Vec<&dyn SearchIndex> {
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
    pub fn validate(self, meta_path: &Path) -> crate::Result<Self> {
        if !self.root.is_absolute() {
            return Err(crate::Error::InvalidMeta(meta_path.to_path_buf()));
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryPlanOutput {
    pub pattern: String,
    pub mode: &'static str,
}

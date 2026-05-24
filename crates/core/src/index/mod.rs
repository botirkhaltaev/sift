pub mod trigram;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use trigram::TrigramIndex;
pub use trigram::TrigramIndexError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryPlanOutput {
    pub pattern: String,
    pub mode: &'static str,
}

/// Whether the index was built from a directory or a single file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum IndexKind {
    /// Built from a directory path — all discovered files were indexed.
    #[default]
    Directory,
    /// Built from a single file path — only that file was indexed.
    SingleFile,
}

const INDEX_KINDS: &[&str] = &["trigram"];

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
}

/// A candidate file returned by an index for searching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchCandidate {
    pub rel_path: PathBuf,
    pub abs_path: PathBuf,
}

/// Registry of opened indexes for a single `.sift` directory.
pub struct Indexes {
    inner: Vec<Box<dyn SearchIndex>>,
    root: PathBuf,
}

impl Indexes {
    /// Create an Indexes registry from a single index and its root.
    ///
    /// Useful for testing and benchmarking.
    #[must_use]
    pub fn from_single(index: impl SearchIndex + 'static, root: PathBuf) -> Self {
        Self {
            inner: vec![Box::new(index)],
            root,
        }
    }

    /// Open all indexes found under `sift_dir`.
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

    /// Resolve candidates for a query across all registered indexes.
    ///
    /// Each index returns its candidate set; results are deduplicated by
    /// absolute path and returned as a flat list ready for filtering and scanning.
    #[must_use]
    pub fn resolve_candidates(&self, query: &crate::query::QuerySpec<'_>) -> Vec<SearchCandidate> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();

        for index in &self.inner {
            for candidate in index.candidates(query) {
                if seen.insert(candidate.abs_path.clone()) {
                    out.push(candidate);
                }
            }
        }

        out
    }

    /// Return all indexed files across all registered indexes.
    ///
    /// Used for output modes that require scanning every file (e.g. `--count`,
    /// `--files-without-match`). Deduplicated by absolute path.
    #[must_use]
    pub fn resolve_all_files(&self) -> Vec<SearchCandidate> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();

        for index in &self.inner {
            for candidate in index.all_files() {
                if seen.insert(candidate.abs_path.clone()) {
                    out.push(candidate);
                }
            }
        }

        out
    }

    #[must_use]
    pub fn first(&self) -> Option<&dyn SearchIndex> {
        self.inner.first().map(AsRef::as_ref)
    }

    /// Returns true when the index was built from a single file input.
    #[must_use]
    pub fn is_single_file(&self) -> bool {
        self.inner.len() == 1
            && self
                .inner
                .first()
                .is_some_and(|idx| idx.kind() == IndexKind::SingleFile)
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

/// An indexed search corpus that can return candidate files for a query.
pub trait SearchIndex: Sync + Send {
    fn root(&self) -> &Path;
    fn kind(&self) -> IndexKind;
    fn candidates(&self, query: &crate::query::QuerySpec<'_>) -> Vec<SearchCandidate>;
    fn all_files(&self) -> Vec<SearchCandidate>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexMeta {
    pub root: PathBuf,
    #[serde(default)]
    pub kind: IndexKind,
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
            kind: IndexKind::Directory,
        };
        let result = meta.validate(Path::new("/fake/meta.json"));
        assert!(result.is_ok());
    }

    #[test]
    fn index_meta_validate_rejects_relative_root() {
        let meta = IndexMeta {
            root: PathBuf::from("relative/path"),
            kind: IndexKind::Directory,
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
}

pub mod trigram;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use trigram::TrigramIndex;

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

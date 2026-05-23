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

pub trait Index {
    fn root(&self) -> &Path;
    fn file_count(&self) -> usize;
    fn file_path(&self, id: FileId) -> Option<&Path>;
    fn file_abs_path(&self, id: FileId) -> Option<PathBuf>;
}

pub trait CandidateSource<P>: Index {
    fn candidate_ids(&self, plan: &P) -> Vec<FileId>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CorpusKind {
    Directory,
    File { entries: Vec<PathBuf> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexMeta {
    pub root: PathBuf,
    #[serde(flatten)]
    pub kind: CorpusKind,
}

impl IndexMeta {
    pub const fn new(root: PathBuf, kind: CorpusKind) -> Self {
        Self { root, kind }
    }

    pub fn validate(self, meta_path: &Path) -> crate::Result<Self> {
        if !self.root.is_absolute() {
            return Err(crate::Error::InvalidMeta(meta_path.to_path_buf()));
        }
        match &self.kind {
            CorpusKind::Directory => {}
            CorpusKind::File { entries } => {
                if entries.len() != 1
                    || entries.iter().any(|entry| {
                        entry.as_os_str().is_empty()
                            || entry.is_absolute()
                            || entry.components().count() != 1
                    })
                {
                    return Err(crate::Error::InvalidMeta(meta_path.to_path_buf()));
                }
            }
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryPlanOutput {
    pub pattern: String,
    pub mode: &'static str,
}

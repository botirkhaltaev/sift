use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::corpus::walk::WalkSelector;

use super::kinds::Index;

/// Cheap-to-clone set of corpus-relative paths covered by an index.
#[derive(Debug, Clone)]
pub struct IndexedCorpus {
    paths: Arc<HashSet<PathBuf>>,
}

impl IndexedCorpus {
    #[must_use]
    pub fn from_paths(paths: impl IntoIterator<Item = PathBuf>) -> Self {
        Self {
            paths: Arc::new(paths.into_iter().collect()),
        }
    }

    pub(super) fn from_indexes(indexes: &[Index]) -> Self {
        let mut iter = indexes.iter();
        let Some(first) = iter.next() else {
            return Self {
                paths: Arc::new(HashSet::new()),
            };
        };

        let mut paths = first.coverage().into_set();
        for index in iter {
            let next = index.coverage();
            paths.retain(|path| next.contains(path));
            if paths.is_empty() {
                break;
            }
        }

        Self {
            paths: Arc::new(paths),
        }
    }

    #[must_use]
    pub fn contains(&self, path: &Path) -> bool {
        self.paths.contains(path)
    }

    pub fn paths(&self) -> impl Iterator<Item = &Path> {
        self.paths.iter().map(PathBuf::as_path)
    }

    #[must_use]
    pub fn into_set(self) -> HashSet<PathBuf> {
        Arc::try_unwrap(self.paths).unwrap_or_else(|paths| (*paths).clone())
    }

    /// Drop paths already present in this indexed corpus.
    #[must_use]
    pub fn retain_unindexed(self, paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
        paths
            .into_iter()
            .filter(|path| !self.contains(path))
            .collect()
    }

    pub(crate) const fn unindexed_files(&self) -> UnindexedFiles<'_> {
        UnindexedFiles { indexed: self }
    }
}

#[derive(Clone, Copy)]
pub struct UnindexedFiles<'a> {
    indexed: &'a IndexedCorpus,
}

impl WalkSelector for UnindexedFiles<'_> {
    fn includes(&self, rel_path: &Path) -> bool {
        !self.indexed.contains(rel_path)
    }
}

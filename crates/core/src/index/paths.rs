use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::corpus::walk::WalkSelector;

/// Cheap-to-clone set of corpus-relative paths covered by an index.
#[derive(Debug, Clone)]
pub struct IndexedCorpus {
    paths: Arc<HashSet<PathBuf>>,
}

impl IndexedCorpus {
    #[must_use]
    pub(crate) fn new(paths: impl IntoIterator<Item = PathBuf>) -> Self {
        Self {
            paths: Arc::new(paths.into_iter().collect()),
        }
    }

    /// Corpus-relative paths covered by every provided coverage set.
    #[must_use]
    pub(crate) fn intersection(coverages: impl IntoIterator<Item = Self>) -> Self {
        let mut iter = coverages.into_iter();
        let Some(first) = iter.next() else {
            return Self::new([]);
        };

        let mut paths = first.into_set();
        for next in iter {
            let next_paths = next.into_set();
            paths.retain(|path| next_paths.contains(path));
            if paths.is_empty() {
                break;
            }
        }

        Self::new(paths)
    }

    #[must_use]
    pub fn contains(&self, path: &Path) -> bool {
        self.paths.contains(path)
    }

    #[must_use]
    pub(crate) fn into_set(self) -> HashSet<PathBuf> {
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

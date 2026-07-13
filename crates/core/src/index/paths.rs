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
    ///
    /// A single coverage is returned as-is (cheap `Arc` clone). Multi-index
    /// intersection clones one set and retains against the others by reference,
    /// avoiding `into_set`/`Arc::try_unwrap` which copies the full path table
    /// whenever the index still holds a coverage cache.
    #[must_use]
    pub(crate) fn intersection(coverages: impl IntoIterator<Item = Self>) -> Self {
        let mut coverages = coverages.into_iter();
        let Some(first) = coverages.next() else {
            return Self::new([]);
        };
        let Some(second) = coverages.next() else {
            return first;
        };

        let mut paths = (*first.paths).clone();
        paths.retain(|path| second.contains(path));
        for next in coverages {
            if paths.is_empty() {
                break;
            }
            paths.retain(|path| next.contains(path));
        }
        Self::new(paths)
    }

    #[must_use]
    pub fn contains(&self, path: &Path) -> bool {
        self.paths.contains(path)
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

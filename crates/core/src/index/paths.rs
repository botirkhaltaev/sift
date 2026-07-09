use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::corpus::walk::WalkSelector;

use super::kinds::Index;

/// Corpus-relative paths present in one or more opened indexes.
pub(super) struct IndexedPaths {
    paths: HashSet<PathBuf>,
}

impl IndexedPaths {
    pub(super) fn from_indexes(indexes: &[Index]) -> Self {
        let mut iter = indexes.iter();
        let Some(first) = iter.next() else {
            return Self {
                paths: HashSet::new(),
            };
        };

        let mut paths = first.indexed_rel_paths();
        for index in iter {
            let next = index.indexed_rel_paths();
            paths.retain(|path| next.contains(path));
            if paths.is_empty() {
                break;
            }
        }

        Self { paths }
    }

    pub(super) fn contains(&self, path: &Path) -> bool {
        self.paths.contains(path)
    }

    pub(super) fn into_set(self) -> HashSet<PathBuf> {
        self.paths
    }

    pub(super) const fn unindexed_files(&self) -> UnindexedFiles<'_> {
        UnindexedFiles { indexed: self }
    }
}

#[derive(Clone, Copy)]
pub(super) struct UnindexedFiles<'a> {
    indexed: &'a IndexedPaths,
}

impl WalkSelector for UnindexedFiles<'_> {
    fn includes(&self, rel_path: &Path) -> bool {
        !self.indexed.contains(rel_path)
    }
}

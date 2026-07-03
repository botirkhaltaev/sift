use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::kinds::Index;

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
}

use std::path::PathBuf;

use sift_core::{IndexStore, StoreMeta};

/// Merges rapid partial index requests. Empty `paths` means full corpus.
#[derive(Debug, Default)]
pub struct IndexCoalesce {
    full: bool,
    paths: Vec<PathBuf>,
}

impl IndexCoalesce {
    pub fn push(&mut self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            self.full = true;
            self.paths.clear();
            return;
        }
        if self.full {
            return;
        }
        for path in paths {
            if !self.paths.contains(&path) {
                self.paths.push(path);
            }
        }
    }

    pub const fn is_pending(&self) -> bool {
        self.full || !self.paths.is_empty()
    }

    /// Take pending paths. Empty return value means full corpus when pending.
    pub fn take(&mut self) -> Option<Vec<PathBuf>> {
        if self.full {
            self.full = false;
            self.paths.clear();
            Some(Vec::new())
        } else if self.paths.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut self.paths))
        }
    }

    /// Take pending paths and reconcile them against the store.
    pub fn reconcile(&mut self, sift_dir: &std::path::Path, meta: &StoreMeta) {
        if let Some(paths) = self.take() {
            let result = IndexStore::open_or_create(sift_dir, meta)
                .and_then(|mut store| store.reconcile(meta, &paths));
            if let Err(e) = result {
                eprintln!("sift-daemon: refresh failed: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_partial_paths() {
        let mut c = IndexCoalesce::default();
        c.push(vec![PathBuf::from("a.rs")]);
        c.push(vec![PathBuf::from("a.rs"), PathBuf::from("b.rs")]);
        assert_eq!(
            c.take(),
            Some(vec![PathBuf::from("a.rs"), PathBuf::from("b.rs")])
        );
    }

    #[test]
    fn full_promotes_and_clears_partials() {
        let mut c = IndexCoalesce::default();
        c.push(vec![PathBuf::from("a.rs")]);
        c.push(Vec::new());
        assert_eq!(c.take(), Some(Vec::new()));
        assert!(!c.is_pending());
    }
}

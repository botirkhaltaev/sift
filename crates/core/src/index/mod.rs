pub mod artifacts;
pub mod config;
pub mod error;
pub mod kinds;
pub mod meta;
pub mod mmap;
pub mod ngram;
pub mod registry;
pub mod snapshot;
pub mod store;

pub use artifacts::{IndexDestination, IndexSource};
pub use config::{CorpusKind, CorpusSpec, IndexBuildConfig};
pub use error::IndexError;
pub use kinds::{FileId, Index, IndexConfig, IndexId, PlanMode, QueryPlanOutput};
pub use meta::{CorpusMeta, FilterMeta, IndexCoverage, WalkMeta};
pub use registry::Indexes;
pub use snapshot::SnapshotId;
pub use store::ReconcileOutcome;

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
    fn indexes_open_empty_when_no_current_file() {
        let tmp = TempDir::new().expect("create temp dir");
        let sift_dir = tmp.path().join(".sift");
        fs::create_dir_all(&sift_dir).expect("create sift dir");
        let indexes = Indexes::open(&sift_dir).expect("open indexes");
        assert!(indexes.is_empty());
        assert!(indexes.root().as_os_str().is_empty());
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

pub mod config;
pub mod error;
pub mod kinds;
pub mod meta;
pub mod registry;
pub mod snapshot;
pub mod store;
pub mod trigram;

use snapshot::{SnapshotRead, SnapshotWrite};

pub use config::{CorpusKind, CorpusSpec, IndexConfig};
pub use error::IndexError;
pub use kinds::{FileId, Index, IndexId, IndexKind, PlanMode, QueryPlanOutput};
pub use registry::Indexes;

/// Where index artifacts are read from.
#[derive(Clone, Copy)]
pub enum IndexSource<'a> {
    /// Read from a directory on disk.
    Directory(&'a std::path::Path),
    /// Read from a snapshot transaction.
    Snapshot {
        reader: &'a dyn SnapshotRead,
        namespace: &'a str,
    },
}

/// Where index artifacts are written to.
pub enum IndexDestination<'a> {
    /// Write to a directory on disk.
    Directory(&'a std::path::Path),
    /// Write into a snapshot transaction.
    Snapshot {
        writer: &'a mut dyn SnapshotWrite,
        namespace: &'a str,
    },
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

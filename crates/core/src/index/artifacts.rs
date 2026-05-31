use crate::index::snapshot::{SnapshotRead, SnapshotWrite};

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

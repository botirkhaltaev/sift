pub mod disk;

#[cfg(test)]
pub mod memory;

use super::artifact::ArtifactData;
use super::identity::SnapshotId;
use super::manifest::SnapshotManifest;

/// A readable snapshot with access to its manifest and named artifacts.
pub trait SnapshotRead {
    fn manifest(&self) -> &SnapshotManifest;
    fn artifact(&self, namespace: &str, name: &str) -> crate::Result<ArtifactData>;
}

/// A writable snapshot transaction that accepts named byte artifacts.
pub trait SnapshotWrite {
    fn id(&self) -> &SnapshotId;
    fn put_artifact(&mut self, namespace: &str, name: &str, bytes: Vec<u8>) -> crate::Result<()>;
}

/// A scoped writer session that serialises access to the snapshot store.
///
/// Dropping the session releases any exclusive access (e.g. a file lock).
pub trait SnapshotWriterSession {
    type Read: SnapshotRead;
    type Write: SnapshotWrite;

    fn current(&self) -> crate::Result<Option<Self::Read>>;
    fn begin(&mut self) -> crate::Result<Self::Write>;
    fn publish(
        &mut self,
        write: Self::Write,
        manifest: SnapshotManifest,
    ) -> crate::Result<SnapshotId>;
}

/// Generic atomic snapshot store.
///
/// Provides read access to the current snapshot and scoped write access via
/// [`writer()`](SnapshotStore::writer).
pub trait SnapshotStore {
    type Read: SnapshotRead;
    type Write: SnapshotWrite;
    type Writer<'a>: SnapshotWriterSession<Read = Self::Read, Write = Self::Write>
    where
        Self: 'a;

    fn current_id(&self) -> Option<&SnapshotId>;
    fn current(&self) -> crate::Result<Option<Self::Read>>;
    fn writer(&mut self) -> crate::Result<Self::Writer<'_>>;
}

pub mod disk;

#[cfg(test)]
pub mod memory;

use super::artifact::ArtifactData;
use super::identity::SnapshotId;
use super::manifest::SnapshotManifest;

/// A readable snapshot with access to its manifest and named artifacts.
pub trait SnapshotRead {
    fn manifest(&self) -> &SnapshotManifest;

    /// Load a named artifact from the snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error if the namespace or artifact is missing, or I/O/mmap fails.
    fn artifact(&self, namespace: &str, name: &str) -> crate::Result<ArtifactData>;

    /// List every artifact name stored under `namespace`.
    ///
    /// Used by lifecycle code to copy artifacts for an unchanged index kind
    /// without hard-coding its layout.
    ///
    /// # Errors
    ///
    /// Returns an error if the namespace cannot be inspected.
    fn artifacts(&self, namespace: &str) -> crate::Result<Vec<String>>;
}

/// A writable snapshot transaction that accepts named byte artifacts.
pub trait SnapshotWrite {
    fn id(&self) -> &SnapshotId;

    /// Store a named artifact in the in-progress snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error if the artifact cannot be written.
    fn put_artifact(&mut self, namespace: &str, name: &str, bytes: Vec<u8>) -> crate::Result<()>;
}

/// A scoped writer session that serialises access to the snapshot store.
///
/// Dropping the session releases any exclusive access (e.g. a file lock).
pub trait SnapshotWriterSession {
    type Read: SnapshotRead;
    type Write: SnapshotWrite;

    /// Open the current snapshot for reading within this writer session.
    ///
    /// # Errors
    ///
    /// Returns an error if the current snapshot cannot be opened or leased.
    fn current(&self) -> crate::Result<Option<Self::Read>>;

    /// Begin a new in-progress snapshot write.
    ///
    /// # Errors
    ///
    /// Returns an error if the write transaction cannot be started.
    fn begin(&mut self) -> crate::Result<Self::Write>;

    /// Commit `write` and make it the current snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error if publishing the manifest or updating `CURRENT` fails.
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

    /// Open the current snapshot for reading.
    ///
    /// # Errors
    ///
    /// Returns an error if the current snapshot cannot be opened or leased.
    fn current(&self) -> crate::Result<Option<Self::Read>>;

    /// Acquire exclusive write access to the store.
    ///
    /// # Errors
    ///
    /// Returns an error if the write lock cannot be acquired.
    fn writer(&mut self) -> crate::Result<Self::Writer<'_>>;
}

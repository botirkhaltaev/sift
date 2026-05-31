pub mod artifact;
pub mod identity;
pub mod lease;
pub mod manifest;
pub mod opened;
pub mod store;

pub use artifact::ArtifactData;
pub use identity::SnapshotId;
pub use lease::SnapshotLease;
pub use manifest::SnapshotManifest;
pub use opened::Snapshot;
pub use store::disk::DiskSnapshotStore;
pub use store::{SnapshotRead, SnapshotStore, SnapshotWrite, SnapshotWriterSession};

#[cfg(test)]
mod tests;

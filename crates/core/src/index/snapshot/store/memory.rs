use std::collections::BTreeMap;
use std::sync::Arc;

use super::super::artifact::ArtifactData;
use super::super::identity::SnapshotId;
use super::super::manifest::SnapshotManifest;
use super::{SnapshotRead, SnapshotStore, SnapshotWrite, SnapshotWriterSession};

/// Pure in-memory snapshot store with no filesystem access.
pub struct MemorySnapshotStore {
    snapshots: BTreeMap<SnapshotId, MemorySnapshotData>,
    current_id: Option<SnapshotId>,
    next_id: u64,
}

struct MemorySnapshotData {
    manifest: SnapshotManifest,
    artifacts: BTreeMap<String, BTreeMap<String, Arc<[u8]>>>,
}

impl MemorySnapshotStore {
    pub fn new() -> Self {
        Self {
            snapshots: BTreeMap::new(),
            current_id: None,
            next_id: 0,
        }
    }
}

impl Default for MemorySnapshotStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Writer session for [`MemorySnapshotStore`] (no-op locking).
pub struct MemorySnapshotWriterSession<'a> {
    store: &'a mut MemorySnapshotStore,
}

impl SnapshotWriterSession for MemorySnapshotWriterSession<'_> {
    type Read = MemorySnapshotReader;
    type Write = MemorySnapshotWriter;

    fn current(&self) -> crate::Result<Option<Self::Read>> {
        let Some(ref id) = self.store.current_id else {
            return Ok(None);
        };
        let data = self.store.snapshots.get(id).ok_or_else(|| {
            crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "current snapshot missing from in-memory store",
            ))
        })?;
        Ok(Some(MemorySnapshotReader {
            manifest: data.manifest.clone(),
            artifacts: data.artifacts.clone(),
        }))
    }

    fn begin(&mut self) -> crate::Result<Self::Write> {
        let id_num = self.store.next_id;
        self.store.next_id += 1;
        let id = SnapshotId::new(format!("mem-{id_num:020x}"));
        Ok(MemorySnapshotWriter {
            id,
            artifacts: BTreeMap::new(),
        })
    }

    fn publish(
        &mut self,
        write: Self::Write,
        manifest: SnapshotManifest,
    ) -> crate::Result<SnapshotId> {
        let data = MemorySnapshotData {
            manifest,
            artifacts: write.artifacts,
        };
        let id = write.id;
        self.store.snapshots.insert(id.clone(), data);
        self.store.current_id = Some(id.clone());
        Ok(id)
    }
}

/// An in-progress snapshot write backed by a memory buffer.
pub struct MemorySnapshotWriter {
    id: SnapshotId,
    artifacts: BTreeMap<String, BTreeMap<String, Arc<[u8]>>>,
}

impl SnapshotWrite for MemorySnapshotWriter {
    fn id(&self) -> &SnapshotId {
        &self.id
    }

    fn put_artifact(&mut self, namespace: &str, name: &str, bytes: Vec<u8>) -> crate::Result<()> {
        self.artifacts
            .entry(namespace.to_string())
            .or_default()
            .insert(name.to_string(), Arc::from(bytes));
        Ok(())
    }
}

/// A readable snapshot backed by in-memory buffers.
#[derive(Clone)]
pub struct MemorySnapshotReader {
    manifest: SnapshotManifest,
    artifacts: BTreeMap<String, BTreeMap<String, Arc<[u8]>>>,
}

impl SnapshotRead for MemorySnapshotReader {
    fn manifest(&self) -> &SnapshotManifest {
        &self.manifest
    }

    fn artifact(&self, namespace: &str, name: &str) -> crate::Result<ArtifactData> {
        let ns = self.artifacts.get(namespace).ok_or_else(|| {
            crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("namespace {namespace} not found"),
            ))
        })?;
        let bytes = ns.get(name).ok_or_else(|| {
            crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("artifact {namespace}/{name} not found"),
            ))
        })?;
        Ok(ArtifactData::Memory(bytes.clone()))
    }
}

impl SnapshotStore for MemorySnapshotStore {
    type Read = MemorySnapshotReader;
    type Write = MemorySnapshotWriter;
    type Writer<'a> = MemorySnapshotWriterSession<'a>;

    fn current_id(&self) -> Option<&SnapshotId> {
        self.current_id.as_ref()
    }

    fn current(&self) -> crate::Result<Option<Self::Read>> {
        let Some(ref id) = self.current_id else {
            return Ok(None);
        };
        let data = self.snapshots.get(id).ok_or_else(|| {
            crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "current snapshot missing from in-memory store",
            ))
        })?;
        Ok(Some(MemorySnapshotReader {
            manifest: data.manifest.clone(),
            artifacts: data.artifacts.clone(),
        }))
    }

    fn writer(&mut self) -> crate::Result<Self::Writer<'_>> {
        Ok(MemorySnapshotWriterSession { store: self })
    }
}

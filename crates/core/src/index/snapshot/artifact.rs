use std::sync::Arc;

#[derive(Debug)]
pub enum ArtifactData {
    Memory(Arc<[u8]>),
    Mmap(memmap2::Mmap),
}

impl AsRef<[u8]> for ArtifactData {
    fn as_ref(&self) -> &[u8] {
        match self {
            Self::Memory(bytes) => bytes,
            Self::Mmap(mmap) => mmap.as_ref(),
        }
    }
}

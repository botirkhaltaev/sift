use std::path::PathBuf;

/// IPC operation sent to the index daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonOp {
    Watch,
    /// Rel-paths to index. Empty vec = full corpus.
    Index(Vec<PathBuf>),
}

impl DaemonOp {
    pub const WATCH_OPCODE: u8 = 0x01;
    pub const INDEX_OPCODE: u8 = 0x02;
    pub const STATUS_OK: u8 = 0x00;
    pub const STATUS_ERR: u8 = 0x01;
}

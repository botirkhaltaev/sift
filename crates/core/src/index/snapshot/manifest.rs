use serde::{Deserialize, Serialize};

use super::identity::SnapshotId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotManifest {
    pub id: SnapshotId,
    pub indexes: Vec<String>,
}

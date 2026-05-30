use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::search::filter::VisibilityConfig;

/// Whether the index was built from a directory or a single file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CorpusKind {
    /// Built from a directory path — all discovered files were indexed.
    #[default]
    Directory,
    /// Built from a single file path — only that file was indexed.
    SingleFile,
}

/// Configuration for building or updating an index over a corpus.
pub struct IndexConfig<'a> {
    pub corpus: CorpusSpec<'a>,
    pub visibility: VisibilityConfig,
}

/// Description of a corpus to index.
pub struct CorpusSpec<'a> {
    pub root: &'a Path,
    pub kind: CorpusKind,
    pub follow_links: bool,
    pub include_paths: &'a [PathBuf],
    pub exclude_paths: &'a [PathBuf],
}

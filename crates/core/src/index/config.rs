use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::grep::filter::VisibilityConfig;

/// Whether the index was built from a directory or a single file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CorpusKind {
    /// Built from a directory path — all discovered files were indexed.
    #[default]
    Directory,
    /// Built from a single file path — only that file was indexed.
    SingleFile,
}

/// Filesystem walk behavior for index builds and updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexWalkConfig {
    pub follow_links: bool,
    pub one_file_system: bool,
    pub max_depth: Option<usize>,
    pub max_filesize: Option<u64>,
}

impl IndexWalkConfig {
    #[must_use]
    pub const fn new(follow_links: bool) -> Self {
        Self {
            follow_links,
            one_file_system: false,
            max_depth: None,
            max_filesize: None,
        }
    }
}

/// Corpus and filesystem inputs for building or updating index artifacts.
pub struct IndexBuildConfig<'a> {
    pub corpus: CorpusSpec<'a>,
    pub walk: IndexWalkConfig,
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

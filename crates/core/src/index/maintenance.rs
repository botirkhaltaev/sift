use std::path::{Path, PathBuf};

use crate::index::CorpusKind;

pub struct IndexBuildConfig<'a> {
    pub root: &'a Path,
    pub follow_links: bool,
    pub exclude_paths: &'a [PathBuf],
    pub include_paths: &'a [PathBuf],
    pub corpus_kind: CorpusKind,
}

pub trait IndexMaintenance: Sync + Send {
    type Index: super::SearchIndex + 'static;

    const NAME: &'static str;

    /// Build a new index over the corpus described in `config`, writing
    /// persistence files into `output_dir`.
    ///
    /// # Errors
    ///
    /// Returns an error if corpus walking, trigram extraction, or file I/O fails.
    fn build(config: &IndexBuildConfig<'_>, output_dir: &Path) -> crate::Result<Self::Index>;

    /// Open an index that was previously persisted to `index_dir`.
    ///
    /// # Errors
    ///
    /// Returns an error if persistence files are missing or malformed.
    fn open(index_dir: &Path, root: &Path, corpus_kind: CorpusKind) -> crate::Result<Self::Index>;
}

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::config::{CorpusKind, IndexConfig};
use super::trigram;
use super::{IndexDestination, IndexSource};

/// How an index query plan resolves candidates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PlanMode {
    /// The query was narrowed using trigram candidates from the index.
    #[default]
    IndexedCandidates,
    /// No trigrams were usable — all indexed files must be scanned.
    FullScan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryPlanOutput {
    pub pattern: String,
    pub mode: PlanMode,
}

/// Tag identifying an index kind for lifecycle dispatch (build, open, update).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IndexKind {
    Trigram,
}

impl IndexKind {
    pub const ALL: &[Self] = &[Self::Trigram];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Trigram => "trigram",
        }
    }

    /// Return the artifact file names that this index kind produces.
    #[must_use]
    pub(crate) const fn artifact_names(self) -> &'static [&'static str] {
        match self {
            Self::Trigram => &[
                crate::FILES_BIN,
                crate::LEXICON_BIN,
                crate::POSTINGS_BIN,
                crate::TRIGRAMS_BIN,
            ],
        }
    }

    /// Build this index kind into the given destination.
    pub(crate) fn build(
        self,
        config: &IndexConfig<'_>,
        dest: IndexDestination,
        paths: &[PathBuf],
    ) -> crate::Result<()> {
        match self {
            Self::Trigram => {
                let tables = trigram::builder::IndexTables::build(config, paths)?;
                let root = config.corpus.root.canonicalize()?;
                trigram::TrigramIndex::persist_tables(&tables, &root, config.corpus.kind, dest)?;
                Ok(())
            }
        }
    }

    /// Open this index kind from the given source.
    pub(crate) fn open(
        self,
        source: IndexSource,
        root: &Path,
        corpus_kind: CorpusKind,
    ) -> crate::Result<Index> {
        match self {
            Self::Trigram => Ok(Index::Trigram(trigram::TrigramIndex::open_tables(
                source,
                root,
                corpus_kind,
            )?)),
        }
    }

    /// Update this index kind from the current snapshot, writing to `dest`.
    ///
    /// Returns `true` if a new index was written. If the index kind is not
    /// present in the current snapshot, it is built from scratch.
    pub(crate) fn update(
        self,
        current: IndexSource,
        config: &IndexConfig<'_>,
        dest: IndexDestination,
        paths: &[PathBuf],
    ) -> crate::Result<bool> {
        let is_present = match &current {
            IndexSource::Directory(dir) => dir.exists(),
            IndexSource::Snapshot { reader, namespace } => {
                reader.manifest().indexes.iter().any(|n| n == *namespace)
            }
        };
        if !is_present {
            self.build(config, dest, paths)?;
            return Ok(true);
        }

        let root = config
            .corpus
            .root
            .canonicalize()
            .unwrap_or_else(|_| config.corpus.root.to_path_buf());
        match self {
            Self::Trigram => {
                let existing =
                    trigram::TrigramIndex::open_tables(current, &root, config.corpus.kind)?;
                let output = existing.rebuild(config, dest, paths)?;
                Ok(output.is_some())
            }
        }
    }
}

impl std::fmt::Display for IndexKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for IndexKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "trigram" => Ok(Self::Trigram),
            other => Err(format!("unknown index kind: {other}")),
        }
    }
}

/// An opened index instance, used for search-time dispatch.
pub enum Index {
    Trigram(trigram::TrigramIndex),
}

impl Index {
    #[must_use]
    pub fn root(&self) -> &Path {
        match self {
            Self::Trigram(idx) => idx.root(),
        }
    }

    #[must_use]
    pub const fn corpus_kind(&self) -> CorpusKind {
        match self {
            Self::Trigram(idx) => idx.corpus_kind(),
        }
    }

    #[must_use]
    pub fn candidates(&self, query: &crate::query::QuerySpec<'_>) -> Option<Vec<crate::Candidate>> {
        match self {
            Self::Trigram(idx) => idx.candidates(query),
        }
    }

    #[must_use]
    pub(crate) fn all_files(&self) -> Vec<crate::Candidate> {
        match self {
            Self::Trigram(idx) => idx.all_files(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileId(usize);

impl FileId {
    #[must_use]
    pub const fn new(value: usize) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IndexId(usize);

impl IndexId {
    #[must_use]
    pub const fn new(value: usize) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

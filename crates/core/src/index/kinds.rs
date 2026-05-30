use std::path::Path;

use serde::{Deserialize, Serialize};

use super::config::{CorpusKind, IndexConfig};
use super::trigram;

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

    pub(crate) fn build(self, config: &IndexConfig<'_>, output_dir: &Path) -> crate::Result<()> {
        match self {
            Self::Trigram => {
                trigram::TrigramIndex::build(config, output_dir)?;
                Ok(())
            }
        }
    }

    pub(crate) fn open_from_dir(
        self,
        index_dir: &Path,
        root: &Path,
        corpus_kind: CorpusKind,
    ) -> crate::Result<Index> {
        match self {
            Self::Trigram => Ok(Index::Trigram(trigram::TrigramIndex::open(
                index_dir,
                root,
                corpus_kind,
            )?)),
        }
    }

    /// Returns `true` if a new index was written.
    pub(crate) fn update(
        self,
        snapshot_dir: &Path,
        config: &IndexConfig<'_>,
        output_dir: &Path,
    ) -> crate::Result<bool> {
        let existing_dir = snapshot_dir.join(self.as_str());
        if !existing_dir.exists() {
            self.build(config, output_dir)?;
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
                    trigram::TrigramIndex::open(&existing_dir, &root, config.corpus.kind)?;
                Ok(existing.update(config, output_dir)?.is_some())
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

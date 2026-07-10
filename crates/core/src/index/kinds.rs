use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::config::{CorpusKind, IndexBuildConfig};
use super::ngram;
use super::{IndexDestination, IndexSource};

/// How an index query plan resolves candidates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PlanMode {
    /// The query was narrowed using indexed candidates.
    #[default]
    IndexedCandidates,
    /// No index terms were usable, so all indexed files must be scanned.
    FullScan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryPlanOutput {
    pub pattern: String,
    pub mode: PlanMode,
}

/// How an opened index covers a query.
#[derive(Debug)]
pub enum CandidatePlan {
    /// The query has no usable index terms.
    Unavailable,
    /// Every indexed file is a possible match, so the index cannot narrow further.
    AllIndexed,
    /// A narrowed set of possible matching files.
    Narrowed(Vec<crate::Candidate>),
}

impl CandidatePlan {
    #[must_use]
    pub const fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable)
    }
}

/// Configured index identity persisted in metadata and snapshot manifests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IndexConfig {
    /// Runtime-width N-gram index configuration.
    NGram(ngram::Config),
}

impl IndexConfig {
    pub const ALL: &[Self] = &[Self::NGram(ngram::Config::DEFAULT)];

    #[must_use]
    pub const fn ngram(width: ngram::GramWidth) -> Self {
        Self::NGram(ngram::Config::new(width))
    }

    #[must_use]
    pub fn name(self) -> String {
        match self {
            Self::NGram(config) => config.name(),
        }
    }

    #[must_use]
    pub(crate) const fn artifact_names(self) -> &'static [&'static str] {
        match self {
            Self::NGram(config) => config.artifact_names(),
        }
    }

    pub(crate) fn build(
        self,
        build: &IndexBuildConfig<'_>,
        dest: IndexDestination,
        paths: &[PathBuf],
    ) -> crate::Result<()> {
        match self {
            Self::NGram(config) => {
                config.build_into(build, dest, paths)?;
                Ok(())
            }
        }
    }

    pub(crate) fn open(
        self,
        source: IndexSource,
        root: &Path,
        corpus_kind: CorpusKind,
    ) -> crate::Result<Index> {
        match self {
            Self::NGram(config) => Ok(Index::NGram(config.open_from(source, root, corpus_kind)?)),
        }
    }

    pub(crate) fn update(
        self,
        current: IndexSource,
        build: &IndexBuildConfig<'_>,
        dest: IndexDestination,
        paths: &[PathBuf],
    ) -> crate::Result<bool> {
        let is_present = match &current {
            IndexSource::Directory(dir) => dir.exists(),
            IndexSource::Snapshot { reader, namespace } => reader
                .manifest()
                .indexes
                .iter()
                .any(|name| name == *namespace),
        };
        if !is_present {
            self.build(build, dest, paths)?;
            return Ok(true);
        }

        let root = build
            .corpus
            .root
            .canonicalize()
            .unwrap_or_else(|_| build.corpus.root.to_path_buf());
        match self {
            Self::NGram(config) => {
                let existing = config.open_from(current, &root, build.corpus.kind)?;
                let output = existing.rebuild(build, dest, paths)?;
                Ok(output.is_some())
            }
        }
    }
}

impl std::fmt::Display for IndexConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&(*self).name())
    }
}

impl Serialize for IndexConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&(*self).name())
    }
}

impl<'de> Deserialize<'de> for IndexConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

impl std::str::FromStr for IndexConfig {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "trigram" => Ok(Self::ngram(ngram::GramWidth::TRIGRAM)),
            other => ngram::Config::parse_name(other).map(Self::NGram),
        }
    }
}

/// Opened runtime index used for query-time candidate narrowing.
#[derive(Debug)]
pub enum Index {
    /// Runtime-width N-gram index.
    NGram(ngram::Index),
}

impl Index {
    #[must_use]
    pub fn root(&self) -> &Path {
        match self {
            Self::NGram(index) => index.root(),
        }
    }

    #[must_use]
    pub const fn corpus_kind(&self) -> CorpusKind {
        match self {
            Self::NGram(index) => index.corpus_kind(),
        }
    }

    #[must_use]
    pub fn plan(&self, query: &crate::candidates::CandidateSpec<'_>) -> CandidatePlan {
        match self {
            Self::NGram(index) => index.plan(query),
        }
    }

    #[must_use]
    pub(crate) fn all_files(&self) -> Vec<crate::Candidate> {
        match self {
            Self::NGram(index) => index.all_files(),
        }
    }

    #[must_use]
    pub(crate) fn indexed_rel_paths(&self) -> HashSet<PathBuf> {
        match self {
            Self::NGram(index) => index.indexed_rel_paths(),
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

use crate::Candidate;
use crate::search::output::SearchOutput;
use crate::search::output::style::SearchSeparators;

#[derive(Clone)]
pub struct CandidateContent {
    pub candidate: Candidate,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Copy)]
pub struct StreamInput<'a> {
    pub display_path: &'a str,
    pub bytes: &'a [u8],
}

#[derive(Clone, Copy)]
pub enum SearchInput<'a> {
    Candidates(&'a [Candidate]),
    Transformed(&'a [CandidateContent]),
    Stream(StreamInput<'a>),
}

impl SearchInput<'_> {
    #[must_use]
    pub const fn count(self) -> usize {
        match self {
            Self::Candidates(candidates) => candidates.len(),
            Self::Transformed(contents) => contents.len(),
            Self::Stream(_) => 1,
        }
    }

    #[must_use]
    pub fn bytes(self) -> u64 {
        match self {
            Self::Candidates(candidates) => Candidate::total_file_bytes(candidates),
            Self::Transformed(contents) => contents
                .iter()
                .map(|content| u64::try_from(content.bytes.len()).unwrap_or(u64::MAX))
                .sum(),
            Self::Stream(input) => u64::try_from(input.bytes.len()).unwrap_or(u64::MAX),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LinkTraversal {
    #[default]
    DoNotFollow,
    Follow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WalkOptions {
    pub links: LinkTraversal,
    pub max_depth: Option<usize>,
    pub max_filesize: Option<u64>,
    pub one_file_system: bool,
}

#[derive(Clone)]
pub struct SearchExecution<'a> {
    pub inputs: Vec<SearchInput<'a>>,
    pub output: SearchOutput,
    pub separators: &'a SearchSeparators,
    pub collect: SearchCollection,
}

/// Optional artifacts gathered during search beyond primary output.
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchCollection {
    pub stats: bool,
    pub hits: bool,
}

impl Default for SearchCollection {
    fn default() -> Self {
        Self::none()
    }
}

impl SearchCollection {
    pub const fn none() -> Self {
        Self {
            stats: false,
            hits: false,
        }
    }

    pub const fn stats() -> Self {
        Self {
            stats: true,
            hits: false,
        }
    }

    pub const fn hits() -> Self {
        Self {
            stats: false,
            hits: true,
        }
    }

    pub const fn stats_and_hits() -> Self {
        Self {
            stats: true,
            hits: true,
        }
    }

    pub const fn with_stats(self, stats: bool) -> Self {
        Self {
            stats,
            hits: self.hits,
        }
    }

    pub const fn with_hits(self, hits: bool) -> Self {
        Self {
            stats: self.stats,
            hits,
        }
    }
}

use crate::corpus::CandidateOrder;

use super::IndexNarrowing;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexFallback {
    IndexHitsOnly,
    WalkOnStaleSnapshot,
}

impl IndexFallback {
    #[must_use]
    pub const fn walk_on_stale(self) -> bool {
        matches!(self, Self::WalkOnStaleSnapshot)
    }
}

/// Which files a search run should consider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CandidateScope {
    /// No corpus files; search only explicit byte streams.
    None,
    /// All corpus files (complete coverage).
    All,
    /// Only index-narrowed potential matches.
    #[default]
    Indexed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorpusMode {
    Indexed,
    Walk,
    Transformed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateExtent {
    PotentialMatches,
    Complete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateSelection {
    Corpus {
        corpus: CorpusMode,
        fallback: IndexFallback,
        order: CandidateOrder,
    },
    None,
}

#[derive(Debug, Clone, Copy)]
pub struct CandidateRequest {
    pub scope: CandidateScope,
    pub corpus: CorpusMode,
    pub fallback: IndexFallback,
    pub order: CandidateOrder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResolvedCandidateRequest {
    pub scope: CandidateScope,
    pub fallback: IndexFallback,
    pub order: CandidateOrder,
}

impl CandidateRequest {
    #[must_use]
    pub(crate) const fn resolve(self, index_narrowing: IndexNarrowing) -> ResolvedCandidateRequest {
        let scope = if matches!(self.scope, CandidateScope::None) {
            CandidateScope::None
        } else {
            match self.corpus {
                CorpusMode::Walk | CorpusMode::Transformed => CandidateScope::All,
                CorpusMode::Indexed => match index_narrowing {
                    IndexNarrowing::Disabled => CandidateScope::All,
                    IndexNarrowing::Enabled => self.scope,
                },
            }
        };
        ResolvedCandidateRequest {
            scope,
            fallback: self.fallback,
            order: self.order,
        }
    }
}

impl CandidateSelection {
    #[must_use]
    pub fn request(self, extent: CandidateExtent) -> CandidateRequest {
        self.request_for_scope(extent.scope())
    }

    #[must_use]
    pub fn request_for_scope(self, scope: CandidateScope) -> CandidateRequest {
        match self {
            Self::Corpus {
                corpus,
                fallback,
                order,
            } => CandidateRequest {
                scope,
                corpus,
                fallback,
                order,
            },
            Self::None => CandidateRequest {
                scope: CandidateScope::None,
                corpus: CorpusMode::Walk,
                fallback: IndexFallback::IndexHitsOnly,
                order: CandidateOrder::default(),
            },
        }
    }
}

impl CandidateExtent {
    const fn scope(self) -> CandidateScope {
        match self {
            Self::PotentialMatches => CandidateScope::Indexed,
            Self::Complete => CandidateScope::All,
        }
    }
}

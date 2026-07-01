use crate::corpus::CandidateCoverage;
use crate::corpus::CandidateOrder;
use crate::grep::engine::{CompiledQuery, Indexability};
use crate::query::ResolutionFallback;

pub use crate::query::ResolutionFallback as IndexFallback;

/// Which files a search run should consider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CandidateScope {
    /// All corpus files (complete coverage).
    All,
    /// Only index-narrowed potential matches.
    #[default]
    Indexed,
}

impl CandidateScope {
    #[must_use]
    pub const fn coverage(self) -> CandidateCoverage {
        match self {
            Self::All => CandidateCoverage::Complete,
            Self::Indexed => CandidateCoverage::PotentialMatches,
        }
    }

    #[must_use]
    pub const fn resolve(
        output_scope: Self,
        indexability: Indexability,
        corpus: CorpusState,
    ) -> Self {
        match corpus {
            CorpusState::Unindexed | CorpusState::TransformedBytes => Self::All,
            CorpusState::Indexed => match indexability {
                Indexability::Complete(_) => Self::All,
                Indexability::Indexed => output_scope,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorpusState {
    Indexed,
    Unindexed,
    TransformedBytes,
}

#[derive(Debug, Clone, Copy)]
pub struct CandidatePolicyConfig {
    pub output_scope: CandidateScope,
    pub corpus: CorpusState,
    pub fallback: ResolutionFallback,
    pub order: CandidateOrder,
}

/// Per-run candidate selection policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CandidatePolicy {
    pub scope: CandidateScope,
    pub corpus: CorpusState,
    pub fallback: ResolutionFallback,
    pub order: CandidateOrder,
}

impl CandidatePolicyConfig {
    #[must_use]
    pub const fn policy(self, compiled: &CompiledQuery) -> CandidatePolicy {
        let scope =
            CandidateScope::resolve(self.output_scope, compiled.indexability(), self.corpus);
        CandidatePolicy {
            scope,
            corpus: self.corpus,
            fallback: self.fallback,
            order: self.order,
        }
    }
}

impl CandidatePolicy {
    #[must_use]
    pub const fn coverage(self) -> CandidateCoverage {
        self.scope.coverage()
    }
}

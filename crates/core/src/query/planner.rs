use crate::Candidate;
use crate::index::Indexes;
use crate::query::QuerySpec;

/// Whether search needs all candidate paths or only potential matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateRequirement {
    Complete,
    PotentialMatches,
}

/// Plans candidate selection by combining indexes and a lazy base provider.
pub struct QueryPlanner<'a> {
    spec: QuerySpec<'a>,
}

impl<'a> QueryPlanner<'a> {
    #[must_use]
    pub const fn new(spec: QuerySpec<'a>) -> Self {
        Self { spec }
    }

    /// Resolve candidates using indexes or the lazy base provider.
    ///
    /// # Errors
    ///
    /// Delegates to `base` when fallback is triggered; returns `base` errors unchanged.
    pub fn candidates(
        &self,
        indexes: &Indexes,
        requirement: CandidateRequirement,
        base: impl FnOnce() -> crate::Result<Vec<Candidate>>,
    ) -> crate::Result<Vec<Candidate>> {
        match requirement {
            CandidateRequirement::Complete => {
                if indexes.is_empty() {
                    base()
                } else {
                    Ok(indexes.complete_candidates())
                }
            }
            CandidateRequirement::PotentialMatches => {
                if indexes.is_empty() {
                    return base();
                }
                indexes.candidates(&self.spec).map_or_else(base, Ok)
            }
        }
    }
}

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
    pub const fn new(spec: QuerySpec<'a>) -> Self {
        Self { spec }
    }

    pub fn candidates(
        &self,
        indexes: &Indexes,
        requirement: CandidateRequirement,
        base: impl FnOnce() -> crate::Result<Vec<Candidate>>,
    ) -> crate::Result<Vec<Candidate>> {
        let use_base = match requirement {
            CandidateRequirement::Complete => true,
            CandidateRequirement::PotentialMatches => {
                indexes.is_empty() || indexes.candidates(&self.spec).is_none()
            }
        };

        if use_base {
            base()
        } else {
            // SAFETY: we already confirmed indexes.candidates returned Some.
            Ok(indexes.candidates(&self.spec).unwrap())
        }
    }
}

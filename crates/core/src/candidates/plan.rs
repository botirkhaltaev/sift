use std::collections::HashSet;
use std::path::PathBuf;

use crate::corpus::Candidate;
use crate::corpus::filter::FilterAdmission;
use crate::corpus::order::CandidateOrder;
use crate::corpus::walk::FileWalk;
use crate::index::kinds::IndexQueryResult;

use crate::candidates::source::CandidateSource;

use super::collection::Candidates;

/// The execution plan for candidate resolution.
#[must_use]
pub(crate) struct CandidatePlan {
    pub discovery: PlannedDiscovery,
    pub order: CandidateOrder,
    pub query_result: IndexQueryResult,
}

/// How candidate discovery will run at resolve time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlannedDiscovery {
    Empty,
    Walk,
    Index {
        admission: FilterAdmission,
    },
    /// Index hits merged with a walk of unindexed paths (lazy snapshots).
    Merge {
        admission: FilterAdmission,
    },
}

impl CandidatePlan {
    /// Run the plan against storage: filesystem walk and/or index lookups.
    ///
    /// # Errors
    ///
    /// Returns an error if filesystem walking or ordering fails.
    pub(crate) fn resolve<'a>(
        self,
        source: &'a CandidateSource<'a>,
    ) -> crate::Result<Candidates<'a>> {
        let Self {
            discovery,
            order,
            query_result,
        } = self;
        let candidates = match discovery {
            PlannedDiscovery::Empty => Candidates::empty(),
            PlannedDiscovery::Walk => Candidates::from(Self::walk(source)?),
            PlannedDiscovery::Index { admission } => {
                source
                    .indexes
                    .indexed_candidates(query_result, source.filter, admission)
            }
            PlannedDiscovery::Merge { admission } => {
                Candidates::from(Self::merge(source, query_result, admission)?)
            }
        };
        Self::order(candidates, order)
    }

    fn walk(source: &CandidateSource<'_>) -> crate::Result<Vec<Candidate>> {
        let walked = FileWalk::from_filter(source.filter).candidates()?;
        Ok(source.filter.retain(walked, FilterAdmission::Full))
    }

    fn merge(
        source: &CandidateSource<'_>,
        query_result: IndexQueryResult,
        admission: FilterAdmission,
    ) -> crate::Result<Vec<Candidate>> {
        let IndexQueryResult::Matched { file_ids } = query_result else {
            return Self::walk(source);
        };
        let mut candidates = source
            .indexes
            .hydrate_rows(&file_ids, source.filter, admission);
        let walked = FileWalk::from_filter(source.filter)
            .candidates_matching(source.indexes.indexed_corpus().unindexed_files())?;
        let walked = source.filter.retain(walked, FilterAdmission::Full);
        let mut seen: HashSet<PathBuf> = candidates
            .iter()
            .map(|candidate| candidate.rel_path().to_path_buf())
            .collect();
        for candidate in walked {
            if seen.insert(candidate.rel_path().to_path_buf()) {
                candidates.push(candidate);
            }
        }
        Ok(candidates)
    }

    fn order(candidates: Candidates<'_>, order: CandidateOrder) -> crate::Result<Candidates<'_>> {
        if !order.is_sorted() {
            return Ok(candidates);
        }
        let mut items = candidates.into_vec();
        order.order(&mut items)?;
        Ok(Candidates::from(items))
    }
}

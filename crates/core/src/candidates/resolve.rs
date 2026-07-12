use std::collections::HashSet;
use std::path::PathBuf;

use crate::corpus::Candidate;
use crate::corpus::filter::FilterAdmission;
use crate::corpus::order::CandidateOrder;
use crate::corpus::walk::FileWalk;
use crate::index::kinds::NarrowingResult;

use super::plan::{CandidatePlan, PlannedDiscovery};
use super::resolved::Candidates;
use super::source::CandidateSource;

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
            narrowing,
        } = self;
        let candidates = match discovery {
            PlannedDiscovery::Empty => Candidates::empty(),
            PlannedDiscovery::Walk => Candidates::from(walk_candidates(source)?),
            PlannedDiscovery::Index { admission } => Candidates::from(
                source
                    .indexes
                    .index_file_ids(narrowing, source.filter, admission),
            ),
            PlannedDiscovery::Merge { admission } => {
                Candidates::from(merge_index_and_walk(source, narrowing, admission)?)
            }
        };
        apply_order(candidates, order)
    }
}

fn walk_candidates(source: &CandidateSource<'_>) -> crate::Result<Vec<Candidate>> {
    let walked = FileWalk::from_filter(source.filter).candidates()?;
    Ok(source.filter.retain(walked, FilterAdmission::Full))
}

fn merge_index_and_walk(
    source: &CandidateSource<'_>,
    narrowing: NarrowingResult,
    admission: FilterAdmission,
) -> crate::Result<Vec<Candidate>> {
    let NarrowingResult::Narrowed { file_ids } = narrowing else {
        return walk_candidates(source);
    };
    let mut candidates = source
        .indexes
        .materialize_rows(&file_ids, source.filter, admission);
    let walked = source.indexes.unindexed_walk_candidates(source.filter)?;
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

fn apply_order(candidates: Candidates<'_>, order: CandidateOrder) -> crate::Result<Candidates<'_>> {
    if !order.is_sorted() {
        return Ok(candidates);
    }
    let mut items = candidates.into_vec();
    order.order(&mut items)?;
    Ok(Candidates::from(items))
}

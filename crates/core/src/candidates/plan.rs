use std::collections::HashSet;
use std::path::PathBuf;

use rayon::prelude::*;

use crate::corpus::Candidate;
use crate::corpus::filter::{CandidateFilter, FilterAdmission};
use crate::corpus::order::CandidateOrder;
use crate::corpus::walk::FileWalk;
use crate::index::kinds::NarrowingResult;

use super::collection::Candidates;
use super::query::CandidateQuery;
use super::source::CandidateSource;

/// The execution plan for candidate resolution.
#[must_use]
pub struct CandidatePlan<'src> {
    pub discovery: PlannedDiscovery,
    pub order: CandidateOrder,
    pub(crate) source: &'src CandidateSource<'src>,
    pub(crate) query: &'src CandidateQuery<'src>,
}

/// How candidate discovery will run at resolve time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlannedDiscovery {
    Empty,
    Walk,
    Index {
        admission: FilterAdmission,
    },
    /// Index hits merged with a walk of unindexed paths (lazy snapshots).
    Merge,
}

impl<'src> CandidatePlan<'src> {
    pub(crate) const fn new(
        discovery: PlannedDiscovery,
        order: CandidateOrder,
        source: &'src CandidateSource<'src>,
        query: &'src CandidateQuery<'src>,
    ) -> Self {
        Self {
            discovery,
            order,
            source,
            query,
        }
    }

    /// Run the plan against storage: filesystem walk and/or index lookups.
    ///
    /// # Errors
    ///
    /// Returns an error if filesystem walking or ordering fails.
    pub fn resolve(self) -> crate::Result<Candidates<'src>> {
        let Self {
            discovery,
            order,
            source,
            query,
        } = self;
        let candidates = match discovery {
            PlannedDiscovery::Empty => Candidates::empty(),
            PlannedDiscovery::Walk => {
                let walked = FileWalk::from_filter(source.filter).candidates()?;
                Candidates::from_vec(retain_matching(
                    walked,
                    source.filter,
                    FilterAdmission::Full,
                ))
            }
            PlannedDiscovery::Index { admission } => {
                source.indexes.candidates(query, source.filter, admission)
            }
            PlannedDiscovery::Merge => merge_index_and_walk(source, query)?,
        };
        candidates.order(order)
    }
}

fn merge_index_and_walk<'src>(
    source: &'src CandidateSource<'src>,
    query: &'src CandidateQuery<'src>,
) -> crate::Result<Candidates<'src>> {
    let narrowing = source.indexes.narrow(query);
    let NarrowingResult::Narrowed { file_ids } = narrowing else {
        let walked = FileWalk::from_filter(source.filter).candidates()?;
        return Ok(Candidates::from_vec(retain_matching(
            walked,
            source.filter,
            FilterAdmission::Full,
        )));
    };
    let mut candidates =
        source
            .indexes
            .materialize_rows(&file_ids, source.filter, FilterAdmission::Full);
    let walked = source.indexes.unindexed_walk_candidates(source.filter)?;
    let mut seen: HashSet<PathBuf> = candidates
        .iter()
        .map(|candidate| candidate.rel_path().to_path_buf())
        .collect();
    for candidate in retain_matching(walked, source.filter, FilterAdmission::Full) {
        if seen.insert(candidate.rel_path().to_path_buf()) {
            candidates.push(candidate);
        }
    }
    Ok(Candidates::from_vec(candidates))
}

fn retain_matching(
    candidates: Vec<Candidate>,
    filter: &CandidateFilter,
    admission: FilterAdmission,
) -> Vec<Candidate> {
    candidates
        .into_par_iter()
        .filter(|candidate| candidate.matches(filter, admission))
        .collect()
}

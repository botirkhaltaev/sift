use crate::corpus::Candidate;
use crate::corpus::filter::FilterAdmission;
use crate::corpus::order::{CandidateOrder, CandidateOrderKey};
use crate::corpus::walk::FileWalk;
use crate::index::FileId;
use crate::index::IndexedCorpus;

use crate::candidates::source::CandidateSource;

use super::collection::Candidates;

/// The execution plan for candidate resolution.
#[must_use]
pub(crate) struct CandidatePlan {
    pub discovery: PlannedDiscovery,
    pub order: CandidateOrder,
    pub file_ids: Vec<FileId>,
    pub indexed_corpus: IndexedCorpus,
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
            file_ids,
            indexed_corpus,
        } = self;
        let candidates = match discovery {
            PlannedDiscovery::Empty => Candidates::empty(),
            PlannedDiscovery::Walk => Candidates::from(Self::walk(source)?),
            PlannedDiscovery::Index { admission } => {
                source
                    .indexes
                    .indexed_candidates(file_ids, source.filter, admission)
            }
            PlannedDiscovery::Merge { admission } => {
                Self::merge(source, file_ids, admission, &indexed_corpus)?
            }
        };
        Self::order(candidates, order)
    }

    fn walk(source: &CandidateSource<'_>) -> crate::Result<Vec<Candidate>> {
        let walked = FileWalk::from_filter(source.filter).candidates()?;
        Ok(source.filter.retain(walked, FilterAdmission::Full))
    }

    fn merge<'a>(
        source: &'a CandidateSource<'a>,
        file_ids: Vec<FileId>,
        admission: FilterAdmission,
        indexed_corpus: &IndexedCorpus,
    ) -> crate::Result<Candidates<'a>> {
        let walked = FileWalk::from_filter(source.filter)
            .candidates_matching(indexed_corpus.unindexed_files())?;
        let unindexed = source.filter.retain(walked, FilterAdmission::Full);
        Ok(Candidates::mixed(
            source.indexes,
            file_ids,
            source.filter,
            admission,
            unindexed,
        ))
    }

    fn order(candidates: Candidates<'_>, order: CandidateOrder) -> crate::Result<Candidates<'_>> {
        if !order.is_sorted() {
            return Ok(candidates);
        }
        if matches!(order.key, CandidateOrderKey::Path) {
            match candidates {
                Candidates::Indexed(mut indexed) => {
                    if matches!(
                        order.direction,
                        crate::corpus::order::CandidateOrderDirection::Descending
                    ) {
                        indexed.file_ids.reverse();
                    }
                    return Ok(Candidates::Indexed(indexed));
                }
                other => {
                    let mut items = other.into_vec();
                    order.order(&mut items)?;
                    return Ok(Candidates::from(items));
                }
            }
        }
        let mut items = candidates.into_vec();
        order.order(&mut items)?;
        Ok(Candidates::from(items))
    }
}

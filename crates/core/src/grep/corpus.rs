use crate::grep::candidates::CandidateOrder;
use crate::grep::filter::CandidateFilter;
use crate::grep::input::CandidateContent;
use crate::index::Indexes;
use crate::query::{CandidateSource, SnapshotValidation};
use crate::{Candidate, StoreMeta};

#[derive(Clone, Copy)]
pub struct CandidateIndexState<'a> {
    pub store_meta: Option<&'a StoreMeta>,
    pub snapshot: SnapshotValidation,
}

impl<'a> CandidateIndexState<'a> {
    pub(crate) const fn candidate_source(self) -> CandidateSource<'a> {
        CandidateSource {
            store_meta: self.store_meta,
            snapshot: self.snapshot,
        }
    }
}

pub struct GrepCorpus<'a> {
    pub(crate) indexes: &'a Indexes,
    pub(crate) filter: &'a CandidateFilter,
    pub(crate) index_state: CandidateIndexState<'a>,
    pub(crate) order: CandidateOrder,
    pub(crate) content_source: Option<&'a dyn CandidateContentSource>,
}

impl<'a> GrepCorpus<'a> {
    #[must_use]
    pub fn new(
        indexes: &'a Indexes,
        filter: &'a CandidateFilter,
        index_state: CandidateIndexState<'a>,
    ) -> Self {
        Self {
            indexes,
            filter,
            index_state,
            order: CandidateOrder::default(),
            content_source: None,
        }
    }

    #[must_use]
    pub const fn order(mut self, order: CandidateOrder) -> Self {
        self.order = order;
        self
    }

    #[must_use]
    pub fn content_source(mut self, source: Option<&'a dyn CandidateContentSource>) -> Self {
        self.content_source = source;
        self
    }
}

pub trait CandidateContentSource {
    /// # Errors
    ///
    /// Returns an error if transformed content cannot be read for any candidate.
    fn read(&self, candidates: &[Candidate]) -> crate::Result<Vec<CandidateContent>>;
}

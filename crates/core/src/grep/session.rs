use crate::StoreMeta;
use crate::corpus::filter::CandidateFilter;
use crate::index::Indexes;

/// Open index store, filter, and optional store metadata for a search run.
pub struct Session<'a> {
    pub indexes: &'a Indexes,
    pub filter: &'a CandidateFilter,
    pub store_meta: Option<&'a StoreMeta>,
}

impl<'a> Session<'a> {
    #[must_use]
    pub const fn new(
        indexes: &'a Indexes,
        filter: &'a CandidateFilter,
        store_meta: Option<&'a StoreMeta>,
    ) -> Self {
        Self {
            indexes,
            filter,
            store_meta,
        }
    }
}

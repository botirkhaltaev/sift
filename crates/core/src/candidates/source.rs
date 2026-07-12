use crate::StoreMeta;
use crate::corpus::filter::CandidateFilter;
use crate::index::Indexes;

use super::scope::{IndexNarrowing, ScanScope};

/// Indexes, filters, and metadata used to resolve candidate files.
pub struct CandidateSource<'a> {
    pub indexes: &'a Indexes,
    pub filter: &'a CandidateFilter,
    pub store_meta: Option<&'a StoreMeta>,
    pub scope: ScanScope,
    pub index_narrowing: IndexNarrowing,
}

impl<'a> CandidateSource<'a> {
    #[must_use]
    pub const fn new(
        indexes: &'a Indexes,
        filter: &'a CandidateFilter,
        store_meta: Option<&'a StoreMeta>,
        scope: ScanScope,
        index_narrowing: IndexNarrowing,
    ) -> Self {
        Self {
            indexes,
            filter,
            store_meta,
            scope,
            index_narrowing,
        }
    }
}

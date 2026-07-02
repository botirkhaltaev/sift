use crate::StoreMeta;
use crate::corpus::filter::CandidateFilter;
use crate::index::Indexes;

/// Indexes, filters, and metadata used to resolve candidate files.
pub struct CandidateSource<'a> {
    pub indexes: &'a Indexes,
    pub filter: &'a CandidateFilter,
    pub store_meta: Option<&'a StoreMeta>,
}

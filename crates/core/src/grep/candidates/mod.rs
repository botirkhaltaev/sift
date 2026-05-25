pub mod indexed;
pub mod walk;

use crate::grep::filter::{CandidateInfo, SearchFilter};
use crate::index::Indexes;
use crate::query::QuerySpec;

pub fn resolve_indexed_candidates(
    indexes: &Indexes,
    spec: &QuerySpec<'_>,
    filter: &SearchFilter,
) -> Vec<CandidateInfo> {
    let candidates = indexes.resolve_candidates(spec);
    indexed::prepare_candidates(candidates, filter)
}

pub fn resolve_all_indexed_candidates(
    indexes: &Indexes,
    filter: &SearchFilter,
) -> Vec<CandidateInfo> {
    let candidates = indexes.resolve_all_files();
    indexed::prepare_candidates(candidates, filter)
}

use std::path::Path;

use rayon::prelude::*;

use crate::index::SearchCandidate;
use crate::search::filter::{CandidateInfo, SearchFilter};

pub fn prepare_candidates(
    candidates: Vec<SearchCandidate>,
    filter: &SearchFilter,
) -> Vec<CandidateInfo> {
    let need_rel = filter.needs_rel_str_for_matching();
    let max_fs = filter.max_filesize();
    let max_d = filter.max_depth();

    let exceeds_depth = |rel: &Path| -> bool {
        max_d.is_some_and(|d| rel.components().count().saturating_sub(1) > d)
    };

    candidates
        .into_par_iter()
        .filter_map(|candidate| {
            if exceeds_depth(&candidate.rel_path) {
                return None;
            }
            let rel_str = if need_rel {
                candidate.rel_path.to_string_lossy().replace('\\', "/")
            } else {
                String::new()
            };
            if max_fs.is_some_and(|limit| {
                std::fs::metadata(&candidate.abs_path).is_ok_and(|m| m.len() > limit)
            }) {
                return None;
            }
            let info = CandidateInfo {
                rel_path: candidate.rel_path,
                rel_str,
                abs_path: candidate.abs_path,
            };
            filter.is_candidate_info(&info).then_some(info)
        })
        .collect()
}

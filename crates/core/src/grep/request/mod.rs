use crate::grep::candidates::{indexed, walk};
use crate::grep::filter::{CandidateInfo, SearchFilter};
use crate::grep::output::SearchOutput;
use crate::grep::output::mode::CandidateSet;
use crate::grep::output::style::SearchSeparators;
use crate::index::Indexes;
use crate::query::QuerySpec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkTraversal {
    DoNotFollow,
    Follow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalkOptions {
    pub links: LinkTraversal,
    pub max_depth: Option<usize>,
    pub max_filesize: Option<u64>,
    pub one_file_system: bool,
}

impl Default for WalkOptions {
    fn default() -> Self {
        Self {
            links: LinkTraversal::DoNotFollow,
            max_depth: None,
            max_filesize: None,
            one_file_system: false,
        }
    }
}

#[derive(Clone, Copy)]
pub struct SearchRequest<'a> {
    pub indexes: &'a Indexes,
    pub filter: &'a SearchFilter,
    pub output: SearchOutput,
    pub separators: &'a SearchSeparators,
    pub collect_stats: bool,
}

impl SearchRequest<'_> {
    /// Resolves candidates from indexed search or filesystem walk.
    ///
    /// # Errors
    ///
    /// Returns an error if the filesystem walk fails or paths cannot be resolved.
    pub(crate) fn resolve_candidates(
        &self,
        spec: &QuerySpec<'_>,
    ) -> crate::Result<Vec<CandidateInfo>> {
        let output = self.output;
        if self.indexes.is_empty() {
            let abs_paths = walk::collect_abs_paths_for_scopes(self.filter)?;
            if abs_paths.is_empty() {
                return Ok(Vec::new());
            }
            return Ok(walk::prepare_walk_candidates(&abs_paths, self.filter));
        }

        let candidates = match output.candidate_set() {
            CandidateSet::AllIndexedFiles => {
                let all = self.indexes.resolve_all_files();
                indexed::prepare_candidates(all, self.filter)
            }
            CandidateSet::IndexedCandidates => {
                let resolved = self.indexes.resolve_candidates(spec);
                indexed::prepare_candidates(resolved, self.filter)
            }
        };
        Ok(candidates)
    }
}

use crate::search::filter::CandidateInfo;
use crate::search::output::SearchOutput;
use crate::search::output::style::SearchSeparators;

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

#[derive(Clone)]
pub struct SearchExecution<'a> {
    pub candidates: Vec<CandidateInfo>,
    pub output: SearchOutput,
    pub separators: &'a SearchSeparators,
    pub collect_stats: bool,
}

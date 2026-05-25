use crate::grep::execution::stats::SearchStats;
use crate::grep::filter::SearchFilter;
use crate::grep::output::SearchOutput;
use crate::grep::output::style::SearchSeparators;

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

pub struct SearchExecution<'a> {
    pub filter: &'a SearchFilter,
    pub output: SearchOutput,
    pub separators: &'a SearchSeparators,
    pub stats: Option<&'a mut SearchStats>,
}

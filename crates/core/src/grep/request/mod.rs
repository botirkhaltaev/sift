use crate::grep::filter::SearchFilter;
use crate::grep::output::SearchOutput;
use crate::grep::output::style::SearchSeparators;
use crate::index::Indexes;

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

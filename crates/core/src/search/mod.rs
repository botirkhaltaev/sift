//! Indexed search execution built on the public grep crates.

mod execute;
mod filter;
mod matcher;
mod types;

pub use execute::{parallel_candidate_threshold, walk_file_paths};
pub use filter::{
    CandidateInfo, GlobConfig, HiddenMode, IgnoreConfig, IgnoreSources, SearchFilter,
    SearchFilterConfig, VisibilityConfig,
};
pub use types::{
    CaseMode, ColorChoice, CompiledSearch, FilenameMode, Match, OutputEmission, PathDisplay,
    SearchLineStyle, SearchMatchFlags, SearchMode, SearchOptions, SearchOutput, SearchOutputFormat,
    SearchRecordStyle, SearchSeparators, SearchStats,
};

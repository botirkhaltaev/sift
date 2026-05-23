//! Indexed search execution built on the public grep crates.

mod compile;
mod error;
mod execute;
mod filter;
mod matcher;
mod types;

pub use compile::{PatternCompiler, compile_pattern, compile_search_pattern, pattern_branch};
pub use error::SearchError;
pub use execute::{parallel_candidate_threshold, walk_file_paths};
pub use filter::{
    CandidateInfo, GlobConfig, HiddenMode, IgnoreConfig, IgnoreSources, SearchFilter,
    SearchFilterConfig, TypeDef, VisibilityConfig,
};
pub use types::{
    BinaryMode, CaseMode, ColorChoice, CompiledSearch, FilenameMode, LineStyleFlags, Match,
    OutputEmission, PathDisplay, SearchLineStyle, SearchMatchFlags, SearchMode, SearchOptions,
    SearchOutput, SearchOutputFormat, SearchRecordStyle, SearchSeparators, SearchStats,
};

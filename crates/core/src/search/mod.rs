//! Indexed search execution built on the public grep crates.

mod execute;
mod filter;
mod matcher;
mod types;

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

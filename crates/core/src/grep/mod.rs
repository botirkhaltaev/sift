//! Indexed search execution built on the public grep crates.

mod compile;
mod error;
mod execute;
mod filter;
mod matcher;
mod types;

pub use compile::PatternCompiler;
pub use error::SearchError;
pub use execute::discover_files;
pub use filter::{
    CandidateInfo, GlobConfig, HiddenMode, IgnoreConfig, IgnoreSources, SearchFilter,
    SearchFilterConfig, TypeDef, VisibilityConfig,
};
pub use types::{
    BinaryMode, CandidateSet, CaseMode, ColorChoice, ColumnLimit, ColumnOverflow, CompiledSearch,
    FilenameMode, LineStyleFlags, LinkTraversal, Match, MatchEmissionMode, OutputEmission,
    PassthruMode, PathDisplay, RecordTerminator, SearchExecution, SearchLineStyle,
    SearchMatchFlags, SearchMode, SearchOptions, SearchOutput, SearchOutputFormat,
    SearchRecordStyle, SearchSeparators, SearchStats, WalkOptions, ZeroCountMode,
};

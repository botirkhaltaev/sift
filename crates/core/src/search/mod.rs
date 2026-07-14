pub mod event;
mod hit;
pub mod input;
mod matcher;
pub mod mode;
pub mod options;
pub mod query;
pub mod report;
mod searcher;
pub mod stats;
mod task;

pub use event::{
    BinaryEvent, ContextEvent, ContextKind, FileEvent, MatchEvent, SearchEvent, SearchSink,
};
pub use hit::Match;
pub use input::{
    CandidateTransform, Input, InputConversion, InputIdentity, Inputs, SearchFile, SearchInputs,
};
pub(crate) use matcher::PrefilterCompatibility;
pub use mode::{SearchMode, ZeroCounts};
pub use options::{
    BinaryMode, CaseMode, InputEncoding, RegexEngine, SearchBound, SearchFlags, SearchOptions,
};
pub use query::{SearchQuery, SearchQueryBuilder};
pub use report::{FileReport, Report};
pub(crate) use searcher::EventEmission;
pub use searcher::Searcher;
pub use stats::{Stats, StatsMode};
pub(crate) use task::SearchOutcome;

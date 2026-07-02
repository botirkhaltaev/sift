//! Grep pipeline orchestration.

mod collection;
mod compiled;
pub mod error;
pub mod input;
mod matched;
pub mod options;
pub mod policy;
mod query;
pub mod report;
mod session;
mod stats;

pub use crate::corpus::Candidate;
pub use crate::corpus::candidate::PathDisplay;
pub use crate::corpus::filter::{
    CandidateFilter, CandidateFilterConfig, GlobConfig, HiddenMode, IgnoreConfig, IgnoreSources,
    TypeFilterRule, VisibilityConfig,
};
pub use crate::corpus::order::{CandidateOrder, CandidateOrderDirection, CandidateOrderKey};
pub use crate::corpus::walk::FileWalk;
pub use crate::index::Indexes;
pub use compiled::{CompiledQuery, IndexUse};
pub use error::Error;
pub use input::{Input, Inputs};
pub use matched::Match;
pub use options::{
    BinaryMode, CaseMode, InputEncoding, MatchFlags, MatchOptions, RegexEngineRequest,
};
pub use policy::{
    CandidatePolicy, CandidatePolicyConfig, CandidateScope, CorpusState, IndexFallback,
};
pub use query::Query;
pub use report::Report;
pub use session::Session;
pub use stats::{Stats, StatsMode};

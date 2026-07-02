//! Grep pipeline orchestration.

pub mod error;
pub mod input;
pub mod options;
pub mod policy;
pub mod report;
mod search;
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
pub use error::Error;
pub use input::{Input, Inputs};
pub use options::{
    BinaryMode, CaseMode, InputEncoding, MatchFlags, MatchOptions, RegexEngineRequest,
};
pub use policy::{
    CandidatePolicy, CandidatePolicyConfig, CandidateScope, CorpusState, IndexFallback,
};
pub use report::Report;
pub use search::{CompiledQuery, Match, Query};
pub use session::Session;
pub use stats::{Stats, StatsMode};

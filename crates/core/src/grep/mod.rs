//! Grep pipeline orchestration.

pub(crate) mod engine;
pub mod error;
pub mod input;
pub mod options;
pub mod pattern;
pub mod policy;
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
pub use engine::{CompiledQuery, Indexability, IndexabilityReason, Matcher, SearcherConfig};
pub use error::Error;
pub use input::{Input, Inputs};
pub use options::{
    BinaryMode, CaseMode, InputEncoding, MatchFlags, MatchOptions, RegexEngineRequest,
};
pub use pattern::{Match, PatternCompiler, Query};
pub use policy::{
    CandidatePolicy, CandidatePolicyConfig, CandidateScope, CorpusState, IndexFallback,
};
pub use report::Report;
pub use session::Session;
pub use stats::{Stats, StatsMode};

pub mod planner;
pub mod request;
pub mod source;
pub mod spec;

pub use planner::CandidatePlanner;
pub(crate) use planner::IndexNarrowing;
pub use request::{
    CandidateExtent, CandidateRequest, CandidateScope, CandidateSelection, CorpusMode,
    IndexFallback,
};
pub use source::CandidateSource;
pub use spec::{CandidateFlags, CandidateSpec};

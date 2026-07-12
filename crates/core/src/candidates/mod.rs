pub mod plan;
pub mod planner;
pub mod query;
pub mod selection;
pub mod source;

#[path = "candidates.rs"]
mod collection;

pub use collection::Candidates;
pub use selection::{CandidateCoverage, CandidateSelection, IndexFallback};
pub use source::CandidateSource;

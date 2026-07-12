pub mod narrowing;
pub mod plan;
pub mod resolve;
pub mod resolved;
pub mod selection;
pub mod source;

pub use resolved::Candidates;
pub use selection::{CandidateCoverage, CandidateSelection, IndexFallback};
pub use source::CandidateSource;

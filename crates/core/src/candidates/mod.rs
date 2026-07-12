pub mod collection;
pub mod coverage;
pub mod query;
pub mod request;
pub mod resolve;
pub mod source;

pub use collection::Candidates;
pub use coverage::CandidateCoverage;
pub use request::{CandidateSelection, IndexFallback};
pub use source::CandidateSource;

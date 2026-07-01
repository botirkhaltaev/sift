//! Shared corpus foundation: candidates, filters, and filesystem walk.

pub mod candidate;
pub mod coverage;
pub mod filter;
pub mod order;
pub mod walk;

pub use candidate::Candidate;
pub use coverage::CandidateCoverage;
pub use order::CandidateOrder;

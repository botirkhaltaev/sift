//! Shared corpus foundation: candidates, filters, and filesystem walk.

pub mod candidate;
pub mod filter;
pub mod order;
pub mod walk;

pub use candidate::Candidate;
pub use order::CandidateOrder;

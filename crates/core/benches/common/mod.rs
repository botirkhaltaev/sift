//! Shared fixtures and helpers for sift-core benchmarks.
//!
//! Search/open/candidate benches build fixtures outside `b.iter`; build benches
//! materialize inside `b.iter`. Import only the submodules each bench needs.

pub mod criterion_config;
pub mod fixtures;

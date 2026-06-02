//! Index lifecycle (`sift index build`, `sift index update`) and background refresh.

pub mod command;
pub mod daemon;

pub use command::run_index_command;

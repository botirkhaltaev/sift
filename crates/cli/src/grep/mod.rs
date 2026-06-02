//! Search (`sift PATTERN`) and related flag handling.

pub mod engine;
pub mod filter;
pub mod ignore;
pub mod output;
pub mod paths;
pub mod pattern;
pub mod search;

pub use search::{run_files_mode, run_type_list};

pub mod cli;
pub mod grep;
pub mod index;
pub mod update;

// Stable paths for benchmarks and integration tests that import submodules directly.
pub use grep::run::{Grep, GrepConfig, GrepOutcome};
pub use grep::{Argv, engine, filter, ignore, output, paths, pattern};
pub use index::daemon;

use std::process::ExitCode;

use clap::Parser;
use cli::Cli;

#[must_use]
pub fn main_entry() -> ExitCode {
    let cli = Cli::parse();
    let argv_storage = Argv::from_env();
    let argv = Argv::new(&argv_storage);
    cli.dispatch(&argv)
}

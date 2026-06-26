pub mod cli;
pub mod config;
pub mod grep;
pub mod index;
pub mod update;

// Stable paths for benchmarks and integration tests that import submodules directly.
pub use grep::run::{Grep, GrepConfig, GrepMode, GrepOutcome};
pub use grep::{Argv, engine, filter, ignore, output, paths, pattern};

use std::process::ExitCode;

use clap::Parser;
use cli::Cli;
use config::ConfigArgs;

#[must_use]
pub fn main_entry() -> ExitCode {
    let raw_args = Argv::from_env();
    let config_args = match ConfigArgs::from_env(&raw_args) {
        Ok(args) => args,
        Err(err) => {
            eprintln!("sift: {err}");
            ConfigArgs::empty()
        }
    };
    let argv_storage = config_args.apply(&raw_args);
    let cli = Cli::parse_from(&argv_storage);
    let argv = Argv::new(&argv_storage);
    cli.dispatch(&argv)
}

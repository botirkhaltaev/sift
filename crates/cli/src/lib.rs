pub mod cli;
pub mod config;
pub mod grep;
pub mod index;
pub mod update;

// Stable paths for benchmarks and integration tests that import submodules directly.
pub use grep::{engine, filter, ignore, output, paths, pattern};
pub use index::daemon;

use std::process::ExitCode;

use clap::Parser;

use cli::Cli;
use config::CliConfig;
use grep::ignore::{MessageFlags, resolve_visibility_and_ignore};
use grep::{run_files_mode, run_type_list};

#[must_use]
pub fn main_entry() -> ExitCode {
    let cli = Cli::parse();

    if cli.filter_decl.type_list {
        run_type_list(&cli);
        return ExitCode::SUCCESS;
    }

    if matches!(cli.command, Some(cli::Commands::Update)) {
        return match update::run_binary_update() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("sift: {e}");
                ExitCode::from(2)
            }
        };
    }

    let cfg = match CliConfig::from_cli(&cli) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("sift: {e}");
            return ExitCode::from(2);
        }
    };

    if let Some(index) = &cfg.index {
        return index::run_index_command(&cfg, index);
    }

    let args: Vec<String> = std::env::args().collect();

    if cfg.files_mode {
        return match run_files_mode(&cli, &args) {
            Ok(true) => ExitCode::SUCCESS,
            Ok(false) => ExitCode::from(1),
            Err(e) => {
                eprintln!("sift: {e}");
                ExitCode::from(2)
            }
        };
    }

    // Re-spawn the daemon if it exited due to idle timeout.
    if let Err(e) = index::daemon::DaemonSupervisor::new_process().spawn(&cfg.daemon.spawn) {
        eprintln!("sift: warning: daemon not started: {e}");
    }

    let no_messages = resolve_visibility_and_ignore(&args)
        .msg_flags
        .contains(MessageFlags::NO_MESSAGES);

    match cli.run_search(&args) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            if let Some(ioe) = e.downcast_ref::<std::io::Error>()
                && ioe.kind() == std::io::ErrorKind::BrokenPipe
            {
                return ExitCode::SUCCESS;
            }
            if !no_messages {
                eprintln!("sift: {e}");
            }
            ExitCode::from(2)
        }
    }
}

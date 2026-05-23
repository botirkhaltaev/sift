pub mod cli;
pub mod engine;
pub mod filter;
pub mod ignore;
pub mod output;
pub mod paths;
pub mod pattern;
pub mod search;

use std::process::ExitCode;

use clap::Parser;
use sift_core::TrigramIndexBuilder;

use cli::{Cli, Commands};
use ignore::{MessageFlags, resolve_visibility_and_ignore};
use search::{run_files_mode, run_type_list};

#[must_use]
pub fn main_entry() -> ExitCode {
    let cli = Cli::parse();

    if cli.filter_decl.type_list {
        run_type_list(&cli);
        return ExitCode::SUCCESS;
    }

    if let Some(Commands::Build { path }) = &cli.command {
        return match TrigramIndexBuilder::new(path)
            .with_dir(&cli.paths.sift_dir)
            .with_follow_links(cli.paths.follow)
            .build()
        {
            Ok(_) => {
                eprintln!(
                    "indexed corpus {} → {}",
                    path.display(),
                    cli.paths.sift_dir.display()
                );
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("sift: {e}");
                ExitCode::from(2)
            }
        };
    }

    let args: Vec<String> = std::env::args().collect();

    if cli.filter_decl.files {
        return match run_files_mode(&cli, &args) {
            Ok(true) => ExitCode::SUCCESS,
            Ok(false) => ExitCode::from(1),
            Err(e) => {
                eprintln!("sift: {e}");
                ExitCode::from(2)
            }
        };
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

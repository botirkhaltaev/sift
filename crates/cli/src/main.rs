use std::process::ExitCode;

use clap::Parser;
use sift_core::IndexBuilder;

use cli::{Cli, Commands};
use ignore::{MessageFlags, resolve_visibility_and_ignore};
use search::{run_files_mode, run_type_list};

mod cli;
mod engine;
mod filter;
mod ignore;
mod output;
mod paths;
mod pattern;
mod search;

fn main() -> ExitCode {
    let cli = Cli::parse();

    if cli.filter_decl.type_list {
        run_type_list(&cli);
        return ExitCode::SUCCESS;
    }

    if let Some(Commands::Build { path }) = &cli.command {
        return match IndexBuilder::new(path)
            .with_dir(&cli.paths.sift_dir)
            .with_follow_links(cli.paths.follow)
            .build()
        {
            Ok(_) => {
                eprintln!(
                    "indexed corpus {} \u{2192} {}",
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

    if cli.filter_decl.files {
        return match run_files_mode(&cli) {
            Ok(true) => ExitCode::SUCCESS,
            Ok(false) => ExitCode::from(1),
            Err(e) => {
                eprintln!("sift: {e}");
                ExitCode::from(2)
            }
        };
    }

    let args: Vec<String> = std::env::args().collect();
    let no_messages = resolve_visibility_and_ignore(&args)
        .msg_flags
        .contains(MessageFlags::NO_MESSAGES);

    match cli.run_search() {
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

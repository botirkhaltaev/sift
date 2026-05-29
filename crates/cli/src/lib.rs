pub mod cli;
pub mod config;
pub mod daemon;
pub mod engine;
pub mod filter;
pub mod ignore;
pub mod output;
pub mod paths;
pub mod pattern;
pub mod search;

use std::process::ExitCode;

use clap::Parser;
use sift_core::{CorpusSpec, IgnoreConfig, IndexConfig, IndexStore, VisibilityConfig};

use cli::Cli;
use config::CliConfig;
use ignore::{MessageFlags, resolve_visibility_and_ignore};
use search::{run_files_mode, run_type_list};

#[must_use]
pub fn main_entry() -> ExitCode {
    let cli = Cli::parse();

    if cli.filter_decl.type_list {
        run_type_list(&cli);
        return ExitCode::SUCCESS;
    }

    let cfg = match CliConfig::from_cli(&cli) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("sift: {e}");
            return ExitCode::from(2);
        }
    };

    if let Some(build) = &cfg.build {
        let args: Vec<String> = std::env::args().collect();
        let ignore_res = resolve_visibility_and_ignore(&args);
        let mut store = match IndexStore::open_or_create(
            &build.sift_dir,
            &build.root,
            build.corpus_kind,
            build.follow_links,
            &build.indexes,
        ) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("sift: {e}");
                return ExitCode::from(2);
            }
        };
        let index_cfg = IndexConfig {
            corpus: CorpusSpec {
                root: &build.root,
                kind: build.corpus_kind,
                follow_links: build.follow_links,
                include_paths: &build.include_paths,
                exclude_paths: &build.exclude_paths,
            },
            visibility: VisibilityConfig {
                hidden: ignore_res.hidden_mode(),
                ignore: IgnoreConfig {
                    sources: ignore_res.sources,
                    require_git: false,
                    ..IgnoreConfig::default()
                },
            },
        };
        if let Err(e) = store.build(&build.indexes, &index_cfg) {
            eprintln!("sift: {e}");
            return ExitCode::from(2);
        }
        if let Err(e) = daemon::DaemonSupervisor::new_process().spawn(&cfg.daemon.spawn) {
            eprintln!("sift: warning: daemon not started: {e}");
        }
        eprintln!(
            "indexed corpus {} → {}",
            build.root.display(),
            build.sift_dir.display()
        );
        return ExitCode::SUCCESS;
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

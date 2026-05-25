pub mod cli;
pub mod daemon;
pub mod engine;
pub mod filter;
pub mod ignore;
pub mod output;
pub mod paths;
pub mod pattern;
pub mod search;

use std::path::Path;
use std::process::ExitCode;

use clap::Parser;
use sift_core::{CorpusKind, IndexBuildConfig, IndexStore, TrigramMaintenance};

use cli::{Cli, Commands};
use ignore::{MessageFlags, resolve_visibility_and_ignore};
use paths::excluded_search_paths;
use search::{run_files_mode, run_type_list};

#[must_use]
pub fn main_entry() -> ExitCode {
    let cli = Cli::parse();

    if cli.filter_decl.type_list {
        run_type_list(&cli);
        return ExitCode::SUCCESS;
    }

    if let Some(Commands::Build { path }) = &cli.command {
        let canonical = match path.canonicalize() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("sift: {e}");
                return ExitCode::from(2);
            }
        };
        let (root, include_paths, corpus_kind) = if canonical.is_file() {
            let parent = if let Some(p) = canonical.parent() {
                p.to_path_buf()
            } else {
                eprintln!("sift: corpus root must have a parent directory");
                return ExitCode::from(2);
            };
            let filename = Path::new(canonical.file_name().unwrap_or_default()).to_path_buf();
            (parent, vec![filename], CorpusKind::SingleFile)
        } else {
            (canonical, Vec::new(), CorpusKind::Directory)
        };
        let sift_dir = &cli.paths.sift_dir;
        let exclude_paths = excluded_search_paths(&root, sift_dir);
        let mut store =
            match IndexStore::open_or_create(sift_dir, &root, corpus_kind, cli.paths.follow) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("sift: {e}");
                    return ExitCode::from(2);
                }
            };
        let config = IndexBuildConfig {
            root: &root,
            follow_links: cli.paths.follow,
            exclude_paths: &exclude_paths,
            include_paths: &include_paths,
            corpus_kind,
        };
        if let Err(e) = store.build::<TrigramMaintenance>(&config) {
            eprintln!("sift: {e}");
            return ExitCode::from(2);
        }
        spawn_daemon(sift_dir, None);
        eprintln!(
            "indexed corpus {} → {}",
            path.display(),
            cli.paths.sift_dir.display()
        );
        return ExitCode::SUCCESS;
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

/// Best-effort spawn `sift-daemon` in the background.
fn spawn_daemon(sift_dir: &Path, init_root: Option<&Path>) {
    let exe = match std::env::current_exe() {
        Ok(p) => p.with_file_name("sift-daemon"),
        Err(_) => return,
    };

    let mut cmd = std::process::Command::new(&exe);
    cmd.arg("--sift-dir").arg(sift_dir);
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::null());
    cmd.stdin(std::process::Stdio::null());

    if let Some(root) = init_root {
        cmd.arg("--init-root").arg(root);
    }

    let _ = cmd.spawn();
}

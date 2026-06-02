//! `sift index build` and `sift index update` dispatch.

use std::process::ExitCode;

use sift_core::{CorpusSpec, IgnoreConfig, IndexConfig, IndexStore, VisibilityConfig};

use crate::config::{CliConfig, IndexCommandConfig, IndexOperation};
use crate::grep::ignore::{IgnoreResolution, resolve_visibility_and_ignore};

use super::daemon::DaemonSupervisor;

/// Run `sift index build` or `sift index update`.
#[must_use]
pub fn run_index_command(cfg: &CliConfig, index: &IndexCommandConfig) -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let ignore_res = resolve_visibility_and_ignore(&args);

    let mut store = match IndexStore::open_or_create(
        &index.sift_dir,
        &index.root,
        index.corpus_kind,
        index.follow_links,
        &index.indexes,
    ) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("sift: {e}");
            return ExitCode::from(2);
        }
    };

    let has_snapshot = store.current_id().is_some();
    let result = match index.operation {
        IndexOperation::Build if has_snapshot => {
            eprintln!(
                "sift: index already exists at {}; run `sift index update` to refresh it",
                index.sift_dir.display()
            );
            return ExitCode::from(2);
        }
        IndexOperation::Build => run_build(&mut store, index, &ignore_res),
        IndexOperation::Update if !has_snapshot => {
            eprintln!(
                "sift: no index at {}; run `sift index build` first",
                index.sift_dir.display()
            );
            return ExitCode::from(2);
        }
        IndexOperation::Update => run_update(&mut store, index, &ignore_res),
    };

    if let Err(e) = result {
        eprintln!("sift: {e}");
        return ExitCode::from(2);
    }

    if let Err(e) = DaemonSupervisor::new_process().spawn(&cfg.daemon.spawn) {
        eprintln!("sift: warning: daemon not started: {e}");
    }

    let verb = match index.operation {
        IndexOperation::Build => "indexed",
        IndexOperation::Update => "updated index for",
    };
    eprintln!(
        "{verb} corpus {} → {}",
        index.root.display(),
        index.sift_dir.display()
    );
    ExitCode::SUCCESS
}

fn run_build(
    store: &mut IndexStore,
    index: &IndexCommandConfig,
    ignore_res: &IgnoreResolution,
) -> sift_core::Result<()> {
    let index_cfg = index_config(index, ignore_res);
    store.build(&index.indexes, &index_cfg).map(|_| ())
}

fn run_update(
    store: &mut IndexStore,
    index: &IndexCommandConfig,
    ignore_res: &IgnoreResolution,
) -> sift_core::Result<()> {
    let index_cfg = index_config(index, ignore_res);
    store.update(&index.indexes, &index_cfg).map(|_| ())
}

fn index_config<'a>(
    index: &'a IndexCommandConfig,
    ignore_res: &IgnoreResolution,
) -> IndexConfig<'a> {
    IndexConfig {
        corpus: CorpusSpec {
            root: &index.root,
            kind: index.corpus_kind,
            follow_links: index.follow_links,
            include_paths: &index.include_paths,
            exclude_paths: &index.exclude_paths,
        },
        visibility: VisibilityConfig {
            hidden: ignore_res.hidden_mode(),
            ignore: IgnoreConfig {
                sources: ignore_res.sources,
                require_git: false,
                ..IgnoreConfig::default()
            },
        },
    }
}

//! Index lifecycle (`sift index build`, `sift index update`) and background refresh.

use std::path::PathBuf;
use std::process::ExitCode;

use sift_core::{
    CorpusKind, CorpusSpec, IgnoreConfig, IndexConfig, IndexKind, IndexStore, VisibilityConfig,
};

use crate::grep::Argv;
use crate::grep::ignore::IgnoreResolution;
use crate::grep::paths::CorpusScope;

pub mod daemon;

use daemon::DaemonSupervisor;

pub use daemon::DaemonSpawnConfig;

/// Which index subcommand was requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexOperation {
    Build,
    Update,
}

/// Parameters for resolving an index build or update.
pub struct IndexRequest {
    pub operation: IndexOperation,
    pub path: PathBuf,
    pub indexes: Option<Vec<IndexKind>>,
    pub sift_dir: PathBuf,
    pub follow_links: bool,
}

/// Resolved `sift index build` / `sift index update` request.
pub struct Index {
    pub operation: IndexOperation,
    pub root: PathBuf,
    pub include_paths: Vec<PathBuf>,
    pub corpus_kind: CorpusKind,
    pub indexes: Vec<IndexKind>,
    pub follow_links: bool,
    pub exclude_paths: Vec<PathBuf>,
    pub sift_dir: PathBuf,
}

impl Index {
    /// Resolve an index request from parsed CLI args.
    ///
    /// # Errors
    ///
    /// Returns an error if the index path cannot be canonicalised.
    pub fn resolve(req: IndexRequest) -> Result<Self, std::io::Error> {
        let canonical = req.path.canonicalize()?;
        let (root, include_paths, corpus_kind) = if canonical.is_file() {
            let parent = canonical.parent().unwrap_or(&canonical).to_path_buf();
            let filename = PathBuf::from(canonical.file_name().unwrap_or_default());
            (parent, vec![filename], CorpusKind::SingleFile)
        } else {
            (canonical, Vec::new(), CorpusKind::Directory)
        };
        let indexes: Vec<IndexKind> = req.indexes.as_deref().unwrap_or(IndexKind::ALL).to_vec();
        let exclude_paths = CorpusScope::excluded_paths(&root, &req.sift_dir);
        Ok(Self {
            operation: req.operation,
            root,
            include_paths,
            corpus_kind,
            indexes,
            follow_links: req.follow_links,
            exclude_paths,
            sift_dir: req.sift_dir,
        })
    }

    /// Run `sift index build` or `sift index update`.
    #[must_use]
    pub fn run(&self, spawn: &DaemonSpawnConfig, argv: &Argv<'_>) -> ExitCode {
        let ignore_res = IgnoreResolution::resolve(argv);

        let mut store = match IndexStore::open_or_create(
            &self.sift_dir,
            &self.root,
            self.corpus_kind,
            self.follow_links,
            &self.indexes,
        ) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("sift: {e}");
                return ExitCode::from(2);
            }
        };

        let has_snapshot = store.current_id().is_some();
        let result = match self.operation {
            IndexOperation::Build if has_snapshot => {
                eprintln!(
                    "sift: index already exists at {}; run `sift index update` to refresh it",
                    self.sift_dir.display()
                );
                return ExitCode::from(2);
            }
            IndexOperation::Build => self.build(&mut store, ignore_res),
            IndexOperation::Update if !has_snapshot => {
                eprintln!(
                    "sift: no index at {}; run `sift index build` first",
                    self.sift_dir.display()
                );
                return ExitCode::from(2);
            }
            IndexOperation::Update => self.update(&mut store, ignore_res),
        };

        if let Err(e) = result {
            eprintln!("sift: {e}");
            return ExitCode::from(2);
        }

        if let Err(e) = DaemonSupervisor::new_process().spawn(spawn) {
            eprintln!("sift: warning: daemon not started: {e}");
        }

        let verb = match self.operation {
            IndexOperation::Build => "indexed",
            IndexOperation::Update => "updated index for",
        };
        eprintln!(
            "{verb} corpus {} → {}",
            self.root.display(),
            self.sift_dir.display()
        );
        ExitCode::SUCCESS
    }

    fn build(&self, store: &mut IndexStore, ignore_res: IgnoreResolution) -> sift_core::Result<()> {
        store
            .build(&self.indexes, &core_config(self, ignore_res))
            .map(|_| ())
    }

    fn update(
        &self,
        store: &mut IndexStore,
        ignore_res: IgnoreResolution,
    ) -> sift_core::Result<()> {
        store
            .update(&self.indexes, &core_config(self, ignore_res))
            .map(|_| ())
    }
}

fn core_config(index: &Index, ignore_res: IgnoreResolution) -> IndexConfig<'_> {
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

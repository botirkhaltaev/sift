use std::path::PathBuf;

use sift_core::grep::{CandidatePolicyConfig, CorpusState, FileWalk, IndexFallback, Session};
use sift_core::{CorpusKind, IndexCoverage, Indexes};

use crate::format::{PrintExtras, PrintMode, SearchPrinter};

use crate::index::daemon::Daemon;

use super::argv::Argv;
use super::filter::FilterConfig;
use super::input::{ContentTransformConfig, InputSources};
use super::output::{FilenameContext, OutputArgv, OutputDecl};
use super::paths::CorpusScope;
use super::pattern::{PatternArgv, PatternDecl, ResolvedPatterns};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    Search,
    ListFiles,
}

/// Resolved configuration for a search invocation.
#[derive(Clone)]
pub struct RunConfig {
    pub pattern: PatternDecl,
    pub filter: FilterConfig,
    pub output: OutputDecl,
    pub sift_dir: PathBuf,
    pub search_paths: Vec<PathBuf>,
    pub threads: Option<usize>,
    pub mode: RunMode,
    pub content: ContentTransformConfig,
    pub candidate_order: sift_core::grep::CandidateOrder,
}

impl RunConfig {
    /// Build a resolved run configuration from parsed CLI state.
    ///
    /// # Errors
    ///
    /// Returns an error if sort/order flags are invalid.
    pub fn from_cli(cli: &crate::Cli, argv: &Argv<'_>) -> Result<Self, anyhow::Error> {
        cli.run_config(argv)
    }
}

/// CLI search runner.
pub struct Run {
    config: RunConfig,
}

/// Result of a search run; variant reflects `--files` vs pattern search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunResult {
    Files { found: bool },
    Search { matched: bool },
}

struct PreparedSession {
    indexes: Indexes,
    scope: CorpusScope,
    search_filter: sift_core::grep::CandidateFilter,
    store_meta: Option<sift_core::StoreMeta>,
}

impl RunResult {
    #[must_use]
    pub const fn succeeded(self) -> bool {
        match self {
            Self::Files { found } => found,
            Self::Search { matched } => matched,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SnapshotTrust {
    Unvalidated,
    Validated,
    Stale,
}

impl Run {
    #[must_use]
    pub const fn new(config: RunConfig) -> Self {
        Self { config }
    }

    /// # Errors
    ///
    /// Returns an error if I/O operations fail, paths are invalid, or filter config building fails.
    pub fn execute(&self, argv: &Argv<'_>, daemon: Option<&Daemon>) -> anyhow::Result<RunResult> {
        match self.config.mode {
            RunMode::ListFiles => self.run_files(argv).map(|found| RunResult::Files { found }),
            RunMode::Search => self
                .run_search(argv, daemon)
                .map(|matched| RunResult::Search { matched }),
        }
    }

    fn configure_threads(&self) {
        if let Some(threads) = self.config.threads {
            rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build_global()
                .ok();
        }
    }

    fn prepare_session(
        &self,
        argv: &Argv<'_>,
        search_paths: &[PathBuf],
    ) -> anyhow::Result<PreparedSession> {
        self.configure_threads();
        let cwd = std::env::current_dir()?;
        let indexes = Indexes::open(&self.config.sift_dir)?;
        let store_meta = sift_core::StoreMeta::read(&self.config.sift_dir).ok();
        let scope = CorpusScope::resolve(
            &indexes,
            store_meta.as_ref(),
            &cwd,
            search_paths,
            &self.config.sift_dir,
        )?;
        let filter_config = self.config.filter.candidate_config(
            argv,
            scope.prefixes.clone(),
            scope.exclude_paths.clone(),
        )?;
        let search_filter =
            sift_core::grep::CandidateFilter::new(&filter_config, &scope.filter_root)?;
        Ok(PreparedSession {
            indexes,
            scope,
            search_filter,
            store_meta,
        })
    }

    fn run_files(&self, argv: &Argv<'_>) -> anyhow::Result<bool> {
        let output_argv = OutputArgv::resolve(argv);
        let session = self.prepare_session(argv, &self.config.search_paths)?;

        let mut candidates = FileWalk::from_filter(&session.search_filter).collect()?;
        candidates.retain(|candidate| session.search_filter.matches_path(candidate.rel_path()));
        self.config.candidate_order.order(&mut candidates)?;
        let all_paths: Vec<_> = candidates
            .into_iter()
            .map(|candidate| candidate.rel_path().to_path_buf())
            .collect();
        let sep = if output_argv.path.nul_terminated {
            '\0'
        } else {
            '\n'
        };
        let mut any = false;
        for p in &all_paths {
            any = true;
            let display = session.scope.filter_root.join(p);
            print!("{}{sep}", display.display());
        }
        Ok(any)
    }

    fn run_search(&self, argv: &Argv<'_>, daemon: Option<&Daemon>) -> anyhow::Result<bool> {
        let patterns = ResolvedPatterns::resolve(&self.config.pattern)?;
        let sources = InputSources::from_paths(&self.config.search_paths);
        let pattern_argv = PatternArgv::resolve(argv);
        let output_argv = OutputArgv::resolve(argv);

        let effective_mode = if pattern_argv.only_matching {
            PrintMode::OnlyMatching
        } else {
            pattern_argv.mode
        };

        let line_number_override =
            if self.config.output.column.pretty || self.config.output.column.vimgrep {
                Some(true)
            } else {
                output_argv.line_number
            };

        let session = self.prepare_session(argv, &sources.paths)?;
        let sources = sources.resolve(patterns.input, session.indexes.is_empty())?;
        let transform = self.config.content.transform()?;

        let query = self
            .config
            .pattern
            .query(patterns.patterns, &pattern_argv)
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let filename_ctx = Self::filename_context(effective_mode, &sources, &session);
        let print_spec = self
            .config
            .output
            .print_spec(
                &output_argv,
                effective_mode,
                pattern_argv.quiet,
                line_number_override,
                filename_ctx,
            )
            .map_err(|e| anyhow::anyhow!(e))?;
        let separators = self.config.output.separators();
        let snapshot = Self::snapshot_validation(&session, daemon);
        let walk_fallback = matches!(snapshot, SnapshotTrust::Stale);

        let compiled = query.compile().map_err(|e| anyhow::anyhow!("{e}"))?;
        let corpus = if session.indexes.is_empty() {
            CorpusState::Unindexed
        } else if transform.is_some() {
            CorpusState::TransformedBytes
        } else {
            CorpusState::Indexed
        };
        let fallback = if walk_fallback {
            IndexFallback::WalkOnStaleSnapshot
        } else {
            IndexFallback::IndexHitsOnly
        };
        let policy = CandidatePolicyConfig {
            output_scope: print_spec.candidate_scope(),
            corpus,
            fallback,
            order: self.config.candidate_order,
        }
        .policy(compiled);

        let session_data = Session::new(
            &session.indexes,
            &session.search_filter,
            session.store_meta.as_ref(),
        );

        let candidates = if sources.resolve_candidates() {
            query
                .candidates(&session_data, policy)
                .map_err(|e| anyhow::anyhow!("{e}"))?
        } else {
            Vec::new()
        };

        let inputs = sources
            .build_inputs(&candidates, transform.as_ref())
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let print_stats = OutputDecl::print_stats(&output_argv, effective_mode);
        let extras = PrintExtras::hits().with_stats(print_stats);
        let report = SearchPrinter::new(&query, compiled, print_spec, &separators, extras)
            .print(&inputs)
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        if let Some(s) = report.stats.as_ref() {
            OutputDecl::write_stats(s);
        }
        let matched = report.matched;
        if let Some(daemon) = daemon
            && session
                .store_meta
                .as_ref()
                .is_some_and(|meta| meta.coverage == IndexCoverage::Lazy)
        {
            let paths = session.indexes.unindexed_hits(report.hit_paths);
            if !paths.is_empty()
                && let Err(e) = daemon.index(paths)
            {
                eprintln!("sift: warning: index request failed: {e}");
            }
        }
        Ok(matched)
    }

    fn filename_context(
        effective_mode: PrintMode,
        sources: &InputSources,
        session: &PreparedSession,
    ) -> FilenameContext {
        if OutputDecl::is_path_mode(effective_mode) {
            FilenameContext::PathMode
        } else if !sources.stdin_bytes.is_empty() && sources.paths.is_empty() {
            FilenameContext::SingleFileCorpus
        } else {
            match session.indexes.corpus_kind() {
                Some(CorpusKind::SingleFile) => FilenameContext::SingleFileCorpus,
                _ => FilenameContext::DirectoryCorpus,
            }
        }
    }

    fn snapshot_validation(session: &PreparedSession, daemon: Option<&Daemon>) -> SnapshotTrust {
        daemon
            .and_then(|daemon| {
                session
                    .indexes
                    .snapshot_id()
                    .map(|id| daemon.validate_snapshot(id))
            })
            .map_or(SnapshotTrust::Unvalidated, |validation| match validation {
                Ok(true) => SnapshotTrust::Validated,
                Ok(false) => SnapshotTrust::Stale,
                Err(_) => SnapshotTrust::Unvalidated,
            })
    }
}

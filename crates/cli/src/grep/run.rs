use std::borrow::Cow;
use std::path::PathBuf;

use sift_core::candidates::{CandidateSelection, CandidateSource, CorpusMode, IndexFallback};
use sift_core::grep::{ByteInput, FileWalk, Grep, InputRequest};
use sift_core::search::{SearchMode, ZeroCounts};
use sift_core::{CorpusKind, GrepRequest, IndexCoverage, Indexes};

use crate::format::output::mode::ZeroCountMode;
use crate::format::{PathDisplay, PrintExtras, PrintMode, SearchPrinter};

use crate::index::daemon::Daemon;

use super::argv::Argv;
use super::filter::FilterConfig;
use super::input::{ContentTransform, ContentTransformConfig, InputSources};
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

struct PreparedCandidateSource {
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
    ) -> anyhow::Result<PreparedCandidateSource> {
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
        Ok(PreparedCandidateSource {
            indexes,
            scope,
            search_filter,
            store_meta,
        })
    }

    fn run_files(&self, argv: &Argv<'_>) -> anyhow::Result<bool> {
        let output_argv = OutputArgv::resolve(argv);
        let session = self.prepare_session(argv, &self.config.search_paths)?;

        let mut candidates = FileWalk::from_filter(&session.search_filter).candidates()?;
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

        let line_number_override = self.line_number_override(&output_argv);

        let session = self.prepare_session(argv, &sources.paths)?;
        let sources = sources.resolve(patterns.input, session.indexes.is_empty())?;
        let transform = self.config.content.transform()?;

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
        let selection = self.candidate_selection(
            &session,
            transform.as_ref(),
            snapshot,
            sources.resolve_candidates(),
        );
        let candidate_source = CandidateSource {
            indexes: &session.indexes,
            filter: &session.search_filter,
            store_meta: session.store_meta.as_ref(),
        };

        let query = self
            .config
            .pattern
            .search_query(patterns.patterns, &pattern_argv)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let mut input_request = Self::input_request(
            &sources,
            &Self::explicit_files(&session),
            print_spec.lines.path_display,
        );
        if let Some(transform) = transform.as_ref() {
            input_request = input_request.with_candidate_transform(transform);
        }
        let print_stats = OutputDecl::print_stats(&output_argv, effective_mode);
        let extras = PrintExtras::hits().with_stats(print_stats);
        let mode = Self::search_mode(effective_mode, print_spec.include_zero);
        let stats = if print_stats {
            sift_core::StatsMode::On
        } else {
            sift_core::StatsMode::Off
        };
        let grep = Grep::new(candidate_source);
        let request = GrepRequest {
            query,
            candidates: selection,
            inputs: input_request,
            mode,
            stats,
        };
        let report = SearchPrinter::print_grep(&grep, request, print_spec, &separators, extras)
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        if let Some(s) = report.stats.as_ref() {
            OutputDecl::write_stats(s);
        }
        let selected = report.selected;
        Self::queue_lazy_hits(daemon, &session, report.hit_paths);
        Ok(selected)
    }

    const fn line_number_override(&self, output_argv: &OutputArgv) -> Option<bool> {
        if self.config.output.column.pretty || self.config.output.column.vimgrep {
            Some(true)
        } else {
            output_argv.line_number
        }
    }

    const fn candidate_selection(
        &self,
        session: &PreparedCandidateSource,
        transform: Option<&ContentTransform>,
        snapshot: SnapshotTrust,
        resolve_candidates: bool,
    ) -> CandidateSelection {
        if !resolve_candidates {
            return CandidateSelection::None;
        }
        CandidateSelection::Corpus {
            corpus: if session.indexes.is_empty() {
                CorpusMode::Walk
            } else if transform.is_some() {
                CorpusMode::Transformed
            } else {
                CorpusMode::Indexed
            },
            fallback: if matches!(snapshot, SnapshotTrust::Stale) {
                IndexFallback::WalkOnStaleSnapshot
            } else {
                IndexFallback::IndexHitsOnly
            },
            order: self.config.candidate_order,
        }
    }

    const fn search_mode(mode: PrintMode, zeros: ZeroCountMode) -> SearchMode {
        let zeros = match zeros {
            ZeroCountMode::Omit => ZeroCounts::Omit,
            ZeroCountMode::Include => ZeroCounts::Include,
        };
        match mode {
            PrintMode::Standard => SearchMode::Lines,
            PrintMode::OnlyMatching => SearchMode::Matches,
            PrintMode::Count => SearchMode::CountLines { zeros },
            PrintMode::CountMatches => SearchMode::CountMatches { zeros },
            PrintMode::FilesWithMatches => SearchMode::FilesWithMatches,
            PrintMode::FilesWithoutMatch => SearchMode::FilesWithoutMatch,
        }
    }

    fn queue_lazy_hits(
        daemon: Option<&Daemon>,
        session: &PreparedCandidateSource,
        hit_paths: Vec<PathBuf>,
    ) {
        if let Some(daemon) = daemon
            && session
                .store_meta
                .as_ref()
                .is_some_and(|meta| meta.coverage == IndexCoverage::Lazy)
        {
            let paths = session.indexes.unindexed_hits(hit_paths);
            if !paths.is_empty()
                && let Err(e) = daemon.index(paths)
            {
                eprintln!("sift: warning: index request failed: {e}");
            }
        }
    }

    fn filename_context(
        effective_mode: PrintMode,
        sources: &InputSources,
        session: &PreparedCandidateSource,
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

    fn snapshot_validation(
        session: &PreparedCandidateSource,
        daemon: Option<&Daemon>,
    ) -> SnapshotTrust {
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

    fn explicit_files(session: &PreparedCandidateSource) -> Vec<PathBuf> {
        session
            .scope
            .prefixes
            .iter()
            .filter(|prefix| session.scope.filter_root.join(prefix).is_file())
            .cloned()
            .collect()
    }

    fn input_request<'a>(
        sources: &'a InputSources,
        explicit_files: &[PathBuf],
        path_display: PathDisplay,
    ) -> InputRequest<'a> {
        let mut request = InputRequest::from_candidates().with_path_display(path_display);
        for path in explicit_files {
            request = request.with_explicit_path(path.clone());
        }
        for bytes in &sources.stdin_bytes {
            request = request.with_stream(ByteInput {
                path: Cow::Borrowed("<stdin>"),
                bytes: Cow::Borrowed(bytes.as_slice()),
                explicit: true,
            });
        }
        request
    }
}

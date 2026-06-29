use std::io::Read;
use std::path::PathBuf;

use sift_core::grep::{CandidateOrder, GrepRequest, GrepSource};
use sift_core::search::{CandidateFilter, SearchMode, StreamInput};
use sift_core::{
    Candidate, CandidateSource, CorpusKind, IndexCoverage, Indexes, SearchQuery, SnapshotValidation,
};

use crate::index::daemon::Daemon;

use super::argv::Argv;
use super::filter::{FilterConfig, SearchFilterCtx};
use super::output::{FilenameContext, OutputArgv, OutputConfig, SearchOutputCtx};
use super::paths::CorpusScope;
use super::pattern::{PatternArgv, PatternConfig, PatternInputUse, ResolvedPatterns};

const STDIN_DISPLAY_PATH: &str = "<stdin>";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrepMode {
    Search,
    ListFiles,
}

/// Resolved configuration for a grep invocation.
#[derive(Clone)]
pub struct GrepConfig {
    pub pattern: PatternConfig,
    pub filter: FilterConfig,
    pub output: OutputConfig,
    pub sift_dir: PathBuf,
    pub search_paths: Vec<PathBuf>,
    pub threads: Option<usize>,
    pub mode: GrepMode,
    pub candidate_order: CandidateOrder,
}

/// Grep-mode search and file listing.
pub struct Grep {
    config: GrepConfig,
}

/// Result of a grep run; variant reflects `--files` vs pattern search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrepOutcome {
    Files { found: bool },
    Search { matched: bool },
}

struct GrepSession {
    indexes: Indexes,
    scope: CorpusScope,
    search_filter: CandidateFilter,
    store_meta: Option<sift_core::StoreMeta>,
}

struct SearchSources {
    paths: Vec<PathBuf>,
    streams: Vec<Vec<u8>>,
}

struct SearchSourceDecl {
    paths: Vec<PathBuf>,
    stream: StreamRequest,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StreamRequest {
    Explicit,
    Unspecified,
}

impl SearchSources {
    const fn searches_corpus(&self) -> bool {
        !self.paths.is_empty() || self.streams.is_empty()
    }

    fn stream_inputs(&self) -> Vec<StreamInput<'_>> {
        self.streams
            .iter()
            .map(|bytes| StreamInput {
                display_path: STDIN_DISPLAY_PATH,
                bytes,
            })
            .collect()
    }
}

impl SearchSourceDecl {
    fn from_paths(search_paths: &[PathBuf]) -> Self {
        let mut paths = Vec::with_capacity(search_paths.len());
        let mut stream = StreamRequest::Unspecified;
        for path in search_paths {
            if path == std::path::Path::new("-") {
                stream = StreamRequest::Explicit;
            } else {
                paths.push(path.clone());
            }
        }

        Self { paths, stream }
    }

    fn resolve(
        self,
        pattern_input: PatternInputUse,
        session: &GrepSession,
    ) -> anyhow::Result<SearchSources> {
        let stream_available = pattern_input == PatternInputUse::None;
        let implicit_stream = stream_available
            && self.paths.is_empty()
            && self.stream == StreamRequest::Unspecified
            && session.indexes.is_empty()
            && stdin_is_pipe();
        let streams = if self.stream == StreamRequest::Explicit {
            let mut bytes = Vec::new();
            std::io::stdin().read_to_end(&mut bytes)?;
            vec![bytes]
        } else if implicit_stream {
            let mut bytes = Vec::new();
            std::io::stdin().read_to_end(&mut bytes)?;
            if bytes.is_empty() {
                Vec::new()
            } else {
                vec![bytes]
            }
        } else {
            Vec::new()
        };

        Ok(SearchSources {
            paths: self.paths,
            streams,
        })
    }
}

impl GrepOutcome {
    #[must_use]
    pub const fn succeeded(self) -> bool {
        match self {
            Self::Files { found } => found,
            Self::Search { matched } => matched,
        }
    }
}

impl Grep {
    #[must_use]
    pub const fn new(config: GrepConfig) -> Self {
        Self { config }
    }

    /// # Errors
    ///
    /// Returns an error if I/O operations fail, paths are invalid, or filter config building fails.
    pub fn run(&self, argv: &Argv<'_>, daemon: Option<&Daemon>) -> anyhow::Result<GrepOutcome> {
        match self.config.mode {
            GrepMode::ListFiles => self
                .run_files(argv)
                .map(|found| GrepOutcome::Files { found }),
            GrepMode::Search => self
                .run_search(argv, daemon)
                .map(|matched| GrepOutcome::Search { matched }),
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
    ) -> anyhow::Result<GrepSession> {
        self.configure_threads();
        let filter = SearchFilterCtx::resolve(argv);
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
            filter,
            scope.prefixes.clone(),
            scope.exclude_paths.clone(),
        )?;
        let search_filter = CandidateFilter::new(&filter_config, &scope.filter_root)?;
        Ok(GrepSession {
            indexes,
            scope,
            search_filter,
            store_meta,
        })
    }

    fn run_files(&self, argv: &Argv<'_>) -> anyhow::Result<bool> {
        let output_argv = OutputArgv::resolve(argv);
        let session = self.prepare_session(argv, &self.config.search_paths)?;

        let walk_opts = sift_core::search::WalkOptions {
            links: if session.search_filter.follow_links() {
                sift_core::search::LinkTraversal::Follow
            } else {
                sift_core::search::LinkTraversal::DoNotFollow
            },
            max_depth: session.search_filter.max_depth(),
            max_filesize: session.search_filter.max_filesize(),
            one_file_system: session.search_filter.one_file_system(),
        };

        let effective_scopes = if session.scope.prefixes.is_empty() {
            vec![PathBuf::from("")]
        } else {
            session.scope.prefixes.clone()
        };

        let mut all_paths: Vec<_> = Vec::new();
        for prefix in &effective_scopes {
            let scope_path = if prefix.as_os_str().is_empty() {
                session.scope.filter_root.clone()
            } else {
                session.scope.filter_root.join(prefix)
            };
            if !scope_path.exists() {
                continue;
            }
            let scope_path = scope_path.canonicalize().unwrap_or(scope_path);
            if scope_path.is_file() {
                let rel = scope_path
                    .strip_prefix(&session.scope.filter_root)
                    .unwrap_or(&scope_path)
                    .to_path_buf();
                if session.search_filter.matches_path(&rel) {
                    all_paths.push(rel);
                }
            } else if scope_path.is_dir() {
                let discovered = walk_opts.discover_files(&scope_path)?;
                for rel_in_scope in discovered {
                    let full_rel = if prefix.as_os_str().is_empty() {
                        rel_in_scope
                    } else {
                        prefix.join(rel_in_scope)
                    };
                    if session.search_filter.matches_path(&full_rel) {
                        all_paths.push(full_rel);
                    }
                }
            }
        }
        all_paths.sort();
        all_paths.dedup();
        if self.config.candidate_order.is_sorted() {
            let mut candidates = all_paths
                .into_iter()
                .map(|rel| Candidate::new(rel.clone(), session.scope.filter_root.join(&rel)))
                .collect::<Vec<_>>();
            self.config
                .candidate_order
                .order_candidates(&mut candidates)?;
            all_paths = candidates
                .into_iter()
                .map(|candidate| candidate.rel_path().to_path_buf())
                .collect();
        }
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
        let source_decl = SearchSourceDecl::from_paths(&self.config.search_paths);
        let pattern_argv = PatternArgv::resolve(argv);
        let output_argv = OutputArgv::resolve(argv);

        let effective_mode = if pattern_argv.only_matching {
            SearchMode::OnlyMatching
        } else {
            pattern_argv.mode
        };

        let line_number_override =
            if self.config.output.column.pretty || self.config.output.column.vimgrep {
                Some(true)
            } else {
                output_argv.line_number
            };

        if output_argv.mode.json {
            match effective_mode {
                SearchMode::Count
                | SearchMode::CountMatches
                | SearchMode::FilesWithMatches
                | SearchMode::FilesWithoutMatch => {
                    anyhow::bail!(
                        "sift: --json cannot be used with --count, --count-matches, --files-with-matches, or --files-without-match"
                    );
                }
                SearchMode::Standard | SearchMode::OnlyMatching => {}
            }
        }

        let session = self.prepare_session(argv, &source_decl.paths)?;
        let sources = source_decl.resolve(patterns.input, &session)?;

        let opts = self
            .config
            .pattern
            .search_options(&pattern_argv, pattern_argv.only_matching);
        let query =
            SearchQuery::new(&patterns.patterns, opts).map_err(|e| anyhow::anyhow!("{e}"))?;

        let (out, _) = SearchOutputCtx::resolve(
            &self.config.output,
            argv,
            effective_mode,
            pattern_argv.quiet,
            line_number_override,
        );

        let filename_ctx = Self::filename_context(&out, &sources, &session);
        let output = out.to_core_output(&self.config.output, filename_ctx);
        let snapshot = Self::snapshot_validation(&session, daemon);

        let grep_request = GrepRequest {
            indexes: &session.indexes,
            filter: &session.search_filter,
            output,
            separators: &out.separators,
            collect: sift_core::search::SearchCollection::hits().with_stats(out.print_stats),
            candidate_source: CandidateSource {
                store_meta: session.store_meta.as_ref(),
                snapshot,
            },
            candidate_order: self.config.candidate_order,
        };
        let grep_run = Self::search_sources(&grep_request, &query, &sources)?;
        if let Some(s) = &grep_run.outcome.stats {
            SearchOutputCtx::write_stats(s);
        }
        if let Some(daemon) = daemon
            && session
                .store_meta
                .as_ref()
                .is_some_and(|meta| meta.coverage == IndexCoverage::Lazy)
        {
            let paths = session.indexes.unindexed_hits(grep_run.hits);
            if !paths.is_empty()
                && let Err(e) = daemon.index(paths)
            {
                eprintln!("sift: warning: index request failed: {e}");
            }
        }
        Ok(grep_run.outcome.matched)
    }

    fn filename_context(
        out: &SearchOutputCtx,
        sources: &SearchSources,
        session: &GrepSession,
    ) -> FilenameContext {
        if out.lines.is_path_mode {
            FilenameContext::PathMode
        } else if !sources.streams.is_empty() && sources.paths.is_empty() {
            FilenameContext::SingleFileCorpus
        } else {
            match session.indexes.corpus_kind() {
                Some(CorpusKind::SingleFile) => FilenameContext::SingleFileCorpus,
                _ => FilenameContext::DirectoryCorpus,
            }
        }
    }

    fn search_sources(
        request: &GrepRequest<'_>,
        query: &SearchQuery,
        sources: &SearchSources,
    ) -> anyhow::Result<sift_core::grep::GrepRun> {
        let streams = sources.stream_inputs();
        let mut grep_sources = Vec::with_capacity(usize::from(sources.searches_corpus()) + 1);
        if sources.searches_corpus() {
            grep_sources.push(GrepSource::Corpus);
        }
        if !streams.is_empty() {
            grep_sources.push(GrepSource::Streams(&streams));
        }

        request.search(query, &grep_sources).map_err(Into::into)
    }

    fn snapshot_validation(session: &GrepSession, daemon: Option<&Daemon>) -> SnapshotValidation {
        daemon
            .and_then(|daemon| {
                session
                    .indexes
                    .snapshot_id()
                    .map(|id| daemon.validate_snapshot(id))
            })
            .map_or(
                SnapshotValidation::Unvalidated,
                |validation| match validation {
                    Ok(true) => SnapshotValidation::Validated,
                    Ok(false) => SnapshotValidation::Stale,
                    Err(_) => SnapshotValidation::Unvalidated,
                },
            )
    }
}

fn stdin_is_pipe() -> bool {
    use std::io::IsTerminal;

    !std::io::stdin().is_terminal()
}

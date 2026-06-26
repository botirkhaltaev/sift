use std::path::PathBuf;

use sift_core::grep::CandidateSort;
use sift_core::search::{CandidateFilter, SearchMode};
use sift_core::{
    Candidate, CandidateSource, CorpusKind, IndexCoverage, Indexes, SearchQuery, SnapshotValidation,
};

use crate::index::daemon::Daemon;

use super::argv::Argv;
use super::filter::{FilterConfig, SearchFilterCtx};
use super::output::{FilenameContext, OutputArgv, OutputConfig, SearchOutputCtx};
use super::paths::CorpusScope;
use super::pattern::{PatternArgv, PatternConfig, ResolvedPatterns};

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
    pub candidate_sort: CandidateSort,
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

    fn prepare_session(&self, argv: &Argv<'_>) -> anyhow::Result<GrepSession> {
        self.configure_threads();
        let filter = SearchFilterCtx::resolve(argv);
        let cwd = std::env::current_dir()?;
        let indexes = Indexes::open(&self.config.sift_dir)?;
        let store_meta = sift_core::StoreMeta::read(&self.config.sift_dir).ok();
        let scope = CorpusScope::resolve(
            &indexes,
            store_meta.as_ref(),
            &cwd,
            &self.config.search_paths,
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
        let session = self.prepare_session(argv)?;

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
        if self.config.candidate_sort.is_sorted() {
            let mut candidates = all_paths
                .into_iter()
                .map(|rel| Candidate::new(rel.clone(), session.scope.filter_root.join(&rel)))
                .collect::<Vec<_>>();
            self.config
                .candidate_sort
                .sort_candidates(&mut candidates)?;
            all_paths = candidates
                .into_iter()
                .map(|candidate| candidate.rel_path().to_path_buf())
                .collect();
        }
        let sep = if output_argv.path.null_data {
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

        let session = self.prepare_session(argv)?;

        let opts = self
            .config
            .pattern
            .search_options(&pattern_argv, pattern_argv.only_matching);
        let query = SearchQuery::new(&patterns.0, opts).map_err(|e| anyhow::anyhow!("{e}"))?;

        let (out, _) = SearchOutputCtx::resolve(
            &self.config.output,
            argv,
            effective_mode,
            pattern_argv.quiet,
            line_number_override,
        );

        let filename_ctx = if out.lines.is_path_mode {
            FilenameContext::PathMode
        } else {
            match session.indexes.corpus_kind() {
                Some(CorpusKind::SingleFile) => FilenameContext::SingleFileCorpus,
                _ => FilenameContext::DirectoryCorpus,
            }
        };
        let output = out.to_core_output(&self.config.output, filename_ctx);
        let snapshot = daemon
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
            );

        let grep_run = sift_core::grep::GrepRequest {
            indexes: &session.indexes,
            filter: &session.search_filter,
            output,
            separators: &out.separators,
            collect: sift_core::search::SearchCollection::hits().with_stats(out.print_stats),
            candidate_source: CandidateSource {
                store_meta: session.store_meta.as_ref(),
                snapshot,
            },
            candidate_sort: self.config.candidate_sort,
        }
        .run(&query)?;
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
}

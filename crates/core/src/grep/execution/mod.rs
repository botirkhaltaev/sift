#[cfg(test)]
use std::io;
#[cfg(test)]
use std::path::{Path, PathBuf};

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

#[cfg(test)]
use grep_matcher::Matcher;
use grep_regex::RegexMatcher;

use crate::index::Indexes;
#[cfg(test)]
use crate::index::SearchCandidate;
use crate::query::{QueryFlags, QuerySpec};

pub mod candidate;
pub mod config;
pub mod error;
pub mod format;
pub mod json;
pub mod standard;
pub mod stats;
pub mod summary;
pub mod walk;

use crate::grep::SearchError;
use crate::grep::filter::CandidateInfo;
#[cfg(test)]
use crate::grep::filter::SearchFilter;
use crate::grep::output::SearchOutputFormat;
use crate::grep::output::mode::{CandidateSet, SearchMode};
use crate::grep::search::CompiledSearch;
use candidate::prepare_candidates;
use config::SearchExecution;
use format::sum_candidate_file_bytes;
use json::run_json_standard_with_info;
use standard::run_standard_with_info;
use stats::{SearchStats, StatsCollection};
use summary::run_summary_with_info;
use walk::{collect_abs_paths_for_scopes, prepare_walk_candidates};

impl CompiledSearch {
    fn build_query_spec(&self) -> QuerySpec<'_> {
        let mut flags = QueryFlags::empty();
        if self.opts.fixed_strings() {
            flags |= QueryFlags::FIXED_STRINGS;
        }
        if self.opts.case_insensitive() {
            flags |= QueryFlags::CASE_INSENSITIVE;
        }
        if self.opts.word_regexp() {
            flags |= QueryFlags::WORD_REGEXP;
        }
        if self.opts.line_regexp() {
            flags |= QueryFlags::LINE_REGEXP;
        }
        if self.opts.invert_match() {
            flags |= QueryFlags::INVERT_MATCH;
        }
        QuerySpec {
            patterns: &self.patterns,
            flags,
        }
    }

    /// Runs the search across the given indexes.
    ///
    /// # Errors
    ///
    /// Returns an error if pattern compilation fails, max-count is invalid,
    /// or output writing fails.
    pub fn run_indexes(
        &self,
        indexes: &Indexes,
        mut exec: SearchExecution<'_>,
    ) -> crate::Result<bool> {
        let filter = exec.filter;
        if self.opts.max_results == Some(0) {
            return Err(SearchError::InvalidMaxCount.into());
        }
        if indexes.is_empty() {
            if let Some(s) = exec.stats.as_mut() {
                **s = SearchStats::default();
            }
            return Ok(false);
        }

        let spec = self.build_query_spec();

        let candidates = match exec.output.candidate_set() {
            CandidateSet::AllIndexedFiles => {
                prepare_candidates(indexes.resolve_all_files(), filter)
            }
            CandidateSet::IndexedCandidates => {
                prepare_candidates(indexes.resolve_candidates(&spec), filter)
            }
        };
        if candidates.is_empty() {
            if let Some(s) = exec.stats.as_mut() {
                **s = SearchStats::default();
            }
            return Ok(false);
        }

        let search_start = Instant::now();
        let matcher = self.matcher.get_or_try_init(|| self.build_matcher())?;

        if matches!(exec.output.format, SearchOutputFormat::Json) {
            return match exec.output.mode {
                SearchMode::Standard | SearchMode::OnlyMatching => run_json_standard_with_info(
                    self,
                    &candidates,
                    matcher,
                    exec.output,
                    search_start,
                    exec.stats,
                ),
                _ => Err(SearchError::JsonOutputIncompatibleMode.into()),
            };
        }

        self.run_candidate_search(&candidates, matcher, exec, search_start)
    }

    fn run_candidate_search(
        &self,
        candidates: &[CandidateInfo],
        matcher: &RegexMatcher,
        exec: SearchExecution<'_>,
        search_start: Instant,
    ) -> crate::Result<bool> {
        let SearchExecution {
            output,
            separators,
            stats,
            ..
        } = exec;
        let match_counter = AtomicUsize::new(0);
        let counter_ref = stats.is_some().then_some(&match_counter);
        let files_with_matches = AtomicUsize::new(0);
        let files_with_ref = stats.is_some().then_some(&files_with_matches);
        let summary_counter = AtomicUsize::new(0);
        let summary_ref = stats.is_some().then_some(&summary_counter);
        let bytes_printed = AtomicU64::new(0);
        let printed_ref = stats.is_some().then_some(&bytes_printed);

        let ok = match output.mode {
            SearchMode::Standard | SearchMode::OnlyMatching => run_standard_with_info(
                self,
                candidates,
                matcher,
                output,
                separators,
                StatsCollection {
                    primary: counter_ref,
                    files_with_matches: files_with_ref,
                    bytes_printed: printed_ref,
                },
            )?,
            SearchMode::Count
            | SearchMode::CountMatches
            | SearchMode::FilesWithMatches
            | SearchMode::FilesWithoutMatch => run_summary_with_info(
                self,
                candidates,
                matcher,
                output,
                StatsCollection {
                    primary: summary_ref,
                    files_with_matches: files_with_ref,
                    bytes_printed: printed_ref,
                },
            )?,
        };

        if let Some(s) = stats {
            s.matches = match output.mode {
                SearchMode::Standard | SearchMode::OnlyMatching => {
                    match_counter.load(Ordering::Relaxed)
                }
                SearchMode::Count
                | SearchMode::CountMatches
                | SearchMode::FilesWithMatches
                | SearchMode::FilesWithoutMatch => summary_counter.load(Ordering::Relaxed),
            };
            s.files_with_matches = files_with_matches.load(Ordering::Relaxed);
            s.files_searched = candidates.len();
            s.bytes_printed = bytes_printed.load(Ordering::Relaxed);
            s.bytes_searched = sum_candidate_file_bytes(candidates);
            s.elapsed = search_start.elapsed();
        }

        Ok(ok)
    }

    /// Runs the search across a filesystem walk starting from filter scopes.
    ///
    /// # Errors
    ///
    /// Returns an error if pattern compilation fails, max-count is invalid,
    /// or output writing fails.
    pub fn run_walk(&self, mut exec: SearchExecution<'_>) -> crate::Result<bool> {
        if self.opts.max_results == Some(0) {
            return Err(SearchError::InvalidMaxCount.into());
        }

        let abs_paths = collect_abs_paths_for_scopes(exec.filter)?;
        if abs_paths.is_empty() {
            if let Some(s) = exec.stats.as_mut() {
                **s = SearchStats::default();
            }
            return Ok(false);
        }

        let candidates = prepare_walk_candidates(&abs_paths, exec.filter);
        if candidates.is_empty() {
            if let Some(s) = exec.stats.as_mut() {
                **s = SearchStats::default();
            }
            return Ok(false);
        }

        let search_start = Instant::now();
        let matcher = self.matcher.get_or_try_init(|| self.build_matcher())?;

        if matches!(exec.output.format, SearchOutputFormat::Json) {
            return match exec.output.mode {
                SearchMode::Standard | SearchMode::OnlyMatching => run_json_standard_with_info(
                    self,
                    &candidates,
                    matcher,
                    exec.output,
                    search_start,
                    exec.stats,
                ),
                _ => Err(SearchError::JsonOutputIncompatibleMode.into()),
            };
        }

        self.run_candidate_search(&candidates, matcher, exec, search_start)
    }

    #[cfg(test)]
    pub(crate) fn collect_index_matches(
        &self,
        index: &dyn crate::index::SearchIndex,
    ) -> crate::Result<Vec<crate::grep::search::Match>> {
        use crate::grep::filter::config::{
            HiddenMode, IgnoreConfig, SearchFilterConfig, VisibilityConfig,
        };
        let config = SearchFilterConfig {
            visibility: VisibilityConfig {
                hidden: HiddenMode::Include,
                ignore: IgnoreConfig::default(),
            },
            ..SearchFilterConfig::default()
        };
        let filter = SearchFilter::new(&config, index.root())?;
        let spec = self.build_query_spec();
        let candidates = index.candidates(&spec);
        self.collect_index_candidate_paths(&filter, &candidates)
    }

    #[cfg(test)]
    pub(crate) fn collect_walk_matches(
        &self,
        root: &Path,
    ) -> crate::Result<Vec<crate::grep::search::Match>> {
        let root = root.canonicalize()?;
        let mut candidates = Vec::new();
        let walker = ignore::WalkBuilder::new(&root)
            .follow_links(false)
            .hidden(false)
            .parents(false)
            .ignore(false)
            .git_global(false)
            .git_ignore(false)
            .git_exclude(false)
            .require_git(false)
            .build();
        for entry in walker {
            let entry = entry.map_err(crate::Error::Ignore)?;
            if entry.file_type().is_some_and(|ft| ft.is_file()) {
                let path = entry.path();
                if path.components().any(|c| c.as_os_str() == ".sift") {
                    continue;
                }
                candidates.push(path.to_path_buf());
            }
        }
        self.collect_walk_candidates(&candidates)
    }

    #[cfg(test)]
    fn collect_index_candidate_paths(
        &self,
        filter: &SearchFilter,
        candidates: &[SearchCandidate],
    ) -> crate::Result<Vec<crate::grep::search::Match>> {
        use crate::grep::output::mode::MatchEmissionMode;
        let matcher = self.matcher.get_or_try_init(|| self.build_matcher())?;
        let mut out = Vec::new();
        let mut searcher = self.build_searcher(true, None, true);
        for candidate in candidates {
            if !filter.is_candidate(&candidate.rel_path) {
                continue;
            }
            let mut sink = CollectSink::new(
                candidate.abs_path.clone(),
                if self.opts.only_matching() {
                    MatchEmissionMode::OnlyMatching
                } else {
                    MatchEmissionMode::Lines
                },
                matcher.clone(),
            );
            let _ = searcher.search_path(matcher, &candidate.abs_path, &mut sink);
            out.extend(sink.into_matches());
        }
        Ok(out)
    }

    #[cfg(test)]
    fn collect_walk_candidates(
        &self,
        candidates: &[PathBuf],
    ) -> crate::Result<Vec<crate::grep::search::Match>> {
        use crate::grep::output::mode::MatchEmissionMode;
        let matcher = self.matcher.get_or_try_init(|| self.build_matcher())?;
        let mut out = Vec::new();
        let mut searcher = self.build_searcher(true, None, true);
        for candidate in candidates {
            let mut sink = CollectSink::new(
                candidate.clone(),
                if self.opts.only_matching() {
                    MatchEmissionMode::OnlyMatching
                } else {
                    MatchEmissionMode::Lines
                },
                matcher.clone(),
            );
            let _ = searcher.search_path(matcher, candidate, &mut sink);
            out.extend(sink.into_matches());
        }
        Ok(out)
    }
}

pub use walk::discover_files;

#[cfg(test)]
struct CollectSink {
    path: PathBuf,
    emission: crate::grep::output::mode::MatchEmissionMode,
    matcher: RegexMatcher,
    matches: Vec<crate::grep::search::Match>,
}

#[cfg(test)]
impl CollectSink {
    fn new(
        path: PathBuf,
        emission: crate::grep::output::mode::MatchEmissionMode,
        matcher: RegexMatcher,
    ) -> Self {
        Self {
            path,
            emission,
            matcher,
            matches: Vec::new(),
        }
    }

    fn into_matches(self) -> Vec<crate::grep::search::Match> {
        self.matches
    }
}

#[cfg(test)]
impl grep_searcher::Sink for CollectSink {
    type Error = io::Error;

    fn matched(
        &mut self,
        _: &grep_searcher::Searcher,
        mat: &grep_searcher::SinkMatch<'_>,
    ) -> Result<bool, Self::Error> {
        let line = usize::try_from(mat.line_number().unwrap_or(0)).unwrap_or(0);
        let line_bytes = mat.bytes();
        if matches!(
            self.emission,
            crate::grep::output::mode::MatchEmissionMode::OnlyMatching
        ) {
            let _ = self
                .matcher
                .find_iter(line_bytes, |m: grep_matcher::Match| {
                    self.matches.push(crate::grep::search::Match {
                        file: self.path.clone(),
                        line,
                        text: String::from_utf8_lossy(&line_bytes[m.start()..m.end()]).into_owned(),
                    });
                    true
                });
        } else {
            self.matches.push(crate::grep::search::Match {
                file: self.path.clone(),
                line,
                text: String::from_utf8_lossy(line_bytes).into_owned(),
            });
        }
        Ok(true)
    }
}

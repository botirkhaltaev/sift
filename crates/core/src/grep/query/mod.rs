use std::sync::Mutex;
use std::time::Instant;
#[cfg(test)]
use std::{
    io,
    path::{Path, PathBuf},
};

#[cfg(test)]
use grep_matcher::Matcher;
use grep_regex::RegexMatcher;
use grep_searcher::Searcher;
use once_cell::sync::OnceCell;

use crate::grep::SearchError;
use crate::grep::SearchOutcome;
use crate::grep::emit::format::sum_candidate_file_bytes;
use crate::grep::emit::stats::{SearchStats, TextStatsCounters};
#[cfg(test)]
use crate::grep::filter::SearchFilter;
#[cfg(test)]
use crate::grep::filter::config::{HiddenMode, IgnoreConfig, SearchFilterConfig, VisibilityConfig};
use crate::grep::options::SearchOptions;
use crate::grep::output::SearchOutputFormat;
#[cfg(test)]
use crate::grep::output::mode::MatchEmissionMode;
use crate::grep::output::mode::SearchMode;
use crate::grep::request::SearchRequest;
#[cfg(test)]
use crate::index::SearchCandidate;
use crate::query::QueryFlags;
use crate::query::QuerySpec;

pub mod matcher;

type SearcherCacheEntry = ((bool, Option<usize>, usize, usize), Searcher);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub file: std::path::PathBuf,
    pub line: usize,
    pub text: String,
}

#[derive(Debug)]
pub struct SearchQuery {
    pub patterns: Vec<String>,
    pub opts: SearchOptions,
    pub matcher: OnceCell<RegexMatcher>,
    pub searcher_cache: Mutex<Option<SearcherCacheEntry>>,
}

impl SearchQuery {
    /// Creates a new search query from patterns and options.
    ///
    /// # Errors
    ///
    /// Returns `SearchError::EmptyPatterns` if the pattern list is empty.
    pub fn new(patterns: &[String], opts: SearchOptions) -> Result<Self, SearchError> {
        if patterns.is_empty() {
            return Err(SearchError::EmptyPatterns);
        }
        Ok(Self {
            patterns: patterns.to_vec(),
            opts,
            matcher: OnceCell::new(),
            searcher_cache: Mutex::new(None),
        })
    }

    #[must_use]
    pub fn patterns(&self) -> &[String] {
        &self.patterns
    }

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

    fn run_text_output(
        &self,
        candidates: &[crate::grep::filter::CandidateInfo],
        matcher: &RegexMatcher,
        output: crate::grep::output::SearchOutput,
        separators: &crate::grep::output::style::SearchSeparators,
        search_start: Instant,
        collect_stats: bool,
    ) -> crate::Result<(bool, Option<SearchStats>)> {
        let counters = TextStatsCounters::new(collect_stats);

        let did_match = match output.mode {
            SearchMode::Standard | SearchMode::OnlyMatching => {
                let scan = crate::grep::scan::standard::StandardScan::new(
                    self, matcher, output, separators, &counters,
                );
                scan.run(candidates)?
            }
            SearchMode::Count
            | SearchMode::CountMatches
            | SearchMode::FilesWithMatches
            | SearchMode::FilesWithoutMatch => {
                let scan =
                    crate::grep::scan::summary::SummaryScan::new(self, matcher, output, &counters);
                scan.run(candidates)?
            }
        };

        let stats = counters.finish(
            candidates.len(),
            sum_candidate_file_bytes(candidates),
            search_start.elapsed(),
        );

        Ok((did_match, stats))
    }

    /// Runs the search across the provided request.
    ///
    /// # Errors
    ///
    /// Returns an error if regex compilation, candidate resolution, or output emission fails.
    pub fn run(&self, request: SearchRequest<'_>) -> crate::Result<SearchOutcome> {
        if self.opts.max_results == Some(0) {
            return Err(SearchError::InvalidMaxCount.into());
        }

        let spec = self.build_query_spec();
        let output = request.output;
        let candidates = request.resolve_candidates(&spec)?;

        if candidates.is_empty() {
            return Ok(SearchOutcome {
                matched: false,
                stats: request.collect_stats.then_some(SearchStats::default()),
            });
        }

        let search_start = Instant::now();
        let matcher = self.matcher.get_or_try_init(|| self.build_matcher())?;

        let (did_match, stats) = match output.format {
            SearchOutputFormat::Json => match output.mode {
                SearchMode::Standard | SearchMode::OnlyMatching => {
                    let mut stats = request.collect_stats.then_some(SearchStats::default());
                    let scan =
                        crate::grep::scan::json::JsonScan::new(self, matcher, output, search_start);
                    let json_matched = scan.run(&candidates, stats.as_mut())?;
                    (json_matched, stats)
                }
                _ => return Err(SearchError::JsonOutputIncompatibleMode.into()),
            },
            SearchOutputFormat::Text => self.run_text_output(
                &candidates,
                matcher,
                output,
                request.separators,
                search_start,
                request.collect_stats,
            )?,
        };

        Ok(SearchOutcome {
            matched: did_match,
            stats,
        })
    }

    #[cfg(test)]
    pub(crate) fn collect_index_matches(
        &self,
        index: &dyn crate::index::SearchIndex,
    ) -> crate::Result<Vec<crate::grep::Match>> {
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
    ) -> crate::Result<Vec<crate::grep::Match>> {
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
    ) -> crate::Result<Vec<crate::grep::Match>> {
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
    ) -> crate::Result<Vec<crate::grep::Match>> {
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

#[cfg(test)]
struct CollectSink {
    path: PathBuf,
    emission: MatchEmissionMode,
    matcher: RegexMatcher,
    matches: Vec<crate::grep::Match>,
}

#[cfg(test)]
impl CollectSink {
    fn new(path: PathBuf, emission: MatchEmissionMode, matcher: RegexMatcher) -> Self {
        Self {
            path,
            emission,
            matcher,
            matches: Vec::new(),
        }
    }

    fn into_matches(self) -> Vec<crate::grep::Match> {
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
        if matches!(self.emission, MatchEmissionMode::OnlyMatching) {
            let _ = self
                .matcher
                .find_iter(line_bytes, |m: grep_matcher::Match| {
                    self.matches.push(crate::grep::Match {
                        file: self.path.clone(),
                        line,
                        text: String::from_utf8_lossy(&line_bytes[m.start()..m.end()]).into_owned(),
                    });
                    true
                });
        } else {
            self.matches.push(crate::grep::Match {
                file: self.path.clone(),
                line,
                text: String::from_utf8_lossy(line_bytes).into_owned(),
            });
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grep::options::SearchMatchFlags;

    #[test]
    fn case_mode_insensitive_returns_true() {
        assert!(crate::grep::options::CaseMode::Insensitive.is_case_insensitive());
    }

    #[test]
    fn case_mode_sensitive_returns_false() {
        assert!(!crate::grep::options::CaseMode::Sensitive.is_case_insensitive());
    }

    #[test]
    fn case_mode_smart_returns_false() {
        assert!(!crate::grep::options::CaseMode::Smart.is_case_insensitive());
    }

    #[test]
    fn search_options_defaults() {
        let opts = SearchOptions::default();
        assert!(!opts.case_insensitive());
        assert!(!opts.invert_match());
        assert!(!opts.fixed_strings());
        assert!(!opts.word_regexp());
        assert!(!opts.line_regexp());
        assert!(!opts.only_matching());
        assert!(!opts.multiline());
        assert!(!opts.multiline_dotall());
        assert!(!opts.crlf());
        assert!(!opts.precludes_trigram_index());
        assert_eq!(opts.max_results, None);
        assert_eq!(opts.before_context, 0);
        assert_eq!(opts.after_context, 0);
        assert_eq!(opts.binary_mode, crate::grep::options::BinaryMode::Quit);
        assert!(opts.unicode);
    }

    #[test]
    fn search_options_precludes_trigram_index_only_for_invert_match() {
        let mut opts = SearchOptions::default();
        assert!(!opts.precludes_trigram_index());

        opts.flags |= SearchMatchFlags::INVERT_MATCH;
        assert!(opts.precludes_trigram_index());
    }

    #[test]
    fn search_query_new_rejects_empty_patterns() {
        let result = SearchQuery::new(&[], SearchOptions::default());
        assert!(result.is_err());
    }

    #[test]
    fn search_query_new_stores_patterns_and_options() {
        let patterns = vec!["foo".to_string(), "bar".to_string()];
        let opts = SearchOptions {
            case_mode: crate::grep::options::CaseMode::Insensitive,
            ..SearchOptions::default()
        };
        let search = SearchQuery::new(&patterns, opts).expect("create search");
        assert_eq!(search.patterns(), &patterns);
        assert!(search.opts.case_insensitive());
    }
}

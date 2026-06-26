use std::path::PathBuf;
use std::time::Instant;
#[cfg(test)]
use std::{io, path::Path};

#[cfg(test)]
use grep_matcher::Matcher;
use grep_regex::RegexMatcher;
use std::sync::OnceLock;

#[cfg(test)]
use crate::Candidate;
use crate::query::{QueryFlags, QuerySpec};
use crate::search::SearchError;
use crate::search::SearchOutcome;
use crate::search::emit::stats::{SearchStats, TextStatsCounters};
#[cfg(test)]
use crate::search::filter::CandidateFilter;
#[cfg(test)]
use crate::search::filter::config::{
    CandidateFilterConfig, HiddenMode, IgnoreConfig, VisibilityConfig,
};
use crate::search::options::SearchOptions;
use crate::search::output::SearchOutputFormat;
#[cfg(test)]
use crate::search::output::mode::MatchEmissionMode;
use crate::search::output::mode::SearchMode;
use crate::search::request::SearchExecution;

pub mod matcher;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub file: std::path::PathBuf,
    pub line: usize,
    pub text: String,
}

#[derive(Debug)]
pub struct SearchQuery {
    patterns: Vec<String>,
    opts: SearchOptions,
    matcher: OnceLock<RegexMatcher>,
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
            matcher: OnceLock::new(),
        })
    }

    #[must_use]
    pub fn patterns(&self) -> &[String] {
        &self.patterns
    }

    #[must_use]
    pub const fn opts(&self) -> &SearchOptions {
        &self.opts
    }

    pub(crate) fn build_query_spec(&self) -> QuerySpec<'_> {
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

    pub(crate) fn search(
        &self,
        execution: &SearchExecution<'_>,
    ) -> crate::Result<(SearchOutcome, Vec<PathBuf>)> {
        if self.opts.max_results == Some(0) {
            return Err(SearchError::InvalidMaxCount.into());
        }

        let output = execution.output;
        let candidates = execution.candidates;
        let transformed = execution.transformed;

        if candidates.is_empty() {
            return Ok((
                SearchOutcome {
                    matched: false,
                    stats: execution.collect.stats.then_some(SearchStats::default()),
                },
                Vec::new(),
            ));
        }

        let search_start = Instant::now();
        let matcher = self.resolve_matcher()?;

        let (did_match, stats, hits) = match output.format {
            SearchOutputFormat::Json => match output.mode {
                SearchMode::Standard | SearchMode::OnlyMatching => {
                    let mut stats = execution.collect.stats.then_some(SearchStats::default());
                    let scan = crate::search::scan::json::JsonScan::new(
                        self,
                        matcher,
                        output,
                        search_start,
                    );
                    let json_matched = scan.run(candidates, transformed, stats.as_mut())?;
                    (json_matched, stats, Vec::new())
                }
                _ => return Err(SearchError::JsonOutputIncompatibleMode.into()),
            },
            SearchOutputFormat::Text => {
                let (did_match, stats, hits) = self.run_text_output(
                    candidates,
                    transformed,
                    matcher,
                    execution,
                    search_start,
                )?;
                (did_match, stats, hits)
            }
        };

        Ok((
            SearchOutcome {
                matched: did_match,
                stats,
            },
            hits,
        ))
    }

    fn resolve_matcher(&self) -> Result<&RegexMatcher, SearchError> {
        if let Some(m) = self.matcher.get() {
            return Ok(m);
        }
        let m = self.build_matcher()?;
        let _ = self.matcher.set(m);
        Ok(self.matcher.get().expect("just initialised"))
    }

    fn run_text_output(
        &self,
        candidates: &[crate::Candidate],
        transformed: Option<&[crate::search::request::CandidateContent]>,
        matcher: &RegexMatcher,
        execution: &SearchExecution<'_>,
        search_start: Instant,
    ) -> crate::Result<(bool, Option<SearchStats>, Vec<PathBuf>)> {
        let output = execution.output;
        let collect = execution.collect;
        let counters = TextStatsCounters::new(collect.stats);

        let (did_match, hits) = match output.mode {
            SearchMode::Standard | SearchMode::OnlyMatching => {
                let scan = crate::search::scan::standard::StandardScan::new(
                    self,
                    matcher,
                    output,
                    execution.separators,
                    &counters,
                );
                scan.run(candidates, transformed, collect)?
            }
            SearchMode::Count
            | SearchMode::CountMatches
            | SearchMode::FilesWithMatches
            | SearchMode::FilesWithoutMatch => {
                let scan = crate::search::scan::summary::SummaryScan::new(
                    self, matcher, output, &counters,
                );
                scan.run(candidates, transformed, collect)?
            }
        };

        let bytes_searched = if collect.stats {
            transformed.map_or_else(
                || crate::Candidate::total_file_bytes(candidates),
                |items| items.iter().map(|item| item.bytes.len() as u64).sum(),
            )
        } else {
            0
        };
        let input_count = transformed.map_or(
            candidates.len(),
            <[crate::search::request::CandidateContent]>::len,
        );
        let stats = counters.finish(input_count, bytes_searched, search_start.elapsed());

        Ok((did_match, stats, hits))
    }

    #[cfg(test)]
    pub(crate) fn collect_index_matches(
        &self,
        index: &crate::index::Index,
    ) -> crate::Result<Vec<crate::search::Match>> {
        let config = CandidateFilterConfig {
            visibility: VisibilityConfig {
                hidden: HiddenMode::Include,
                ignore: IgnoreConfig::default(),
            },
            ..CandidateFilterConfig::default()
        };
        let filter = CandidateFilter::new(&config, index.root())?;
        let spec = self.build_query_spec();
        let candidates = index.candidates(&spec).unwrap_or_default();
        self.collect_index_candidate_paths(&filter, &candidates)
    }

    #[cfg(test)]
    pub(crate) fn collect_walk_matches(
        &self,
        root: &Path,
    ) -> crate::Result<Vec<crate::search::Match>> {
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
        filter: &CandidateFilter,
        candidates: &[Candidate],
    ) -> crate::Result<Vec<crate::search::Match>> {
        let matcher = self.resolve_matcher()?;
        let mut out = Vec::new();
        let mut searcher = self.build_searcher(true, None, true);
        for candidate in candidates {
            if !candidate.matches(filter) {
                continue;
            }
            let mut sink = CollectSink::new(
                candidate.abs_path().to_path_buf(),
                if self.opts.only_matching() {
                    MatchEmissionMode::OnlyMatching
                } else {
                    MatchEmissionMode::Lines
                },
                matcher.clone(),
            );
            let _ = searcher.search_path(matcher, candidate.abs_path(), &mut sink);
            out.extend(sink.matches);
        }
        Ok(out)
    }

    #[cfg(test)]
    fn collect_walk_candidates(
        &self,
        candidates: &[PathBuf],
    ) -> crate::Result<Vec<crate::search::Match>> {
        let matcher = self.resolve_matcher()?;
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
            out.extend(sink.matches);
        }
        Ok(out)
    }
}

#[cfg(test)]
struct CollectSink {
    path: PathBuf,
    emission: MatchEmissionMode,
    matcher: RegexMatcher,
    matches: Vec<crate::search::Match>,
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
}

#[cfg(test)]
impl grep_searcher::Sink for CollectSink {
    type Error = io::Error;

    fn matched(
        &mut self,
        searcher: &grep_searcher::Searcher,
        mat: &grep_searcher::SinkMatch<'_>,
    ) -> Result<bool, Self::Error> {
        std::hint::black_box(searcher);
        let line = usize::try_from(mat.line_number().unwrap_or(0)).unwrap_or(0);
        let line_bytes = mat.bytes();
        if matches!(self.emission, MatchEmissionMode::OnlyMatching) {
            let _ = self
                .matcher
                .find_iter(line_bytes, |m: grep_matcher::Match| {
                    self.matches.push(crate::search::Match {
                        file: self.path.clone(),
                        line,
                        text: String::from_utf8_lossy(&line_bytes[m.start()..m.end()]).into_owned(),
                    });
                    true
                });
        } else {
            self.matches.push(crate::search::Match {
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
    use crate::search::options::SearchMatchFlags;

    #[test]
    fn case_mode_insensitive_returns_true() {
        assert!(crate::search::options::CaseMode::Insensitive.is_case_insensitive());
    }

    #[test]
    fn case_mode_sensitive_returns_false() {
        assert!(!crate::search::options::CaseMode::Sensitive.is_case_insensitive());
    }

    #[test]
    fn case_mode_smart_returns_false() {
        assert!(!crate::search::options::CaseMode::Smart.is_case_insensitive());
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
        assert_eq!(opts.binary_mode, crate::search::options::BinaryMode::Quit);
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
            case_mode: crate::search::options::CaseMode::Insensitive,
            ..SearchOptions::default()
        };
        let search = SearchQuery::new(&patterns, opts).expect("create search");
        assert_eq!(search.patterns(), &patterns);
        assert!(search.opts().case_insensitive());
    }
}

#[cfg(test)]
use std::{io, path::Path, path::PathBuf};

#[cfg(test)]
use grep_matcher::Matcher;
use std::sync::OnceLock;

#[cfg(test)]
use crate::Candidate;
use crate::grep::GrepError;
#[cfg(test)]
use crate::grep::filter::CandidateFilter;
#[cfg(test)]
use crate::grep::filter::config::{
    CandidateFilterConfig, HiddenMode, IgnoreConfig, VisibilityConfig,
};
use crate::grep::options::{GrepOptions, RegexEngineRequest};
#[cfg(test)]
use crate::grep::output::mode::MatchEmissionMode;
use crate::query::{QueryFlags, QuerySpec};
use matcher::GrepMatcher;

pub(crate) mod matcher;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub file: std::path::PathBuf,
    pub line: usize,
    pub text: String,
}

#[derive(Debug)]
pub struct GrepQuery {
    patterns: Vec<String>,
    opts: GrepOptions,
    compiled: OnceLock<CompiledGrepQuery>,
}

#[derive(Debug)]
pub(crate) struct CompiledGrepQuery {
    matcher: GrepMatcher,
    candidate_strategy: CandidateStrategy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResolvedRegexEngine {
    Rust,
    Pcre2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CandidateStrategy {
    Indexed,
    Complete(CompleteCandidateReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompleteCandidateReason {
    InvertedMatch,
    DecodedInput,
    RegexEngineUnsupportedByPlanner,
}

impl CompiledGrepQuery {
    #[must_use]
    pub(crate) const fn matcher(&self) -> &GrepMatcher {
        &self.matcher
    }

    #[must_use]
    pub(crate) const fn candidate_strategy(&self) -> CandidateStrategy {
        self.candidate_strategy
    }
}

impl GrepQuery {
    /// Creates a new search query from patterns and options.
    ///
    /// # Errors
    ///
    /// Returns `GrepError::EmptyPatterns` if the pattern list is empty.
    pub fn new(patterns: Vec<String>) -> Result<Self, GrepError> {
        if patterns.is_empty() {
            return Err(GrepError::EmptyPatterns);
        }
        Ok(Self {
            patterns,
            opts: GrepOptions::default(),
            compiled: OnceLock::new(),
        })
    }

    #[must_use]
    pub fn options(mut self, opts: GrepOptions) -> Self {
        self.opts = opts;
        self.compiled = OnceLock::new();
        self
    }

    #[must_use]
    pub fn patterns(&self) -> &[String] {
        &self.patterns
    }

    #[must_use]
    pub const fn opts(&self) -> &GrepOptions {
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

    pub(crate) fn compile(&self) -> Result<&CompiledGrepQuery, GrepError> {
        if let Some(compiled) = self.compiled.get() {
            return Ok(compiled);
        }
        let compiled = self.build_compiled_search()?;
        let _ = self.compiled.set(compiled);
        Ok(self.compiled.get().expect("just initialised"))
    }

    fn build_compiled_search(&self) -> Result<CompiledGrepQuery, GrepError> {
        let (matcher, engine) = match self.opts.regex_engine {
            RegexEngineRequest::Rust => (self.build_rust_matcher()?, ResolvedRegexEngine::Rust),
            RegexEngineRequest::Pcre2 => (self.build_pcre2_matcher()?, ResolvedRegexEngine::Pcre2),
            RegexEngineRequest::Auto => match self.build_rust_matcher() {
                Ok(matcher) => (matcher, ResolvedRegexEngine::Rust),
                Err(_) => (self.build_pcre2_matcher()?, ResolvedRegexEngine::Pcre2),
            },
        };

        Ok(CompiledGrepQuery {
            matcher,
            candidate_strategy: self.candidate_strategy(engine),
        })
    }

    fn candidate_strategy(&self, engine: ResolvedRegexEngine) -> CandidateStrategy {
        if self.opts.invert_match() {
            CandidateStrategy::Complete(CompleteCandidateReason::InvertedMatch)
        } else if self.opts.input_encoding.uses_decoded_input() {
            CandidateStrategy::Complete(CompleteCandidateReason::DecodedInput)
        } else if engine != ResolvedRegexEngine::Rust {
            CandidateStrategy::Complete(CompleteCandidateReason::RegexEngineUnsupportedByPlanner)
        } else {
            CandidateStrategy::Indexed
        }
    }

    #[cfg(test)]
    pub(crate) fn collect_index_matches(
        &self,
        index: &crate::index::Index,
    ) -> crate::Result<Vec<crate::grep::Match>> {
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
        filter: &CandidateFilter,
        candidates: &[Candidate],
    ) -> crate::Result<Vec<crate::grep::Match>> {
        let matcher = self.compile()?.matcher();
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
    ) -> crate::Result<Vec<crate::grep::Match>> {
        let matcher = self.compile()?.matcher();
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
    matcher: GrepMatcher,
    matches: Vec<crate::grep::Match>,
}

#[cfg(test)]
impl CollectSink {
    fn new(path: PathBuf, emission: MatchEmissionMode, matcher: GrepMatcher) -> Self {
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
    use crate::grep::options::GrepMatchFlags;

    fn make_search(patterns: &[&str], opts: GrepOptions) -> GrepQuery {
        let patterns: Vec<String> = patterns.iter().map(ToString::to_string).collect();
        GrepQuery::new(patterns)
            .expect("compile search")
            .options(opts)
    }

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
        let opts = GrepOptions::default();
        assert!(!opts.case_insensitive());
        assert!(!opts.invert_match());
        assert!(!opts.fixed_strings());
        assert!(!opts.word_regexp());
        assert!(!opts.line_regexp());
        assert!(!opts.only_matching());
        assert!(!opts.multiline());
        assert!(!opts.multiline_dotall());
        assert!(!opts.crlf());
        assert_eq!(opts.max_results, None);
        assert_eq!(opts.before_context, 0);
        assert_eq!(opts.after_context, 0);
        assert_eq!(opts.binary_mode, crate::grep::options::BinaryMode::Quit);
        assert!(opts.unicode);
    }

    #[test]
    fn compiled_search_selects_candidate_strategy() {
        let mut opts = GrepOptions {
            input_encoding: crate::grep::options::InputEncoding::Raw,
            ..GrepOptions::default()
        };
        let search = make_search(&["needle"], opts.clone());
        assert_eq!(
            search.compile().unwrap().candidate_strategy(),
            CandidateStrategy::Indexed
        );

        opts.flags |= GrepMatchFlags::INVERT_MATCH;
        let search = make_search(&["needle"], opts.clone());
        assert_eq!(
            search.compile().unwrap().candidate_strategy(),
            CandidateStrategy::Complete(CompleteCandidateReason::InvertedMatch)
        );

        opts.flags.remove(GrepMatchFlags::INVERT_MATCH);
        opts.input_encoding = crate::grep::options::InputEncoding::Auto;
        let search = make_search(&["needle"], opts.clone());
        assert_eq!(
            search.compile().unwrap().candidate_strategy(),
            CandidateStrategy::Complete(CompleteCandidateReason::DecodedInput)
        );

        opts.input_encoding = crate::grep::options::InputEncoding::Raw;
        opts.regex_engine = crate::grep::options::RegexEngineRequest::Pcre2;
        let search = make_search(&["needle"], opts.clone());
        assert_eq!(
            search.compile().unwrap().candidate_strategy(),
            CandidateStrategy::Complete(CompleteCandidateReason::RegexEngineUnsupportedByPlanner)
        );

        opts.regex_engine = crate::grep::options::RegexEngineRequest::Auto;
        let search = make_search(&["needle"], opts.clone());
        assert_eq!(
            search.compile().unwrap().candidate_strategy(),
            CandidateStrategy::Indexed
        );

        let search = make_search(&["(?<=ba)r"], opts);
        assert_eq!(
            search.compile().unwrap().candidate_strategy(),
            CandidateStrategy::Complete(CompleteCandidateReason::RegexEngineUnsupportedByPlanner)
        );
    }

    #[test]
    fn options_resets_compiled_query() {
        let raw_opts = GrepOptions {
            input_encoding: crate::grep::options::InputEncoding::Raw,
            ..GrepOptions::default()
        };
        let search = GrepQuery::new(vec!["needle".to_string()]).expect("create search");
        assert_eq!(
            search.compile().unwrap().candidate_strategy(),
            CandidateStrategy::Complete(CompleteCandidateReason::DecodedInput)
        );

        let search = search.options(raw_opts);
        assert_eq!(
            search.compile().unwrap().candidate_strategy(),
            CandidateStrategy::Indexed
        );
    }

    #[test]
    fn search_query_new_rejects_empty_patterns() {
        let result = GrepQuery::new(Vec::new());
        assert!(result.is_err());
    }

    #[test]
    fn search_query_new_stores_patterns_and_options() {
        let patterns = vec!["foo".to_string(), "bar".to_string()];
        let opts = GrepOptions {
            case_mode: crate::grep::options::CaseMode::Insensitive,
            ..GrepOptions::default()
        };
        let search = GrepQuery::new(patterns.clone())
            .expect("create search")
            .options(opts);
        assert_eq!(search.patterns(), &patterns);
        assert!(search.opts().case_insensitive());
    }
}

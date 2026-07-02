use std::sync::OnceLock;

use crate::corpus::Candidate;
use crate::grep::Error;
use crate::grep::input::Inputs;
use crate::grep::options::MatchOptions;
use crate::grep::policy::CandidatePolicy;
use crate::grep::report::Report;
use crate::grep::session::Session;
use crate::grep::stats::StatsMode;
use crate::query::{PlanContext, QueryFlags, QueryPlanner, QuerySpec, ResolutionConfig};

mod compile;
pub mod error;
mod search;

pub use compile::{CompiledQuery, Indexability, IndexabilityReason};

use regex_automata::meta::Regex;
use regex_syntax::escape;

use crate::grep::options::MatchFlags;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub file: std::path::PathBuf,
    pub line: usize,
    pub text: String,
}

#[derive(Debug)]
pub struct Query {
    pub(crate) patterns: Vec<String>,
    pub(crate) opts: MatchOptions,
    pub(crate) compiled: OnceLock<CompiledQuery>,
}

impl Clone for Query {
    fn clone(&self) -> Self {
        Self {
            patterns: self.patterns.clone(),
            opts: self.opts.clone(),
            compiled: OnceLock::new(),
        }
    }
}

impl Query {
    /// Creates a new search query from patterns.
    ///
    /// # Errors
    ///
    /// Returns `Error::EmptyPatterns` if the pattern list is empty.
    pub fn new(patterns: Vec<String>) -> Result<Self, Error> {
        if patterns.is_empty() {
            return Err(Error::EmptyPatterns);
        }
        Ok(Self {
            patterns,
            opts: MatchOptions::default(),
            compiled: OnceLock::new(),
        })
    }

    #[must_use]
    pub fn options(mut self, opts: MatchOptions) -> Self {
        self.opts = opts;
        self.compiled = OnceLock::new();
        self
    }

    #[must_use]
    pub fn patterns(&self) -> &[String] {
        &self.patterns
    }

    #[must_use]
    pub const fn opts(&self) -> &MatchOptions {
        &self.opts
    }

    #[must_use]
    pub fn query_spec(&self) -> QuerySpec<'_> {
        let mut flags = QueryFlags::empty();
        if self.opts().fixed_strings() {
            flags |= QueryFlags::FIXED_STRINGS;
        }
        if self.opts().case_insensitive() {
            flags |= QueryFlags::CASE_INSENSITIVE;
        }
        if self.opts().word_regexp() {
            flags |= QueryFlags::WORD_REGEXP;
        }
        if self.opts().line_regexp() {
            flags |= QueryFlags::LINE_REGEXP;
        }
        if self.opts().invert_match() {
            flags |= QueryFlags::INVERT_MATCH;
        }
        QuerySpec {
            patterns: self.patterns(),
            flags,
        }
    }

    fn validate_max_results(&self) -> crate::Result<()> {
        if self.opts().max_results == Some(0) {
            return Err(crate::Error::Search(Error::InvalidMaxCount));
        }
        Ok(())
    }

    /// Resolve candidate files for this query under the given session and policy.
    ///
    /// # Errors
    ///
    /// Returns an error if candidate resolution or regex compilation fails.
    pub fn candidates(
        &self,
        session: &Session,
        policy: CandidatePolicy,
    ) -> crate::Result<Vec<Candidate>> {
        self.validate_max_results()?;
        let spec = self.query_spec();
        let compiled = self.compile()?;
        QueryPlanner::new(spec).resolve(
            PlanContext::new(
                session.indexes,
                session.filter,
                session.store_meta,
                compiled.index_capable(),
            ),
            ResolutionConfig {
                coverage: policy.coverage(),
                fallback: policy.fallback,
                order: policy.order,
            },
        )
    }

    /// Search the given inputs and return a report.
    ///
    /// Compiles patterns if not already cached. When the query is already
    /// compiled (for example after [`Self::candidates`]), prefer
    /// [`CompiledQuery::report`].
    ///
    /// # Errors
    ///
    /// Returns an error if regex compilation or search execution fails.
    pub fn search(&self, inputs: &Inputs, stats: StatsMode) -> crate::Result<Report> {
        self.validate_max_results()?;
        Ok(self.compile()?.report(self, inputs, stats))
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PatternCompiler {
    flags: MatchFlags,
    case_insensitive: bool,
}

impl PatternCompiler {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            flags: MatchFlags::empty(),
            case_insensitive: false,
        }
    }

    #[must_use]
    pub fn fixed_strings(mut self, on: bool) -> Self {
        self.flags.set(MatchFlags::FIXED_STRINGS, on);
        self
    }

    #[must_use]
    pub fn word_regexp(mut self, on: bool) -> Self {
        self.flags.set(MatchFlags::WORD_REGEXP, on);
        self
    }

    #[must_use]
    pub fn line_regexp(mut self, on: bool) -> Self {
        self.flags.set(MatchFlags::LINE_REGEXP, on);
        self
    }

    #[must_use]
    pub const fn case_insensitive(mut self, on: bool) -> Self {
        self.case_insensitive = on;
        self
    }

    #[must_use]
    pub fn shape(&self, pattern: &str) -> String {
        let mut s = if self.flags.contains(MatchFlags::FIXED_STRINGS) {
            escape(pattern)
        } else {
            pattern.to_string()
        };
        if self.flags.contains(MatchFlags::WORD_REGEXP) {
            s = format!(r"\b(?:{s})\b");
        }
        if self.flags.contains(MatchFlags::LINE_REGEXP) {
            s = format!("^(?:{s})$");
        }
        s
    }

    /// Compiles multiple patterns into a single regex.
    ///
    /// # Errors
    ///
    /// Returns `Error::RegexBuild` if the combined pattern is invalid.
    pub fn compile(&self, patterns: &[&str]) -> Result<Regex, Error> {
        let mut branches: Vec<String> = patterns.iter().map(|p| self.shape(p)).collect();
        let combined = if branches.len() == 1 {
            branches.swap_remove(0)
        } else {
            branches
                .into_iter()
                .map(|b| format!("(?:{b})"))
                .collect::<Vec<_>>()
                .join("|")
        };
        let mut builder = Regex::builder();
        if self.case_insensitive {
            builder.syntax(regex_automata::util::syntax::Config::new().case_insensitive(true));
        }
        builder
            .build(&combined)
            .map_err(|e| Error::RegexBuild(format!("regex compilation failed: {e}")))
    }

    /// Compiles a single pattern into a regex.
    ///
    /// # Errors
    ///
    /// Returns `Error::RegexBuild` if the pattern is invalid.
    pub fn compile_one(&self, pattern: &str) -> Result<Regex, Error> {
        self.compile(&[pattern])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grep::options::{MatchFlags, MatchOptions, RegexEngineRequest};
    use crate::grep::{Indexability, IndexabilityReason};

    fn make_search(patterns: &[&str], opts: MatchOptions) -> Query {
        let patterns: Vec<String> = patterns.iter().map(ToString::to_string).collect();
        Query::new(patterns).expect("compile search").options(opts)
    }

    #[test]
    fn compiled_search_selects_indexability() {
        let mut opts = MatchOptions {
            input_encoding: crate::grep::options::InputEncoding::Raw,
            ..MatchOptions::default()
        };
        let search = make_search(&["needle"], opts.clone());
        assert_eq!(
            search.compile().unwrap().indexability(),
            Indexability::Indexed
        );

        opts.flags |= MatchFlags::INVERT_MATCH;
        let search = make_search(&["needle"], opts.clone());
        assert_eq!(
            search.compile().unwrap().indexability(),
            Indexability::Complete(IndexabilityReason::InvertedMatch)
        );

        opts.flags.remove(MatchFlags::INVERT_MATCH);
        opts.input_encoding = crate::grep::options::InputEncoding::Auto;
        let search = make_search(&["needle"], opts.clone());
        assert_eq!(
            search.compile().unwrap().indexability(),
            Indexability::Complete(IndexabilityReason::DecodedInput)
        );

        opts.input_encoding = crate::grep::options::InputEncoding::Raw;
        opts.regex_engine = RegexEngineRequest::Pcre2;
        let search = make_search(&["needle"], opts.clone());
        assert_eq!(
            search.compile().unwrap().indexability(),
            Indexability::Complete(IndexabilityReason::RegexEngineUnsupportedByPlanner)
        );
    }

    #[test]
    fn search_query_new_rejects_empty_patterns() {
        let result = Query::new(Vec::new());
        assert!(result.is_err());
    }

    #[test]
    fn alternation_matches_either_pattern() {
        let re = PatternCompiler::new().compile(&["foo", "bar"]).unwrap();
        let mut cache = regex_automata::meta::Cache::new(&re);
        assert!(
            re.search_with(&mut cache, &regex_automata::Input::new(b"foo"))
                .is_some()
        );
    }
}

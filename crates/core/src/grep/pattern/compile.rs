use grep_pcre2::{RegexMatcher as Pcre2Matcher, RegexMatcherBuilder as Pcre2MatcherBuilder};
use grep_regex::{RegexMatcher, RegexMatcherBuilder};

use crate::grep::Error;
use crate::grep::options::RegexEngineRequest;
use crate::grep::pattern::Query;

#[derive(Debug, Clone)]
pub enum CompiledQuery {
    Rust {
        matcher: RegexMatcher,
        indexability: Indexability,
    },
    Pcre2 {
        matcher: Pcre2Matcher,
        indexability: Indexability,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RegexEngine {
    Rust,
    Pcre2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Indexability {
    Indexed,
    Complete(IndexabilityReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexabilityReason {
    InvertedMatch,
    DecodedInput,
    RegexEngineUnsupportedByPlanner,
}

impl CompiledQuery {
    #[must_use]
    pub const fn indexability(&self) -> Indexability {
        match self {
            Self::Rust { indexability, .. } | Self::Pcre2 { indexability, .. } => *indexability,
        }
    }

    #[must_use]
    pub const fn index_capable(&self) -> bool {
        matches!(self.indexability(), Indexability::Indexed)
    }
}

impl Query {
    /// Compiles patterns into a matcher and records planner-facing query capabilities.
    ///
    /// # Errors
    ///
    /// Returns an error if pattern compilation fails.
    ///
    /// # Panics
    ///
    /// Panics if the compiled query cache is empty immediately after initialization.
    pub fn compile(&self) -> Result<&CompiledQuery, Error> {
        if let Some(compiled) = self.compiled.get() {
            return Ok(compiled);
        }
        let compiled = self.build_compiled_search()?;
        let _ = self.compiled.set(compiled);
        Ok(self.compiled.get().expect("just initialised"))
    }

    fn build_compiled_search(&self) -> Result<CompiledQuery, Error> {
        match self.opts.regex_engine {
            RegexEngineRequest::Rust => Ok(CompiledQuery::Rust {
                matcher: self.build_rust_matcher()?,
                indexability: self.indexability(RegexEngine::Rust),
            }),
            RegexEngineRequest::Pcre2 => Ok(CompiledQuery::Pcre2 {
                matcher: self.build_pcre2_matcher()?,
                indexability: self.indexability(RegexEngine::Pcre2),
            }),
            RegexEngineRequest::Auto => match self.build_rust_matcher() {
                Ok(matcher) => Ok(CompiledQuery::Rust {
                    matcher,
                    indexability: self.indexability(RegexEngine::Rust),
                }),
                Err(_) => Ok(CompiledQuery::Pcre2 {
                    matcher: self.build_pcre2_matcher()?,
                    indexability: self.indexability(RegexEngine::Pcre2),
                }),
            },
        }
    }

    fn indexability(&self, engine: RegexEngine) -> Indexability {
        if self.opts.invert_match() {
            Indexability::Complete(IndexabilityReason::InvertedMatch)
        } else if self.opts.input_encoding.uses_decoded_input() {
            Indexability::Complete(IndexabilityReason::DecodedInput)
        } else if engine != RegexEngine::Rust {
            Indexability::Complete(IndexabilityReason::RegexEngineUnsupportedByPlanner)
        } else {
            Indexability::Indexed
        }
    }

    fn build_rust_matcher(&self) -> Result<RegexMatcher, Error> {
        let mut builder = RegexMatcherBuilder::new();
        builder.multi_line(true);
        match self.opts.case_mode {
            crate::grep::options::CaseMode::Sensitive => {}
            crate::grep::options::CaseMode::Insensitive => {
                builder.case_insensitive(true);
            }
            crate::grep::options::CaseMode::Smart => {
                builder.case_smart(true);
            }
        }
        builder.unicode(self.opts.unicode);
        builder.fixed_strings(self.opts.fixed_strings());
        if self.opts.word_regexp() {
            builder.word(true);
        }
        if self.opts.line_regexp() {
            builder.whole_line(true);
        }
        if self.opts.regex_size_limit > 0 {
            builder.size_limit(self.opts.regex_size_limit);
        }
        if self.opts.dfa_size_limit > 0 {
            builder.dfa_size_limit(self.opts.dfa_size_limit);
        }
        if self.opts.crlf() {
            builder.crlf(true);
        }
        if self.opts.multiline() {
            if self.opts.multiline_dotall() {
                builder.dot_matches_new_line(true);
            }
        } else {
            builder.line_terminator(Some(self.opts.line_terminator()));
        }
        builder.ban_byte(None);
        builder
            .build_many(&self.patterns)
            .map_err(|e| Error::RegexBuild(e.to_string()))
    }

    fn build_pcre2_matcher(&self) -> Result<Pcre2Matcher, Error> {
        let mut builder = Pcre2MatcherBuilder::new();
        builder.multi_line(true);
        match self.opts.case_mode {
            crate::grep::options::CaseMode::Sensitive => {}
            crate::grep::options::CaseMode::Insensitive => {
                builder.caseless(true);
            }
            crate::grep::options::CaseMode::Smart => {
                builder.case_smart(true);
            }
        }
        builder.utf(self.opts.unicode);
        builder.ucp(self.opts.unicode);
        builder.fixed_strings(self.opts.fixed_strings());
        if self.opts.word_regexp() {
            builder.word(true);
        }
        if self.opts.line_regexp() {
            builder.whole_line(true);
        }
        if self.opts.crlf() {
            builder.crlf(true);
        }
        if self.opts.multiline() && self.opts.multiline_dotall() {
            builder.dotall(true);
        }
        builder
            .build_many(&self.patterns)
            .map_err(|e| Error::RegexBuild(e.to_string()))
    }
}

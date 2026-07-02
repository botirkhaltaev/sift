use grep_pcre2::{RegexMatcher as Pcre2Matcher, RegexMatcherBuilder as Pcre2MatcherBuilder};
use grep_regex::{RegexMatcher, RegexMatcherBuilder};

use crate::grep::Error;
use crate::grep::options::{CaseMode, MatchOptions, RegexEngineRequest};

#[derive(Debug, Clone)]
pub enum CompiledQuery {
    Rust {
        matcher: RegexMatcher,
        index_use: IndexUse,
    },
    Pcre2 {
        matcher: Pcre2Matcher,
        index_use: IndexUse,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexUse {
    Narrow,
    CompleteScan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RegexEngine {
    Rust,
    Pcre2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompleteScanReason {
    InvertedMatch,
    DecodedInput,
    RegexEngineUnsupportedByPlanner,
}

pub struct QueryCompiler<'a> {
    patterns: &'a [String],
    opts: &'a MatchOptions,
}

impl CompiledQuery {
    #[must_use]
    pub const fn index_use(&self) -> IndexUse {
        match self {
            Self::Rust { index_use, .. } | Self::Pcre2 { index_use, .. } => *index_use,
        }
    }
}

impl<'a> QueryCompiler<'a> {
    pub const fn new(patterns: &'a [String], opts: &'a MatchOptions) -> Self {
        Self { patterns, opts }
    }

    pub fn compile(&self) -> Result<CompiledQuery, Error> {
        match self.opts.regex_engine {
            RegexEngineRequest::Rust => Ok(CompiledQuery::Rust {
                matcher: self.rust()?,
                index_use: self.index_use(RegexEngine::Rust),
            }),
            RegexEngineRequest::Pcre2 => Ok(CompiledQuery::Pcre2 {
                matcher: self.pcre2()?,
                index_use: self.index_use(RegexEngine::Pcre2),
            }),
            RegexEngineRequest::Auto => match self.rust() {
                Ok(matcher) => Ok(CompiledQuery::Rust {
                    matcher,
                    index_use: self.index_use(RegexEngine::Rust),
                }),
                Err(_) => Ok(CompiledQuery::Pcre2 {
                    matcher: self.pcre2()?,
                    index_use: self.index_use(RegexEngine::Pcre2),
                }),
            },
        }
    }

    fn index_use(&self, engine: RegexEngine) -> IndexUse {
        match self.complete_scan_reason(engine) {
            Some(_) => IndexUse::CompleteScan,
            None => IndexUse::Narrow,
        }
    }

    fn complete_scan_reason(&self, engine: RegexEngine) -> Option<CompleteScanReason> {
        if self.opts.invert_match() {
            Some(CompleteScanReason::InvertedMatch)
        } else if self.opts.input_encoding.uses_decoded_input() {
            Some(CompleteScanReason::DecodedInput)
        } else if engine != RegexEngine::Rust {
            Some(CompleteScanReason::RegexEngineUnsupportedByPlanner)
        } else {
            None
        }
    }

    fn rust(&self) -> Result<RegexMatcher, Error> {
        let mut builder = RegexMatcherBuilder::new();
        builder.multi_line(true);
        match self.opts.case_mode {
            CaseMode::Sensitive => {}
            CaseMode::Insensitive => {
                builder.case_insensitive(true);
            }
            CaseMode::Smart => {
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
            .build_many(self.patterns)
            .map_err(|e| Error::RegexBuild(e.to_string()))
    }

    fn pcre2(&self) -> Result<Pcre2Matcher, Error> {
        let mut builder = Pcre2MatcherBuilder::new();
        builder.multi_line(true);
        match self.opts.case_mode {
            CaseMode::Sensitive => {}
            CaseMode::Insensitive => {
                builder.caseless(true);
            }
            CaseMode::Smart => {
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
            .build_many(self.patterns)
            .map_err(|e| Error::RegexBuild(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grep::Query;
    use crate::grep::options::{InputEncoding, MatchFlags, RegexEngineRequest};

    fn make_query(patterns: &[&str], opts: MatchOptions) -> Query {
        let patterns = patterns.iter().map(ToString::to_string).collect();
        Query::new(patterns).expect("compile query").options(opts)
    }

    #[test]
    fn compiled_query_uses_index_for_raw_rust_regex() {
        let opts = MatchOptions {
            input_encoding: InputEncoding::Raw,
            ..MatchOptions::default()
        };

        let query = make_query(&["needle"], opts);

        assert_eq!(query.compile().unwrap().index_use(), IndexUse::Narrow);
    }

    #[test]
    fn compiled_query_requires_complete_scan_for_inverted_match() {
        let mut opts = MatchOptions {
            input_encoding: InputEncoding::Raw,
            ..MatchOptions::default()
        };
        opts.flags |= MatchFlags::INVERT_MATCH;

        let query = make_query(&["needle"], opts);

        assert_eq!(query.compile().unwrap().index_use(), IndexUse::CompleteScan);
    }

    #[test]
    fn compiled_query_requires_complete_scan_for_decoded_input() {
        let opts = MatchOptions {
            input_encoding: InputEncoding::Auto,
            ..MatchOptions::default()
        };

        let query = make_query(&["needle"], opts);

        assert_eq!(query.compile().unwrap().index_use(), IndexUse::CompleteScan);
    }

    #[test]
    fn compiled_query_requires_complete_scan_for_pcre2() {
        let opts = MatchOptions {
            input_encoding: InputEncoding::Raw,
            regex_engine: RegexEngineRequest::Pcre2,
            ..MatchOptions::default()
        };

        let query = make_query(&["needle"], opts);

        assert_eq!(query.compile().unwrap().index_use(), IndexUse::CompleteScan);
    }
}

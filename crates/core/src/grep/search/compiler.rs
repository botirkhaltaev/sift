use grep_pcre2::{RegexMatcher as Pcre2Matcher, RegexMatcherBuilder as Pcre2MatcherBuilder};
use grep_regex::{RegexMatcher, RegexMatcherBuilder};

use crate::grep::Error;
use crate::grep::options::{CaseMode, MatchOptions, RegexEngineRequest};
use crate::query::IndexNarrowing;

use super::CompiledQuery;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RegexEngine {
    Rust,
    Pcre2,
}

pub(super) struct QueryCompiler<'a> {
    patterns: &'a [String],
    opts: &'a MatchOptions,
}

impl<'a> QueryCompiler<'a> {
    pub(super) const fn new(patterns: &'a [String], opts: &'a MatchOptions) -> Self {
        Self { patterns, opts }
    }

    pub(super) fn compile(&self) -> Result<CompiledQuery, Error> {
        match self.opts.regex_engine {
            RegexEngineRequest::Rust => Ok(CompiledQuery::Rust {
                matcher: self.rust()?,
                index_narrowing: self.index_narrowing(RegexEngine::Rust),
            }),
            RegexEngineRequest::Pcre2 => Ok(CompiledQuery::Pcre2 {
                matcher: self.pcre2()?,
                index_narrowing: self.index_narrowing(RegexEngine::Pcre2),
            }),
            RegexEngineRequest::Auto => match self.rust() {
                Ok(matcher) => Ok(CompiledQuery::Rust {
                    matcher,
                    index_narrowing: self.index_narrowing(RegexEngine::Rust),
                }),
                Err(_) => Ok(CompiledQuery::Pcre2 {
                    matcher: self.pcre2()?,
                    index_narrowing: self.index_narrowing(RegexEngine::Pcre2),
                }),
            },
        }
    }

    fn index_narrowing(&self, engine: RegexEngine) -> IndexNarrowing {
        if self.opts.invert_match()
            || self.opts.input_encoding.uses_decoded_input()
            || engine != RegexEngine::Rust
        {
            IndexNarrowing::Disabled
        } else {
            IndexNarrowing::Enabled
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

        assert_eq!(
            query.compile().unwrap().index_narrowing(),
            IndexNarrowing::Enabled
        );
    }

    #[test]
    fn compiled_query_requires_complete_scan_for_inverted_match() {
        let mut opts = MatchOptions {
            input_encoding: InputEncoding::Raw,
            ..MatchOptions::default()
        };
        opts.flags |= MatchFlags::INVERT_MATCH;

        let query = make_query(&["needle"], opts);

        assert_eq!(
            query.compile().unwrap().index_narrowing(),
            IndexNarrowing::Disabled
        );
    }

    #[test]
    fn compiled_query_requires_complete_scan_for_decoded_input() {
        let opts = MatchOptions {
            input_encoding: InputEncoding::Auto,
            ..MatchOptions::default()
        };

        let query = make_query(&["needle"], opts);

        assert_eq!(
            query.compile().unwrap().index_narrowing(),
            IndexNarrowing::Disabled
        );
    }

    #[test]
    fn compiled_query_requires_complete_scan_for_pcre2() {
        let opts = MatchOptions {
            input_encoding: InputEncoding::Raw,
            regex_engine: RegexEngineRequest::Pcre2,
            ..MatchOptions::default()
        };

        let query = make_query(&["needle"], opts);

        assert_eq!(
            query.compile().unwrap().index_narrowing(),
            IndexNarrowing::Disabled
        );
    }
}

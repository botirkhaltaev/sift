use grep_matcher::{
    Captures as GrepMatcherCaptures, LineTerminator, Match, Matcher as GrepMatcherTrait,
};
use grep_pcre2::{RegexMatcher as Pcre2Matcher, RegexMatcherBuilder as Pcre2MatcherBuilder};
use grep_regex::{RegexCaptures, RegexMatcher, RegexMatcherBuilder};
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder};

use crate::grep::Error;
use crate::grep::options::BinaryMode;
use crate::grep::pattern::Query;

#[derive(Clone, Debug)]
pub enum Matcher {
    Rust(RegexMatcher),
    Pcre2(Pcre2Matcher),
}

#[derive(Clone, Debug)]
pub enum MatchCaptures {
    Rust(RegexCaptures),
    Pcre2(grep_pcre2::RegexCaptures),
}

impl GrepMatcherCaptures for MatchCaptures {
    fn len(&self) -> usize {
        match self {
            Self::Rust(captures) => captures.len(),
            Self::Pcre2(captures) => captures.len(),
        }
    }

    fn get(&self, index: usize) -> Option<Match> {
        match self {
            Self::Rust(captures) => captures.get(index),
            Self::Pcre2(captures) => captures.get(index),
        }
    }
}

impl GrepMatcherTrait for Matcher {
    type Captures = MatchCaptures;
    type Error = String;

    fn find_at(&self, haystack: &[u8], at: usize) -> Result<Option<Match>, Self::Error> {
        match self {
            Self::Rust(matcher) => matcher.find_at(haystack, at).map_err(|e| e.to_string()),
            Self::Pcre2(matcher) => matcher.find_at(haystack, at).map_err(|e| e.to_string()),
        }
    }

    fn new_captures(&self) -> Result<Self::Captures, Self::Error> {
        match self {
            Self::Rust(matcher) => matcher
                .new_captures()
                .map(MatchCaptures::Rust)
                .map_err(|e| e.to_string()),
            Self::Pcre2(matcher) => matcher
                .new_captures()
                .map(MatchCaptures::Pcre2)
                .map_err(|e| e.to_string()),
        }
    }

    fn capture_count(&self) -> usize {
        match self {
            Self::Rust(matcher) => matcher.capture_count(),
            Self::Pcre2(matcher) => matcher.capture_count(),
        }
    }

    fn capture_index(&self, name: &str) -> Option<usize> {
        match self {
            Self::Rust(matcher) => matcher.capture_index(name),
            Self::Pcre2(matcher) => matcher.capture_index(name),
        }
    }

    fn captures_at(
        &self,
        haystack: &[u8],
        at: usize,
        captures: &mut Self::Captures,
    ) -> Result<bool, Self::Error> {
        match (self, captures) {
            (Self::Rust(matcher), MatchCaptures::Rust(captures)) => matcher
                .captures_at(haystack, at, captures)
                .map_err(|e| e.to_string()),
            (Self::Pcre2(matcher), MatchCaptures::Pcre2(captures)) => matcher
                .captures_at(haystack, at, captures)
                .map_err(|e| e.to_string()),
            _ => Err("capture storage does not match regex engine".to_string()),
        }
    }
}

/// Configuration for building a ripgrep `Searcher`.
#[derive(Debug, Clone, Copy)]
pub struct SearcherConfig {
    pub line_numbers: bool,
    pub max_matches: Option<usize>,
    pub include_context: bool,
}

impl SearcherConfig {
    #[must_use]
    pub const fn match_collection(max_matches: Option<usize>) -> Self {
        Self {
            line_numbers: true,
            max_matches,
            include_context: false,
        }
    }

    #[must_use]
    pub fn searcher(self, query: &Query) -> Searcher {
        let (before_context, after_context) = if self.include_context {
            (query.opts.before_context, query.opts.after_context)
        } else {
            (0, 0)
        };
        let line_number = self.line_numbers || before_context > 0 || after_context > 0;
        let mut builder = SearcherBuilder::new();
        let binary_detection = if query.opts.null_data() {
            BinaryDetection::none()
        } else {
            match query.opts.binary_mode {
                BinaryMode::Quit => BinaryDetection::quit(b'\x00'),
                BinaryMode::Binary => BinaryDetection::convert(b'\x00'),
                BinaryMode::AsText => BinaryDetection::none(),
            }
        };
        builder
            .encoding(query.opts.input_encoding.explicit())
            .bom_sniffing(query.opts.input_encoding.bom_sniffing())
            .binary_detection(binary_detection)
            .line_terminator(LineTerminator::byte(query.opts.line_terminator()))
            .invert_match(query.opts.invert_match())
            .line_number(line_number)
            .before_context(before_context)
            .after_context(after_context)
            .max_matches(self.max_matches.map(|n| n as u64));
        if query.opts.multiline() {
            builder.multi_line(true);
        }
        builder.build()
    }
}

impl Query {
    pub(crate) fn build_rust_matcher(&self) -> Result<Matcher, Error> {
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
        if self.opts.multiline() || self.opts.null_data() {
            builder.ban_byte(None);
        } else {
            match self.opts.binary_mode {
                BinaryMode::AsText => {
                    builder.ban_byte(None);
                }
                _ => {
                    builder.ban_byte(Some(b'\x00'));
                }
            }
        }
        builder
            .build_many(&self.patterns)
            .map(Matcher::Rust)
            .map_err(|e| Error::RegexBuild(e.to_string()))
    }

    pub(crate) fn build_pcre2_matcher(&self) -> Result<Matcher, Error> {
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
            .map(Matcher::Pcre2)
            .map_err(|e| Error::RegexBuild(e.to_string()))
    }
}

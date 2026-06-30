use grep_matcher::{Captures, LineTerminator, Match, Matcher};
use grep_pcre2::{RegexMatcher as Pcre2Matcher, RegexMatcherBuilder as Pcre2MatcherBuilder};
use grep_regex::{RegexCaptures, RegexMatcher, RegexMatcherBuilder};
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder};

use super::GrepQuery;
use crate::grep::GrepError;
use crate::grep::options::BinaryMode;

#[derive(Clone, Debug)]
pub enum GrepMatcher {
    Rust(RegexMatcher),
    Pcre2(Pcre2Matcher),
}

#[derive(Clone, Debug)]
pub enum GrepCaptures {
    Rust(RegexCaptures),
    Pcre2(grep_pcre2::RegexCaptures),
}

impl Captures for GrepCaptures {
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

impl Matcher for GrepMatcher {
    type Captures = GrepCaptures;
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
                .map(GrepCaptures::Rust)
                .map_err(|e| e.to_string()),
            Self::Pcre2(matcher) => matcher
                .new_captures()
                .map(GrepCaptures::Pcre2)
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
            (Self::Rust(matcher), GrepCaptures::Rust(captures)) => matcher
                .captures_at(haystack, at, captures)
                .map_err(|e| e.to_string()),
            (Self::Pcre2(matcher), GrepCaptures::Pcre2(captures)) => matcher
                .captures_at(haystack, at, captures)
                .map_err(|e| e.to_string()),
            _ => Err("capture storage does not match regex engine".to_string()),
        }
    }
}

impl GrepQuery {
    pub(crate) fn build_rust_matcher(&self) -> Result<GrepMatcher, GrepError> {
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
            .map(GrepMatcher::Rust)
            .map_err(|e| GrepError::RegexBuild(e.to_string()))
    }

    pub(crate) fn build_pcre2_matcher(&self) -> Result<GrepMatcher, GrepError> {
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
            .map(GrepMatcher::Pcre2)
            .map_err(|e| GrepError::RegexBuild(e.to_string()))
    }

    pub fn build_searcher(
        &self,
        line_number: bool,
        max_matches: Option<usize>,
        include_context: bool,
    ) -> Searcher {
        let (before_context, after_context) = if include_context {
            (self.opts.before_context, self.opts.after_context)
        } else {
            (0, 0)
        };
        let line_number = line_number || before_context > 0 || after_context > 0;
        let mut builder = SearcherBuilder::new();
        let binary_detection = if self.opts.null_data() {
            BinaryDetection::none()
        } else {
            match self.opts.binary_mode {
                BinaryMode::Quit => BinaryDetection::quit(b'\x00'),
                BinaryMode::Binary => BinaryDetection::convert(b'\x00'),
                BinaryMode::AsText => BinaryDetection::none(),
            }
        };
        builder
            .encoding(self.opts.input_encoding.explicit())
            .bom_sniffing(self.opts.input_encoding.bom_sniffing())
            .binary_detection(binary_detection)
            .line_terminator(LineTerminator::byte(self.opts.line_terminator()))
            .invert_match(self.opts.invert_match())
            .line_number(line_number)
            .before_context(before_context)
            .after_context(after_context)
            .max_matches(max_matches.map(|n| n as u64));
        if self.opts.multiline() {
            builder.multi_line(true);
        }
        builder.build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grep::options::{GrepMatchFlags, GrepOptions};

    fn make_search(patterns: &[&str], opts: GrepOptions) -> GrepQuery {
        let patterns: Vec<String> = patterns.iter().map(ToString::to_string).collect();
        GrepQuery::new(patterns)
            .expect("compile search")
            .options(opts)
    }

    struct CollectStringSink {
        hits: Vec<String>,
    }

    impl grep_searcher::Sink for CollectStringSink {
        type Error = std::io::Error;

        fn matched(
            &mut self,
            searcher: &grep_searcher::Searcher,
            mat: &grep_searcher::SinkMatch<'_>,
        ) -> Result<bool, Self::Error> {
            std::hint::black_box(searcher);
            self.hits
                .push(String::from_utf8_lossy(mat.bytes()).into_owned());
            Ok(true)
        }
    }

    fn search_content(search: &GrepQuery, content: &[u8]) -> Vec<String> {
        let matcher = search.compile().expect("compile search").matcher();
        let mut sink = CollectStringSink { hits: Vec::new() };
        let mut searcher = search.build_searcher(true, None, true);
        let _ = searcher.search_slice(matcher, content, &mut sink);
        sink.hits
    }

    #[test]
    fn sensitive_mode_matches_exact_case_only() {
        use crate::grep::options::CaseMode;
        let opts = GrepOptions {
            case_mode: CaseMode::Sensitive,
            ..GrepOptions::default()
        };
        let search = make_search(&["Hello"], opts);
        let hits = search_content(&search, b"Hello world\nhello world\nHELLO world\n");
        assert_eq!(hits, vec!["Hello world\n"]);
    }

    #[test]
    fn insensitive_mode_matches_case_variants() {
        use crate::grep::options::CaseMode;
        let opts = GrepOptions {
            case_mode: CaseMode::Insensitive,
            ..GrepOptions::default()
        };
        let search = make_search(&["hello"], opts);
        let hits = search_content(&search, b"Hello world\nhello world\nHELLO world\n");
        assert_eq!(hits.len(), 3);
    }

    #[test]
    fn fixed_strings_treat_metacharacters_literally() {
        let mut opts = GrepOptions::default();
        opts.flags |= GrepMatchFlags::FIXED_STRINGS;
        let search = make_search(&["a.c"], opts);
        let hits = search_content(&search, b"a.c\nabc\naXc\n");
        assert_eq!(hits, vec!["a.c\n"]);
    }

    #[test]
    fn word_regexp_rejects_embedded_matches() {
        let mut opts = GrepOptions::default();
        opts.flags |= GrepMatchFlags::WORD_REGEXP;
        let search = make_search(&["cat"], opts);
        let hits = search_content(&search, b"a cat here\nconcatenate\n");
        assert_eq!(hits, vec!["a cat here\n"]);
    }

    #[test]
    fn line_regexp_rejects_partial_line_matches() {
        let mut opts = GrepOptions::default();
        opts.flags |= GrepMatchFlags::LINE_REGEXP;
        let search = make_search(&["yes"], opts);
        let hits = search_content(&search, b"yes\noh yes sir\n");
        assert_eq!(hits, vec!["yes\n"]);
    }

    #[test]
    fn invalid_regex_returns_search_error() {
        let search = make_search(&["("], GrepOptions::default());
        let result = search.compile();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), GrepError::RegexBuild(_)));
    }

    #[test]
    fn binary_mode_as_text_builds_without_error() {
        let opts = GrepOptions {
            binary_mode: BinaryMode::AsText,
            ..GrepOptions::default()
        };
        let search = make_search(&["hello"], opts);
        let result = search.compile();
        assert!(result.is_ok());
    }

    #[test]
    fn binary_mode_quit_builds_without_error() {
        let opts = GrepOptions {
            binary_mode: BinaryMode::Quit,
            ..GrepOptions::default()
        };
        let search = make_search(&["hello"], opts);
        let result = search.compile();
        assert!(result.is_ok());
    }

    #[test]
    fn multiline_mode_builds_without_error() {
        let mut opts = GrepOptions::default();
        opts.flags |= GrepMatchFlags::MULTILINE;
        let search = make_search(&["hello"], opts);
        let result = search.compile();
        assert!(result.is_ok());
    }

    #[test]
    fn crlf_mode_builds_without_error() {
        let mut opts = GrepOptions::default();
        opts.flags |= GrepMatchFlags::CRLF;
        let search = make_search(&["hello"], opts);
        let result = search.compile();
        assert!(result.is_ok());
    }
}

use grep_matcher::LineTerminator;
use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder};

use super::SearchQuery;
use crate::search::SearchError;
use crate::search::options::BinaryMode;

impl SearchQuery {
    /// Builds a regex matcher from the compiled patterns and options.
    ///
    /// # Errors
    ///
    /// Returns `SearchError::RegexBuild` if pattern compilation fails.
    pub fn build_matcher(&self) -> Result<RegexMatcher, SearchError> {
        let mut builder = RegexMatcherBuilder::new();
        builder.multi_line(true);
        match self.opts.case_mode {
            crate::search::options::CaseMode::Sensitive => {}
            crate::search::options::CaseMode::Insensitive => {
                builder.case_insensitive(true);
            }
            crate::search::options::CaseMode::Smart => {
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
            .map_err(|e| SearchError::RegexBuild(e.to_string()))
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
                BinaryMode::SearchBinary => BinaryDetection::convert(b'\x00'),
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
    use crate::search::options::{SearchMatchFlags, SearchOptions};

    fn make_search(patterns: &[&str], opts: SearchOptions) -> SearchQuery {
        let patterns: Vec<String> = patterns.iter().map(ToString::to_string).collect();
        SearchQuery::new(&patterns, opts).expect("compile search")
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

    fn search_content(search: &SearchQuery, content: &[u8]) -> Vec<String> {
        let matcher = search.build_matcher().expect("build matcher");
        let mut sink = CollectStringSink { hits: Vec::new() };
        let mut searcher = search.build_searcher(true, None, true);
        let _ = searcher.search_slice(&matcher, content, &mut sink);
        sink.hits
    }

    #[test]
    fn sensitive_mode_matches_exact_case_only() {
        use crate::search::options::CaseMode;
        let opts = SearchOptions {
            case_mode: CaseMode::Sensitive,
            ..SearchOptions::default()
        };
        let search = make_search(&["Hello"], opts);
        let hits = search_content(&search, b"Hello world\nhello world\nHELLO world\n");
        assert_eq!(hits, vec!["Hello world\n"]);
    }

    #[test]
    fn insensitive_mode_matches_case_variants() {
        use crate::search::options::CaseMode;
        let opts = SearchOptions {
            case_mode: CaseMode::Insensitive,
            ..SearchOptions::default()
        };
        let search = make_search(&["hello"], opts);
        let hits = search_content(&search, b"Hello world\nhello world\nHELLO world\n");
        assert_eq!(hits.len(), 3);
    }

    #[test]
    fn fixed_strings_treat_metacharacters_literally() {
        let mut opts = SearchOptions::default();
        opts.flags |= SearchMatchFlags::FIXED_STRINGS;
        let search = make_search(&["a.c"], opts);
        let hits = search_content(&search, b"a.c\nabc\naXc\n");
        assert_eq!(hits, vec!["a.c\n"]);
    }

    #[test]
    fn word_regexp_rejects_embedded_matches() {
        let mut opts = SearchOptions::default();
        opts.flags |= SearchMatchFlags::WORD_REGEXP;
        let search = make_search(&["cat"], opts);
        let hits = search_content(&search, b"a cat here\nconcatenate\n");
        assert_eq!(hits, vec!["a cat here\n"]);
    }

    #[test]
    fn line_regexp_rejects_partial_line_matches() {
        let mut opts = SearchOptions::default();
        opts.flags |= SearchMatchFlags::LINE_REGEXP;
        let search = make_search(&["yes"], opts);
        let hits = search_content(&search, b"yes\noh yes sir\n");
        assert_eq!(hits, vec!["yes\n"]);
    }

    #[test]
    fn invalid_regex_returns_search_error() {
        let search = make_search(&["("], SearchOptions::default());
        let result = search.build_matcher();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SearchError::RegexBuild(_)));
    }

    #[test]
    fn binary_mode_as_text_builds_without_error() {
        let opts = SearchOptions {
            binary_mode: BinaryMode::AsText,
            ..SearchOptions::default()
        };
        let search = make_search(&["hello"], opts);
        let result = search.build_matcher();
        assert!(result.is_ok());
    }

    #[test]
    fn binary_mode_quit_builds_without_error() {
        let opts = SearchOptions {
            binary_mode: BinaryMode::Quit,
            ..SearchOptions::default()
        };
        let search = make_search(&["hello"], opts);
        let result = search.build_matcher();
        assert!(result.is_ok());
    }

    #[test]
    fn multiline_mode_builds_without_error() {
        let mut opts = SearchOptions::default();
        opts.flags |= SearchMatchFlags::MULTILINE;
        let search = make_search(&["hello"], opts);
        let result = search.build_matcher();
        assert!(result.is_ok());
    }

    #[test]
    fn crlf_mode_builds_without_error() {
        let mut opts = SearchOptions::default();
        opts.flags |= SearchMatchFlags::CRLF;
        let search = make_search(&["hello"], opts);
        let result = search.build_matcher();
        assert!(result.is_ok());
    }
}

use grep_matcher::LineTerminator;
use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder};

use super::{BinaryMode, CaseMode, CompiledSearch};

impl CompiledSearch {
    /// # Errors
    /// Returns an error if pattern compilation fails.
    pub fn build_matcher(&self) -> crate::Result<RegexMatcher> {
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
        builder.fixed_strings(self.opts.fixed_strings());
        if self.opts.word_regexp() {
            builder.word(true);
        }
        if self.opts.line_regexp() {
            builder.whole_line(true);
        }
        builder.line_terminator(Some(b'\n'));
        match self.opts.binary_mode {
            BinaryMode::AsText => {
                builder.ban_byte(None);
            }
            _ => {
                builder.ban_byte(Some(b'\x00'));
            }
        }
        builder
            .build_many(&self.patterns)
            .map_err(|e| crate::Error::RegexBuild(e.to_string()))
    }

    /// `include_context`: standard search uses configured `-A`/`-B`/`-C`; summary/count modes pass `false`.
    pub(super) fn build_searcher(
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
        let binary_detection = match self.opts.binary_mode {
            BinaryMode::Quit => BinaryDetection::quit(b'\x00'),
            BinaryMode::SearchBinary => BinaryDetection::convert(b'\x00'),
            BinaryMode::AsText => BinaryDetection::none(),
        };
        builder
            .binary_detection(binary_detection)
            .line_terminator(LineTerminator::byte(b'\n'))
            .invert_match(self.opts.invert_match())
            .line_number(line_number)
            .before_context(before_context)
            .after_context(after_context)
            .max_matches(max_matches.map(|n| n as u64));
        builder.build()
    }

    pub(crate) fn with_cached_searcher<R>(
        &self,
        line_number: bool,
        max_matches: Option<usize>,
        f: impl FnOnce(&mut Searcher) -> R,
    ) -> R {
        let key = (
            line_number,
            max_matches,
            self.opts.before_context,
            self.opts.after_context,
        );
        let mut inner = {
            let mut guard = self
                .searcher_cache
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let need_new = guard.as_ref().is_none_or(|(k, _)| *k != key);
            if need_new {
                *guard = Some((key, self.build_searcher(line_number, max_matches, true)));
            }
            guard.take().expect("searcher_cache populated above")
        };
        let out = f(&mut inner.1);
        let mut guard = self
            .searcher_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = Some(inner);
        out
    }
}

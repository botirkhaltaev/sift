bitflags::bitflags! {
    /// Flags modifying how a query is interpreted by the search engine and index layer.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct CandidateFlags: u8 {
        const FIXED_STRINGS    = 1 << 0;
        const CASE_INSENSITIVE = 1 << 1;
        const WORD_REGEXP      = 1 << 2;
        const LINE_REGEXP      = 1 << 3;
        const INVERT_MATCH     = 1 << 4;
        const DISABLE_INDEX_NARROWING = 1 << 5;
    }
}

use crate::search::{RegexEngine, SearchFlags, SearchQuery};

/// Index-agnostic description used to narrow candidate files.
#[derive(Debug, Clone, Copy)]
pub struct CandidateSpec<'a> {
    pub patterns: &'a [String],
    pub flags: CandidateFlags,
}

impl<'a> From<&'a SearchQuery> for CandidateSpec<'a> {
    fn from(query: &'a SearchQuery) -> Self {
        let mut flags = CandidateFlags::empty();
        if query.options.flags.contains(SearchFlags::FIXED_STRINGS) {
            flags |= CandidateFlags::FIXED_STRINGS;
        }
        if query.options.case_insensitive() {
            flags |= CandidateFlags::CASE_INSENSITIVE;
        }
        if query.options.flags.contains(SearchFlags::WORD_REGEXP) {
            flags |= CandidateFlags::WORD_REGEXP;
        }
        if query.options.flags.contains(SearchFlags::LINE_REGEXP) {
            flags |= CandidateFlags::LINE_REGEXP;
        }
        if query.options.flags.contains(SearchFlags::INVERT_MATCH) {
            flags |= CandidateFlags::INVERT_MATCH;
        }
        if query.options.input_encoding.uses_decoded_input()
            || matches!(query.options.regex_engine, RegexEngine::Pcre2)
        {
            flags |= CandidateFlags::DISABLE_INDEX_NARROWING;
        }
        Self {
            patterns: &query.patterns,
            flags,
        }
    }
}

impl CandidateSpec<'_> {
    #[must_use]
    pub const fn fixed_strings(&self) -> bool {
        self.flags.contains(CandidateFlags::FIXED_STRINGS)
    }

    #[must_use]
    pub const fn case_insensitive(&self) -> bool {
        self.flags.contains(CandidateFlags::CASE_INSENSITIVE)
    }

    #[must_use]
    pub const fn word_regexp(&self) -> bool {
        self.flags.contains(CandidateFlags::WORD_REGEXP)
    }

    #[must_use]
    pub const fn line_regexp(&self) -> bool {
        self.flags.contains(CandidateFlags::LINE_REGEXP)
    }

    #[must_use]
    pub const fn invert_match(&self) -> bool {
        self.flags.contains(CandidateFlags::INVERT_MATCH)
    }

    #[must_use]
    pub(crate) const fn index_narrowing(&self) -> crate::candidates::IndexNarrowing {
        if self.invert_match() || self.flags.contains(CandidateFlags::DISABLE_INDEX_NARROWING) {
            crate::candidates::IndexNarrowing::Disabled
        } else {
            crate::candidates::IndexNarrowing::Enabled
        }
    }

    pub(crate) fn disable_index_narrowing(&mut self) {
        self.flags |= CandidateFlags::DISABLE_INDEX_NARROWING;
    }
}

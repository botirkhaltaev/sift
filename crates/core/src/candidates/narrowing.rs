bitflags::bitflags! {
    /// Flags modifying how a query is interpreted by the search engine and index layer.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub(crate) struct CandidateFlags: u8 {
        const FIXED_STRINGS    = 1 << 0;
        const CASE_INSENSITIVE = 1 << 1;
        const WORD_REGEXP      = 1 << 2;
        const LINE_REGEXP      = 1 << 3;
        const INVERT_MATCH     = 1 << 4;
        /// Default `InputEncoding::Auto`: BOM sniffing may decode rare UTF-16 files.
        /// Index narrowing stays on for ASCII arms, with UTF-16LE/BE arm expansion.
        const BOM_SNIFFING = 1 << 6;
    }
}

use crate::search::{InputEncoding, PrefilterCompatibility, RegexEngine, SearchFlags, SearchQuery};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IndexNarrowing {
    Enabled,
    Disabled,
}

/// Index-agnostic query projection used to narrow candidate files.
#[must_use]
#[derive(Debug, Clone, Copy)]
pub(crate) struct CandidateQuery<'q> {
    pub patterns: &'q [String],
    pub flags: CandidateFlags,
    index_narrowing: IndexNarrowing,
}

impl<'q> CandidateQuery<'q> {
    /// Build a candidate query from patterns and flags (in-crate unit tests).
    #[cfg(test)]
    pub(crate) const fn from_patterns(patterns: &'q [String], flags: CandidateFlags) -> Self {
        Self {
            patterns,
            flags,
            index_narrowing: IndexNarrowing::Enabled,
        }
    }

    pub(crate) fn new(query: &'q SearchQuery, prefilter: PrefilterCompatibility) -> Self {
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
        if matches!(query.options.input_encoding, InputEncoding::Auto) {
            flags |= CandidateFlags::BOM_SNIFFING;
        }
        let index_narrowing = if flags.contains(CandidateFlags::INVERT_MATCH)
            || query.options.input_encoding.forces_decode()
            || matches!(query.options.regex_engine, RegexEngine::Pcre2)
            || matches!(prefilter, PrefilterCompatibility::Incompatible)
        {
            IndexNarrowing::Disabled
        } else {
            IndexNarrowing::Enabled
        };
        Self {
            patterns: &query.patterns,
            flags,
            index_narrowing,
        }
    }

    #[must_use]
    pub(crate) const fn fixed_strings(&self) -> bool {
        self.flags.contains(CandidateFlags::FIXED_STRINGS)
    }

    #[must_use]
    pub(crate) const fn case_insensitive(&self) -> bool {
        self.flags.contains(CandidateFlags::CASE_INSENSITIVE)
    }

    #[must_use]
    pub(crate) const fn word_regexp(&self) -> bool {
        self.flags.contains(CandidateFlags::WORD_REGEXP)
    }

    #[must_use]
    pub(crate) const fn line_regexp(&self) -> bool {
        self.flags.contains(CandidateFlags::LINE_REGEXP)
    }

    #[must_use]
    pub(crate) const fn invert_match(&self) -> bool {
        self.flags.contains(CandidateFlags::INVERT_MATCH)
    }

    #[must_use]
    pub(crate) const fn bom_sniffing(&self) -> bool {
        self.flags.contains(CandidateFlags::BOM_SNIFFING)
    }

    #[must_use]
    pub(crate) const fn index_narrowing(&self) -> IndexNarrowing {
        self.index_narrowing
    }
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct QueryFlags: u8 {
        const FIXED_STRINGS    = 1 << 0;
        const CASE_INSENSITIVE = 1 << 1;
        const WORD_REGEXP      = 1 << 2;
        const LINE_REGEXP      = 1 << 3;
        const INVERT_MATCH     = 1 << 4;
    }
}

#[derive(Debug, Clone)]
pub struct QuerySpec<'a> {
    pub patterns: &'a [String],
    pub flags: QueryFlags,
}

impl QuerySpec<'_> {
    #[must_use]
    pub const fn fixed_strings(&self) -> bool {
        self.flags.contains(QueryFlags::FIXED_STRINGS)
    }

    #[must_use]
    pub const fn case_insensitive(&self) -> bool {
        self.flags.contains(QueryFlags::CASE_INSENSITIVE)
    }

    #[must_use]
    pub const fn word_regexp(&self) -> bool {
        self.flags.contains(QueryFlags::WORD_REGEXP)
    }

    #[must_use]
    pub const fn line_regexp(&self) -> bool {
        self.flags.contains(QueryFlags::LINE_REGEXP)
    }

    #[must_use]
    pub const fn invert_match(&self) -> bool {
        self.flags.contains(QueryFlags::INVERT_MATCH)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_flags_all_return_false() {
        let spec = QuerySpec {
            patterns: &[],
            flags: QueryFlags::empty(),
        };
        assert!(!spec.fixed_strings());
        assert!(!spec.case_insensitive());
        assert!(!spec.word_regexp());
        assert!(!spec.line_regexp());
        assert!(!spec.invert_match());
    }

    #[test]
    fn fixed_strings_flag_returns_true() {
        let spec = QuerySpec {
            patterns: &[],
            flags: QueryFlags::FIXED_STRINGS,
        };
        assert!(spec.fixed_strings());
    }

    #[test]
    fn case_insensitive_flag_returns_true() {
        let spec = QuerySpec {
            patterns: &[],
            flags: QueryFlags::CASE_INSENSITIVE,
        };
        assert!(spec.case_insensitive());
    }

    #[test]
    fn word_regexp_flag_returns_true() {
        let spec = QuerySpec {
            patterns: &[],
            flags: QueryFlags::WORD_REGEXP,
        };
        assert!(spec.word_regexp());
    }

    #[test]
    fn line_regexp_flag_returns_true() {
        let spec = QuerySpec {
            patterns: &[],
            flags: QueryFlags::LINE_REGEXP,
        };
        assert!(spec.line_regexp());
    }

    #[test]
    fn invert_match_flag_returns_true() {
        let spec = QuerySpec {
            patterns: &[],
            flags: QueryFlags::INVERT_MATCH,
        };
        assert!(spec.invert_match());
    }

    #[test]
    fn multiple_flags_all_return_true() {
        let mut flags = QueryFlags::empty();
        flags |= QueryFlags::FIXED_STRINGS;
        flags |= QueryFlags::CASE_INSENSITIVE;
        flags |= QueryFlags::WORD_REGEXP;
        let spec = QuerySpec {
            patterns: &["test".to_string()],
            flags,
        };
        assert!(spec.fixed_strings());
        assert!(spec.case_insensitive());
        assert!(spec.word_regexp());
        assert!(!spec.line_regexp());
        assert!(!spec.invert_match());
    }
}

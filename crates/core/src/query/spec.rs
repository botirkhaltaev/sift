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

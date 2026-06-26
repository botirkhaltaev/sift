use std::str::FromStr;

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct SearchMatchFlags: u16 {
        const INVERT_MATCH     = 1 << 0;
        const FIXED_STRINGS    = 1 << 1;
        const WORD_REGEXP      = 1 << 2;
        const LINE_REGEXP      = 1 << 3;
        const ONLY_MATCHING    = 1 << 4;
        const MULTILINE        = 1 << 5;
        const MULTILINE_DOTALL = 1 << 6;
        const CRLF             = 1 << 7;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RegexEngine {
    #[default]
    Default,
    Pcre2,
    Auto,
}

impl FromStr for RegexEngine {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "default" | "rust" => Ok(Self::Default),
            "pcre2" => Ok(Self::Pcre2),
            "auto" | "auto-hybrid" => Ok(Self::Auto),
            other => Err(format!(
                "unknown regex engine '{other}': expected default, pcre2, or auto"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CaseMode {
    #[default]
    Sensitive,
    Insensitive,
    Smart,
}

impl CaseMode {
    #[must_use]
    pub const fn is_case_insensitive(self) -> bool {
        matches!(self, Self::Insensitive)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BinaryMode {
    #[default]
    Quit,
    SearchBinary,
    AsText,
}

#[derive(Debug, Clone)]
pub struct SearchOptions {
    pub flags: SearchMatchFlags,
    pub case_mode: CaseMode,
    pub max_results: Option<usize>,
    pub before_context: usize,
    pub after_context: usize,
    pub binary_mode: BinaryMode,
    pub replace: Option<String>,
    pub unicode: bool,
    pub regex_size_limit: usize,
    pub dfa_size_limit: usize,
    pub regex_engine: RegexEngine,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            flags: SearchMatchFlags::default(),
            case_mode: CaseMode::default(),
            max_results: None,
            before_context: 0,
            after_context: 0,
            binary_mode: BinaryMode::default(),
            replace: None,
            unicode: true,
            regex_size_limit: 0,
            dfa_size_limit: 0,
            regex_engine: RegexEngine::default(),
        }
    }
}

impl SearchOptions {
    #[must_use]
    pub const fn case_insensitive(&self) -> bool {
        self.case_mode.is_case_insensitive()
    }

    #[must_use]
    pub const fn invert_match(&self) -> bool {
        self.flags.contains(SearchMatchFlags::INVERT_MATCH)
    }

    #[must_use]
    pub const fn fixed_strings(&self) -> bool {
        self.flags.contains(SearchMatchFlags::FIXED_STRINGS)
    }

    #[must_use]
    pub const fn word_regexp(&self) -> bool {
        self.flags.contains(SearchMatchFlags::WORD_REGEXP)
    }

    #[must_use]
    pub const fn line_regexp(&self) -> bool {
        self.flags.contains(SearchMatchFlags::LINE_REGEXP)
    }

    #[must_use]
    pub const fn only_matching(&self) -> bool {
        self.flags.contains(SearchMatchFlags::ONLY_MATCHING)
    }

    #[must_use]
    pub const fn multiline(&self) -> bool {
        self.flags.contains(SearchMatchFlags::MULTILINE)
    }

    #[must_use]
    pub const fn multiline_dotall(&self) -> bool {
        self.flags.contains(SearchMatchFlags::MULTILINE_DOTALL)
    }

    #[must_use]
    pub const fn crlf(&self) -> bool {
        self.flags.contains(SearchMatchFlags::CRLF)
    }

    #[must_use]
    pub const fn precludes_trigram_index(&self) -> bool {
        self.invert_match()
    }
}

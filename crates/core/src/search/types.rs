use std::path::PathBuf;
use std::sync::Mutex;

use grep_regex::RegexMatcher;
use grep_searcher::Searcher;
use once_cell::sync::OnceCell;

use crate::planner::TrigramPlan;

type SearcherCacheEntry = ((bool, Option<usize>, usize, usize), Searcher);

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

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct SearchMatchFlags: u8 {
        const INVERT_MATCH     = 1 << 0;
        const FIXED_STRINGS    = 1 << 1;
        const WORD_REGEXP      = 1 << 2;
        const LINE_REGEXP      = 1 << 3;
        const ONLY_MATCHING    = 1 << 4;
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SearchOptions {
    pub flags: SearchMatchFlags,
    pub case_mode: CaseMode,
    pub max_results: Option<usize>,
    /// Lines of context before each match (`-B` / leading part of `-C`).
    pub before_context: usize,
    /// Lines of context after each match (`-A` / trailing part of `-C`).
    pub after_context: usize,
}

impl SearchOptions {
    #[must_use]
    pub const fn case_insensitive(self) -> bool {
        self.case_mode.is_case_insensitive()
    }

    #[must_use]
    pub const fn invert_match(self) -> bool {
        self.flags.contains(SearchMatchFlags::INVERT_MATCH)
    }

    #[must_use]
    pub const fn fixed_strings(self) -> bool {
        self.flags.contains(SearchMatchFlags::FIXED_STRINGS)
    }

    #[must_use]
    pub const fn word_regexp(self) -> bool {
        self.flags.contains(SearchMatchFlags::WORD_REGEXP)
    }

    #[must_use]
    pub const fn line_regexp(self) -> bool {
        self.flags.contains(SearchMatchFlags::LINE_REGEXP)
    }

    #[must_use]
    pub const fn only_matching(self) -> bool {
        self.flags.contains(SearchMatchFlags::ONLY_MATCHING)
    }

    #[must_use]
    pub const fn precludes_trigram_index(self) -> bool {
        self.invert_match()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub file: PathBuf,
    pub line: usize,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchMode {
    #[default]
    Standard,
    OnlyMatching,
    Count,
    CountMatches,
    FilesWithMatches,
    FilesWithoutMatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputEmission {
    #[default]
    Normal,
    Quiet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FilenameMode {
    #[default]
    Auto,
    Always,
    Never,
}

/// When to emit ANSI colors (ripgrep-style `--color`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorChoice {
    /// Color only when stdout is a terminal.
    #[default]
    Auto,
    Never,
    Always,
}

/// Per-line presentation: paths, headings, and line numbers (standard / only-matching modes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchLineStyle {
    pub filename_mode: FilenameMode,
    pub heading: bool,
    pub line_number: bool,
}

impl Default for SearchLineStyle {
    fn default() -> Self {
        Self {
            filename_mode: FilenameMode::Auto,
            heading: false,
            line_number: false,
        }
    }
}

/// Path record terminators and color (`-0`, `--color`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchRecordStyle {
    /// `-0` / `--null`: end path-only records with NUL instead of newline.
    pub null_data: bool,
    pub color: ColorChoice,
}

impl Default for SearchRecordStyle {
    fn default() -> Self {
        Self {
            null_data: false,
            color: ColorChoice::Auto,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchOutput {
    pub mode: SearchMode,
    pub emission: OutputEmission,
    pub lines: SearchLineStyle,
    pub records: SearchRecordStyle,
}

impl Default for SearchOutput {
    fn default() -> Self {
        Self {
            mode: SearchMode::Standard,
            emission: OutputEmission::Normal,
            lines: SearchLineStyle::default(),
            records: SearchRecordStyle::default(),
        }
    }
}

/// Counters filled when running with [`CompiledSearch::run_index_with_stats`] /
/// [`CompiledSearch::run_walk_with_stats`].
///
/// `matches` is mode-dependent: line hits for standard / only-matching / count modes,
/// one per matching file for `-l`, and one per listed file for `--files-without-match`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SearchStats {
    /// Files searched after filtering (same length as the candidate list).
    pub files_searched: usize,
    /// Mode-dependent match tally (see struct docs).
    pub matches: usize,
}

#[derive(Debug)]
pub struct CompiledSearch {
    pub patterns: Vec<String>,
    pub opts: SearchOptions,
    pub plan: TrigramPlan,
    /// Lazily filled by [`Self::run_index`] via [`Self::build_matcher`]; repeated searches reuse one matcher.
    pub matcher: OnceCell<RegexMatcher>,
    /// Last [`Searcher`] built for `(line_number, max_matches, before_context, after_context)`; reused when the key matches.
    pub searcher_cache: Mutex<Option<SearcherCacheEntry>>,
}

impl CompiledSearch {
    /// Create a compiled search from patterns and options.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::EmptyPatterns`] when no patterns are provided.
    pub fn new(patterns: &[String], opts: SearchOptions) -> crate::Result<Self> {
        if patterns.is_empty() {
            return Err(crate::Error::EmptyPatterns);
        }
        let plan = TrigramPlan::for_patterns(patterns, &opts);
        Ok(Self {
            patterns: patterns.to_vec(),
            opts,
            plan,
            matcher: OnceCell::new(),
            searcher_cache: Mutex::new(None),
        })
    }

    #[must_use]
    pub fn patterns(&self) -> &[String] {
        &self.patterns
    }

    #[must_use]
    pub(crate) const fn uses_exhaustive_candidates(mode: SearchMode) -> bool {
        matches!(mode, SearchMode::Count | SearchMode::FilesWithoutMatch)
    }
}

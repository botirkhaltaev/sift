use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use grep_regex::RegexMatcher;
use grep_searcher::Searcher;
use once_cell::sync::OnceCell;

use super::error::SearchError;
use super::filter::SearchFilter;

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

/// Binary file handling mode (ripgrep's `-a`/`--text` and `--binary`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BinaryMode {
    /// Quit searching a file when a NUL byte is found (default).
    #[default]
    Quit,
    /// Continue searching binary files but convert NUL bytes (`--binary`).
    SearchBinary,
    /// Treat binary files as text; no NUL detection at all (`-a`/`--text`).
    AsText,
}

#[derive(Debug, Clone)]
pub struct SearchOptions {
    pub flags: SearchMatchFlags,
    pub case_mode: CaseMode,
    pub max_results: Option<usize>,
    /// Lines of context before each match (`-B` / leading part of `-C`).
    pub before_context: usize,
    /// Lines of context after each match (`-A` / trailing part of `-C`).
    pub after_context: usize,
    /// How to handle binary files.
    pub binary_mode: BinaryMode,
    /// Replacement string for `--replace`; `None` = no replacement.
    pub replace: Option<String>,
    /// Whether to enable Unicode mode in the regex engine (default: true).
    pub unicode: bool,
    /// Compiled regex size limit in bytes (0 = use engine default).
    pub regex_size_limit: usize,
    /// DFA size limit in bytes (0 = use engine default).
    pub dfa_size_limit: usize,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PathDisplay {
    #[default]
    Relative,
    Absolute,
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct LineStyleFlags: u8 {
        const HEADING     = 1 << 0;
        const LINE_NUMBER = 1 << 1;
        const BYTE_OFFSET = 1 << 2;
        const TRIM        = 1 << 3;
        const COLUMN      = 1 << 4;
    }
}

/// How to handle lines that exceed the column limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnOverflow {
    /// Silently omit lines that exceed the column limit.
    Omit,
    /// Show a truncated preview of lines that exceed the column limit.
    Preview,
}

/// Maximum column limit with overflow behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColumnLimit {
    /// Maximum number of columns per line.
    pub max: u64,
    /// How to display lines that exceed the limit.
    pub overflow: ColumnOverflow,
}

/// Per-line presentation: paths, headings, and line numbers (standard / only-matching modes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchLineStyle {
    pub filename_mode: FilenameMode,
    pub flags: LineStyleFlags,
    pub path_display: PathDisplay,
    /// Maximum columns per line (`-M`); `None` means no limit.
    pub columns: Option<ColumnLimit>,
}

impl SearchLineStyle {
    #[must_use]
    pub const fn heading(self) -> bool {
        self.flags.contains(LineStyleFlags::HEADING)
    }

    #[must_use]
    pub const fn line_number(self) -> bool {
        self.flags.contains(LineStyleFlags::LINE_NUMBER)
    }

    #[must_use]
    pub const fn byte_offset(self) -> bool {
        self.flags.contains(LineStyleFlags::BYTE_OFFSET)
    }

    #[must_use]
    pub const fn trim(self) -> bool {
        self.flags.contains(LineStyleFlags::TRIM)
    }
}

impl Default for SearchLineStyle {
    fn default() -> Self {
        Self {
            filename_mode: FilenameMode::Auto,
            flags: LineStyleFlags::empty(),
            path_display: PathDisplay::default(),
            columns: None,
        }
    }
}

/// Line terminator for path-only records (`-0` / `--null`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordTerminator {
    /// End records with a newline (default).
    Newline,
    /// End records with a NUL byte.
    Nul,
}

/// Path record terminators and color (`-0`, `--color`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchRecordStyle {
    /// How to terminate path-only records (`-0` / `--null`).
    pub terminator: RecordTerminator,
    pub color: ColorChoice,
    /// `--path-separator`: override the platform path separator in output.
    pub path_separator: Option<u8>,
}

impl Default for SearchRecordStyle {
    fn default() -> Self {
        Self {
            terminator: RecordTerminator::Newline,
            color: ColorChoice::Auto,
            path_separator: None,
        }
    }
}

/// Stdout encoding for search results (`--json` uses [JSON Lines](https://jsonlines.org/) like ripgrep).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchOutputFormat {
    /// Human-readable lines (default).
    #[default]
    Text,
    /// Machine-readable JSON Lines (`grep_printer::JSON`, ripgrep-compatible wire format).
    Json,
}

/// Whether to show every line including non-matching ones (`--passthru`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PassthruMode {
    #[default]
    Disabled,
    /// Show every line (non-matching lines as context).
    Enabled,
}

/// Whether to follow symbolic links when walking the filesystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkTraversal {
    /// Do not follow symbolic links.
    DoNotFollow,
    /// Follow symbolic links.
    Follow,
}

/// Grouped parameters for search execution, replacing long parameter lists
/// on [`CompiledSearch::run_indexes`] and [`CompiledSearch::run_walk`].
pub struct SearchExecution<'a> {
    /// Search-time filtering rules (glob, hidden, ignore, scope).
    pub filter: &'a SearchFilter,
    /// Output mode, line style, record style, and pass-through/zero settings.
    pub output: SearchOutput,
    /// Field and context separator configuration.
    pub separators: &'a SearchSeparators,
    /// Optional mutable stats reference filled during execution.
    pub stats: Option<&'a mut SearchStats>,
}

/// Options for walking the filesystem to discover files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalkOptions {
    /// How to handle symbolic links.
    pub links: LinkTraversal,
    /// Maximum directory depth for walk-based search.
    pub max_depth: Option<usize>,
    /// Maximum file size in bytes; files above this are skipped.
    pub max_filesize: Option<u64>,
    /// Whether to stay on the same filesystem when walking.
    pub one_file_system: bool,
}

impl Default for WalkOptions {
    fn default() -> Self {
        Self {
            links: LinkTraversal::DoNotFollow,
            max_depth: None,
            max_filesize: None,
            one_file_system: false,
        }
    }
}

/// How to emit matches from a per-file search sink.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchEmissionMode {
    /// Emit full matching lines (default).
    Lines,
    /// Emit only the matched portion of each line.
    OnlyMatching,
}

/// Whether to use indexed candidate narrowing or return all files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateSet {
    /// Use the index to narrow down candidate files for a query.
    IndexedCandidates,
    /// Return all indexed files (for modes that need a full scan).
    AllIndexedFiles,
}

/// Whether to include files with zero matches in count mode (`--include-zero`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ZeroCountMode {
    #[default]
    Omit,
    /// In count mode, print files with zero matches.
    Include,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchOutput {
    pub format: SearchOutputFormat,
    pub mode: SearchMode,
    pub emission: OutputEmission,
    pub lines: SearchLineStyle,
    pub records: SearchRecordStyle,
    /// `--passthru`: show every line (non-matching as context).
    pub passthru: PassthruMode,
    /// `--include-zero`: in count mode, print files with zero matches.
    pub include_zero: ZeroCountMode,
}

impl SearchOutput {
    /// Whether this output mode needs all indexed files rather than narrowed candidates.
    #[must_use]
    pub const fn candidate_set(self) -> CandidateSet {
        match self.mode {
            SearchMode::Count | SearchMode::FilesWithoutMatch => CandidateSet::AllIndexedFiles,
            SearchMode::Standard
            | SearchMode::OnlyMatching
            | SearchMode::CountMatches
            | SearchMode::FilesWithMatches => CandidateSet::IndexedCandidates,
        }
    }
}

impl Default for SearchOutput {
    fn default() -> Self {
        Self {
            format: SearchOutputFormat::Text,
            mode: SearchMode::Standard,
            emission: OutputEmission::Normal,
            lines: SearchLineStyle::default(),
            records: SearchRecordStyle::default(),
            passthru: PassthruMode::Disabled,
            include_zero: ZeroCountMode::Omit,
        }
    }
}

/// Configurable field and context-break separators (ripgrep `--context-separator`,
/// `--field-match-separator`, `--field-context-separator`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchSeparators {
    /// Printed between non-contiguous context groups (`--context-separator`).
    /// `None` suppresses the break line entirely (`--no-context-separator`).
    /// `Some(vec![])` prints just a newline (`--context-separator ""`).
    pub context_separator: Option<Vec<u8>>,
    /// Delimiter between path/line/col and the matched line (`--field-match-separator`).
    pub field_match_separator: Vec<u8>,
    /// Delimiter between path/line/col and a context line (`--field-context-separator`).
    pub field_context_separator: Vec<u8>,
}

impl Default for SearchSeparators {
    fn default() -> Self {
        Self {
            context_separator: Some(b"--".to_vec()),
            field_match_separator: b":".to_vec(),
            field_context_separator: b"-".to_vec(),
        }
    }
}

/// Counters filled when running with `stats: Some(&mut SearchStats)` on
/// [`CompiledSearch::run_indexes`] / [`CompiledSearch::run_walk`].
///
/// `matches` is mode-dependent: line hits for standard / only-matching / count modes,
/// one per matching file for `-l`, and one per listed file for `--files-without-match`.
///
/// `elapsed` covers wall time for the search stage (matcher build + scanning candidates), not
/// index open or filter prep.
///
/// `bytes_searched` is the sum of [`std::fs::Metadata::len`] for each candidate path (best-effort;
/// missing metadata counts as 0). This approximates ripgrep’s “bytes searched” for `--stats`.
///
/// `bytes_printed` counts bytes written to stdout (including separators between heading blocks).
///
/// `files_with_matches` follows ripgrep’s “files contained matches”: files that had a positive hit
/// for the current [`SearchMode`] (for `--files-without-match`, this stays 0 because listed paths are
/// non-matching files only).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SearchStats {
    /// Mode-dependent match tally (see struct docs).
    pub matches: usize,
    /// Files that contained at least one hit (mode-dependent; see struct docs).
    pub files_with_matches: usize,
    /// Files searched after filtering (same length as the candidate list).
    pub files_searched: usize,
    /// Bytes written to stdout for this search (best-effort).
    pub bytes_printed: u64,
    /// Sum of candidate file sizes from metadata (see struct docs).
    pub bytes_searched: u64,
    /// Wall-clock time spent in the search phase after candidates are ready.
    pub elapsed: Duration,
}

#[derive(Debug)]
pub struct CompiledSearch {
    pub patterns: Vec<String>,
    pub opts: SearchOptions,
    pub matcher: OnceCell<RegexMatcher>,
    pub searcher_cache: Mutex<Option<SearcherCacheEntry>>,
}

impl CompiledSearch {
    /// Create a compiled search from patterns and options.
    ///
    /// # Errors
    ///
    /// Returns [`SearchError::EmptyPatterns`] when no patterns are provided.
    pub fn new(patterns: &[String], opts: SearchOptions) -> Result<Self, SearchError> {
        if patterns.is_empty() {
            return Err(SearchError::EmptyPatterns);
        }
        Ok(Self {
            patterns: patterns.to_vec(),
            opts,
            matcher: OnceCell::new(),
            searcher_cache: Mutex::new(None),
        })
    }

    #[must_use]
    pub fn patterns(&self) -> &[String] {
        &self.patterns
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_mode_insensitive_returns_true() {
        assert!(CaseMode::Insensitive.is_case_insensitive());
    }

    #[test]
    fn case_mode_sensitive_returns_false() {
        assert!(!CaseMode::Sensitive.is_case_insensitive());
    }

    #[test]
    fn case_mode_smart_returns_false() {
        assert!(!CaseMode::Smart.is_case_insensitive());
    }

    #[test]
    fn search_options_defaults() {
        let opts = SearchOptions::default();
        assert!(!opts.case_insensitive());
        assert!(!opts.invert_match());
        assert!(!opts.fixed_strings());
        assert!(!opts.word_regexp());
        assert!(!opts.line_regexp());
        assert!(!opts.only_matching());
        assert!(!opts.multiline());
        assert!(!opts.multiline_dotall());
        assert!(!opts.crlf());
        assert!(!opts.precludes_trigram_index());
        assert_eq!(opts.max_results, None);
        assert_eq!(opts.before_context, 0);
        assert_eq!(opts.after_context, 0);
        assert_eq!(opts.binary_mode, BinaryMode::Quit);
        assert!(opts.unicode);
    }

    #[test]
    fn search_options_precludes_trigram_index_only_for_invert_match() {
        let mut opts = SearchOptions::default();
        assert!(!opts.precludes_trigram_index());

        opts.flags |= SearchMatchFlags::INVERT_MATCH;
        assert!(opts.precludes_trigram_index());
    }

    #[test]
    fn search_line_style_defaults() {
        let style = SearchLineStyle::default();
        assert!(!style.heading());
        assert!(!style.line_number());
        assert!(!style.byte_offset());
        assert!(!style.trim());
    }

    #[test]
    fn search_record_style_defaults() {
        let style = SearchRecordStyle::default();
        assert!(matches!(style.terminator, RecordTerminator::Newline));
        assert_eq!(style.color, ColorChoice::Auto);
        assert!(style.path_separator.is_none());
    }

    #[test]
    fn search_output_defaults() {
        let output = SearchOutput::default();
        assert_eq!(output.format, SearchOutputFormat::Text);
        assert_eq!(output.mode, SearchMode::Standard);
        assert_eq!(output.emission, OutputEmission::Normal);
        assert!(matches!(output.passthru, PassthruMode::Disabled));
        assert!(matches!(output.include_zero, ZeroCountMode::Omit));
    }

    #[test]
    fn search_separators_defaults() {
        let sep = SearchSeparators::default();
        assert_eq!(sep.context_separator, Some(b"--".to_vec()));
        assert_eq!(sep.field_match_separator, b":".to_vec());
        assert_eq!(sep.field_context_separator, b"-".to_vec());
    }

    #[test]
    fn compiled_search_new_rejects_empty_patterns() {
        let result = CompiledSearch::new(&[], SearchOptions::default());
        assert!(result.is_err());
    }

    #[test]
    fn compiled_search_new_stores_patterns_and_options() {
        let patterns = vec!["foo".to_string(), "bar".to_string()];
        let opts = SearchOptions {
            case_mode: CaseMode::Insensitive,
            ..SearchOptions::default()
        };
        let search = CompiledSearch::new(&patterns, opts).expect("create search");
        assert_eq!(search.patterns(), &patterns);
        assert!(search.opts.case_insensitive());
    }
}

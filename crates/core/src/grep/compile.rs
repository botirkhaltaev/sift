//! Regex compilation — Rust regex syntax (ERE-like), with grep-style `-F`/`-w`/`-x` shaping.

use regex_automata::meta::Regex;
use regex_syntax::escape;

use super::error::SearchError;

/// Configurable pattern compiler for grep-style regex building.
///
/// Build a compiler with the desired shaping flags, then use it to shape
/// individual patterns or compile a combined regex from multiple patterns.
#[derive(Debug, Clone, Copy, Default)]
pub struct PatternCompiler {
    flags: PatternFlags,
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    struct PatternFlags: u8 {
        const FIXED_STRINGS    = 1 << 0;
        const WORD_REGEXP      = 1 << 1;
        const LINE_REGEXP      = 1 << 2;
        const CASE_INSENSITIVE = 1 << 3;
    }
}

impl PatternCompiler {
    /// Create a new compiler with default settings (sensitive, no shaping).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            flags: PatternFlags::empty(),
        }
    }

    /// Conditionally enable fixed-string escaping.
    #[must_use]
    pub fn fixed_strings(mut self, on: bool) -> Self {
        if on {
            self.flags |= PatternFlags::FIXED_STRINGS;
        }
        self
    }

    /// Conditionally enable word-boundary wrapping.
    #[must_use]
    pub fn word_regexp(mut self, on: bool) -> Self {
        if on {
            self.flags |= PatternFlags::WORD_REGEXP;
        }
        self
    }

    /// Conditionally enable line anchoring.
    #[must_use]
    pub fn line_regexp(mut self, on: bool) -> Self {
        if on {
            self.flags |= PatternFlags::LINE_REGEXP;
        }
        self
    }

    /// Conditionally enable case-insensitive matching.
    #[must_use]
    pub fn case_insensitive(mut self, on: bool) -> Self {
        if on {
            self.flags |= PatternFlags::CASE_INSENSITIVE;
        }
        self
    }

    /// Shape a single pattern string by applying escaping and anchors/boundaries.
    #[must_use]
    pub fn shape(&self, pattern: &str) -> String {
        let mut s = if self.flags.contains(PatternFlags::FIXED_STRINGS) {
            escape(pattern)
        } else {
            pattern.to_string()
        };
        if self.flags.contains(PatternFlags::LINE_REGEXP) {
            s = format!("^(?:{s})$");
        } else if self.flags.contains(PatternFlags::WORD_REGEXP) {
            s = format!(r"\b(?:{s})\b");
        }
        s
    }

    /// Compile multiple patterns into a combined alternation regex.
    ///
    /// # Errors
    ///
    /// Returns [`SearchError::RegexBuild`] if the combined pattern is invalid.
    pub fn compile(&self, patterns: &[&str]) -> Result<Regex, SearchError> {
        let branches: Vec<String> = patterns.iter().map(|p| self.shape(p)).collect();
        let combined = if branches.len() == 1 {
            branches[0].clone()
        } else {
            branches
                .into_iter()
                .map(|b| format!("(?:{b})"))
                .collect::<Vec<_>>()
                .join("|")
        };
        let mut builder = Regex::builder();
        if self.flags.contains(PatternFlags::CASE_INSENSITIVE) {
            builder.syntax(regex_automata::util::syntax::Config::new().case_insensitive(true));
        }
        builder
            .build(&combined)
            .map_err(|e| SearchError::RegexBuild(format!("regex compilation failed: {e}")))
    }

    /// Convenience: compile a single pattern.
    ///
    /// # Errors
    ///
    /// Returns [`SearchError::RegexBuild`] if the pattern is invalid.
    pub fn compile_one(&self, pattern: &str) -> Result<Regex, SearchError> {
        self.compile(&[pattern])
    }
}

/// Shape a single pattern string using the given options.
#[must_use]
pub fn pattern_branch(p: &str, opts: &super::types::SearchOptions) -> String {
    PatternCompiler::new()
        .fixed_strings(opts.fixed_strings())
        .word_regexp(opts.word_regexp())
        .line_regexp(opts.line_regexp())
        .shape(p)
}

/// Build a combined `Regex` from one or more patterns.
///
/// # Errors
///
/// Returns a boxed [`regex_automata::meta::BuildError`] if the combined pattern is invalid.
pub fn compile_search_pattern(
    patterns: &[String],
    opts: &super::types::SearchOptions,
) -> Result<Regex, Box<regex_automata::meta::BuildError>> {
    debug_assert!(!patterns.is_empty());
    let compiler = PatternCompiler::new()
        .fixed_strings(opts.fixed_strings())
        .word_regexp(opts.word_regexp())
        .line_regexp(opts.line_regexp())
        .case_insensitive(opts.case_insensitive());
    let pattern_refs: Vec<&str> = patterns.iter().map(String::as_str).collect();
    let branches: Vec<String> = pattern_refs.iter().map(|p| compiler.shape(p)).collect();
    let combined = if branches.len() == 1 {
        branches[0].clone()
    } else {
        branches
            .into_iter()
            .map(|b| format!("(?:{b})"))
            .collect::<Vec<_>>()
            .join("|")
    };
    let mut builder = Regex::builder();
    if compiler.flags.contains(PatternFlags::CASE_INSENSITIVE) {
        builder.syntax(regex_automata::util::syntax::Config::new().case_insensitive(true));
    }
    builder.build(&combined).map_err(Box::new)
}

/// Build a `Regex` for a single pattern.
///
/// # Errors
///
/// Returns a boxed [`regex_automata::meta::BuildError`] if `pattern` is invalid.
pub fn compile_pattern(
    pattern: &str,
    case_insensitive: bool,
) -> Result<Regex, Box<regex_automata::meta::BuildError>> {
    let shaped = PatternCompiler::new()
        .case_insensitive(case_insensitive)
        .shape(pattern);
    let mut builder = Regex::builder();
    if case_insensitive {
        builder.syntax(regex_automata::util::syntax::Config::new().case_insensitive(true));
    }
    builder.build(&shaped).map_err(Box::new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SearchMatchFlags, SearchOptions};

    #[test]
    fn alternation_matches_either_pattern() {
        let re = PatternCompiler::new().compile(&["foo", "bar"]).unwrap();
        let mut cache = regex_automata::meta::Cache::new(&re);
        assert!(
            re.search_with(&mut cache, &regex_automata::Input::new(b"foo"))
                .is_some()
        );
        assert!(
            re.search_with(&mut cache, &regex_automata::Input::new(b"bar"))
                .is_some()
        );
        assert!(
            re.search_with(&mut cache, &regex_automata::Input::new(b"baz"))
                .is_none()
        );
    }

    #[test]
    fn fixed_strings_escape_metacharacters() {
        let re = PatternCompiler::new()
            .fixed_strings(true)
            .compile(&[r"a.c"])
            .unwrap();
        let mut cache = regex_automata::meta::Cache::new(&re);
        assert!(
            re.search_with(&mut cache, &regex_automata::Input::new(b"a.c"))
                .is_some()
        );
        assert!(
            re.search_with(&mut cache, &regex_automata::Input::new(b"abc"))
                .is_none()
        );
    }

    #[test]
    fn case_insensitive() {
        let re = PatternCompiler::new()
            .case_insensitive(true)
            .compile(&["Hello"])
            .unwrap();
        let mut cache = regex_automata::meta::Cache::new(&re);
        assert!(
            re.search_with(&mut cache, &regex_automata::Input::new(b"hello"))
                .is_some()
        );
        assert!(
            re.search_with(&mut cache, &regex_automata::Input::new(b"HELLO"))
                .is_some()
        );
    }

    #[test]
    fn word_regexp() {
        let re = PatternCompiler::new()
            .word_regexp(true)
            .compile(&["cat"])
            .unwrap();
        let mut cache = regex_automata::meta::Cache::new(&re);
        assert!(
            re.search_with(&mut cache, &regex_automata::Input::new(b"a cat here"))
                .is_some()
        );
        assert!(
            re.search_with(&mut cache, &regex_automata::Input::new(b"concat"))
                .is_none()
        );
    }

    #[test]
    fn line_regexp() {
        let re = PatternCompiler::new()
            .line_regexp(true)
            .compile(&["yes"])
            .unwrap();
        let mut cache = regex_automata::meta::Cache::new(&re);
        assert!(
            re.search_with(&mut cache, &regex_automata::Input::new(b"yes"))
                .is_some()
        );
        assert!(
            re.search_with(&mut cache, &regex_automata::Input::new(b"oh yes sir"))
                .is_none()
        );
    }

    #[test]
    fn invalid_regex_returns_err() {
        assert!(PatternCompiler::new().compile(&["("]).is_err());
    }

    #[test]
    fn shape_fixed_strings_escapes_metacharacters() {
        let compiler = PatternCompiler::new().fixed_strings(true);
        assert_eq!(compiler.shape("a.c"), r"a\.c");
        assert_eq!(compiler.shape("foo*bar"), r"foo\*bar");
    }

    #[test]
    fn shape_word_regexp_wraps_in_word_boundary() {
        let compiler = PatternCompiler::new().word_regexp(true);
        let shaped = compiler.shape("cat");
        assert!(shaped.contains(r"\b"));
    }

    #[test]
    fn shape_line_regexp_wraps_with_anchors() {
        let compiler = PatternCompiler::new().line_regexp(true);
        let shaped = compiler.shape("yes");
        assert!(shaped.starts_with("^(?:"));
        assert!(shaped.ends_with(")$"));
    }

    #[test]
    fn shape_line_regexp_takes_precedence_over_word_regexp() {
        let compiler = PatternCompiler::new().line_regexp(true).word_regexp(true);
        let shaped = compiler.shape("yes");
        assert!(shaped.starts_with("^(?:"));
        assert!(!shaped.contains(r"\b"));
    }

    #[test]
    fn pattern_branch_reflects_search_options() {
        let mut opts = SearchOptions::default();
        opts.flags |= SearchMatchFlags::FIXED_STRINGS;
        let shaped = pattern_branch("a.c", &opts);
        assert_eq!(shaped, r"a\.c");
    }

    #[test]
    fn compile_search_pattern_rejects_invalid_regex() {
        let opts = SearchOptions::default();
        let result = compile_search_pattern(&["(".to_string()], &opts);
        assert!(result.is_err());
    }

    #[test]
    fn compile_search_pattern_case_insensitive() {
        let opts = SearchOptions {
            case_mode: crate::CaseMode::Insensitive,
            ..SearchOptions::default()
        };
        let re = compile_search_pattern(&["hello".to_string()], &opts).expect("compile");
        let mut cache = regex_automata::meta::Cache::new(&re);
        assert!(
            re.search_with(&mut cache, &regex_automata::Input::new(b"HELLO"))
                .is_some()
        );
    }

    #[test]
    fn compile_one_rejects_invalid_pattern() {
        let compiler = PatternCompiler::new();
        assert!(compiler.compile_one("(").is_err());
    }

    #[test]
    fn compile_single_pattern_returns_single_regex() {
        let re = PatternCompiler::new().compile(&["hello"]).unwrap();
        let mut cache = regex_automata::meta::Cache::new(&re);
        assert!(
            re.search_with(&mut cache, &regex_automata::Input::new(b"hello"))
                .is_some()
        );
    }
}

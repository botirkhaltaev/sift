pub mod error;

use regex_automata::meta::Regex;
use regex_syntax::escape;

use crate::search::SearchError;
use crate::search::options::SearchMatchFlags;

#[derive(Debug, Clone, Copy, Default)]
pub struct PatternCompiler {
    flags: SearchMatchFlags,
    case_insensitive: bool,
}

impl PatternCompiler {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            flags: SearchMatchFlags::empty(),
            case_insensitive: false,
        }
    }

    #[must_use]
    pub fn fixed_strings(mut self, on: bool) -> Self {
        if on {
            self.flags |= SearchMatchFlags::FIXED_STRINGS;
        }
        self
    }

    #[must_use]
    pub fn word_regexp(mut self, on: bool) -> Self {
        if on {
            self.flags |= SearchMatchFlags::WORD_REGEXP;
        }
        self
    }

    #[must_use]
    pub fn line_regexp(mut self, on: bool) -> Self {
        if on {
            self.flags |= SearchMatchFlags::LINE_REGEXP;
        }
        self
    }

    #[must_use]
    pub const fn case_insensitive(mut self, on: bool) -> Self {
        self.case_insensitive = on;
        self
    }

    #[must_use]
    pub fn shape(&self, pattern: &str) -> String {
        let mut s = if self.flags.contains(SearchMatchFlags::FIXED_STRINGS) {
            escape(pattern)
        } else {
            pattern.to_string()
        };
        if self.flags.contains(SearchMatchFlags::WORD_REGEXP) {
            s = format!(r"\b(?:{s})\b");
        }
        if self.flags.contains(SearchMatchFlags::LINE_REGEXP) {
            s = format!("^(?:{s})$");
        }
        s
    }

    /// Compiles multiple patterns into a single regex.
    ///
    /// # Errors
    ///
    /// Returns `SearchError::RegexBuild` if the combined pattern is invalid.
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
        if self.case_insensitive {
            builder.syntax(regex_automata::util::syntax::Config::new().case_insensitive(true));
        }
        builder
            .build(&combined)
            .map_err(|e| SearchError::RegexBuild(format!("regex compilation failed: {e}")))
    }

    /// Compiles a single pattern into a regex.
    ///
    /// # Errors
    ///
    /// Returns `SearchError::RegexBuild` if the pattern is invalid.
    pub fn compile_one(&self, pattern: &str) -> Result<Regex, SearchError> {
        self.compile(&[pattern])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn shape_line_regexp_wraps_in_anchors() {
        let compiler = PatternCompiler::new().line_regexp(true);
        let shaped = compiler.shape("yes");
        assert!(shaped.starts_with('^'));
        assert!(shaped.ends_with('$'));
    }

    #[test]
    fn shape_line_and_word_combined() {
        let compiler = PatternCompiler::new().line_regexp(true).word_regexp(true);
        let shaped = compiler.shape("cat");
        assert!(shaped.starts_with('^'));
        assert!(shaped.ends_with('$'));
        assert!(shaped.contains(r"\b"));
    }

    #[test]
    fn shape_no_flags_returns_pattern_unchanged() {
        let compiler = PatternCompiler::new();
        assert_eq!(compiler.shape("hello"), "hello");
    }

    #[test]
    fn compile_one_delegates_to_compile() {
        let re = PatternCompiler::new().compile_one("hello").unwrap();
        let mut cache = regex_automata::meta::Cache::new(&re);
        assert!(
            re.search_with(&mut cache, &regex_automata::Input::new(b"hello"))
                .is_some()
        );
    }
}

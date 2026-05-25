use std::sync::Mutex;

use grep_regex::RegexMatcher;
use grep_searcher::Searcher;
use once_cell::sync::OnceCell;

use crate::grep::SearchError;
use crate::grep::options::SearchOptions;

type SearcherCacheEntry = ((bool, Option<usize>, usize, usize), Searcher);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub file: std::path::PathBuf,
    pub line: usize,
    pub text: String,
}

#[derive(Debug)]
pub struct CompiledSearch {
    pub patterns: Vec<String>,
    pub opts: SearchOptions,
    pub matcher: OnceCell<RegexMatcher>,
    pub searcher_cache: Mutex<Option<SearcherCacheEntry>>,
}

impl CompiledSearch {
    /// Creates a new compiled search from patterns and options.
    ///
    /// # Errors
    ///
    /// Returns `SearchError::EmptyPatterns` if the pattern list is empty.
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
    use crate::grep::options::SearchMatchFlags;

    #[test]
    fn case_mode_insensitive_returns_true() {
        assert!(crate::grep::options::CaseMode::Insensitive.is_case_insensitive());
    }

    #[test]
    fn case_mode_sensitive_returns_false() {
        assert!(!crate::grep::options::CaseMode::Sensitive.is_case_insensitive());
    }

    #[test]
    fn case_mode_smart_returns_false() {
        assert!(!crate::grep::options::CaseMode::Smart.is_case_insensitive());
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
        assert_eq!(opts.binary_mode, crate::grep::options::BinaryMode::Quit);
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
    fn compiled_search_new_rejects_empty_patterns() {
        let result = CompiledSearch::new(&[], SearchOptions::default());
        assert!(result.is_err());
    }

    #[test]
    fn compiled_search_new_stores_patterns_and_options() {
        let patterns = vec!["foo".to_string(), "bar".to_string()];
        let opts = SearchOptions {
            case_mode: crate::grep::options::CaseMode::Insensitive,
            ..SearchOptions::default()
        };
        let search = CompiledSearch::new(&patterns, opts).expect("create search");
        assert_eq!(search.patterns(), &patterns);
        assert!(search.opts.case_insensitive());
    }
}

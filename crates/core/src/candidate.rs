use once_cell::sync::OnceCell;
use std::path::PathBuf;

use crate::search::output::style::PathDisplay;

/// A candidate file that might match a query.
///
/// Created by index planning or filesystem walk, then processed through
/// the candidate pipeline for depth, filesize, and metadata constraints.
/// The normalized string path is computed lazily on first access.
///
/// Fields are accessible via accessor methods to guarantee that the lazy
/// `rel_str` cache remains consistent with `rel_path`.
#[derive(Debug, Clone)]
pub struct Candidate {
    /// Path relative to the index root or filter root.
    rel_path: PathBuf,
    /// Absolute filesystem path.
    abs_path: PathBuf,
    rel_str: OnceCell<String>,
}

impl Candidate {
    #[must_use]
    pub const fn new(rel_path: PathBuf, abs_path: PathBuf) -> Self {
        Self {
            rel_path,
            abs_path,
            rel_str: OnceCell::new(),
        }
    }

    #[must_use]
    pub fn rel_path(&self) -> &std::path::Path {
        &self.rel_path
    }

    #[must_use]
    pub fn abs_path(&self) -> &std::path::Path {
        &self.abs_path
    }

    /// Normalized relative path string (forward slashes), computed lazily.
    #[must_use]
    pub fn rel_str(&self) -> &str {
        self.rel_str
            .get_or_init(|| self.rel_path.to_string_lossy().replace('\\', "/"))
    }

    #[must_use]
    pub fn display_path(&self, display: PathDisplay, path_separator: Option<u8>) -> String {
        let raw = match display {
            PathDisplay::Absolute => self.abs_path().display().to_string(),
            PathDisplay::Relative => self.rel_path().display().to_string(),
        };
        if let Some(sep) = path_separator {
            let sep_char = sep as char;
            raw.replace(std::path::MAIN_SEPARATOR, &sep_char.to_string())
        } else {
            raw
        }
    }

    /// Check depth constraint against a filter's max depth.
    #[must_use]
    pub fn within_depth(&self, max_depth: Option<usize>) -> bool {
        max_depth.is_none_or(|d| self.rel_path.components().count().saturating_sub(1) <= d)
    }

    /// Check filesize constraint against a filter's max filesize.
    #[must_use]
    pub fn within_filesize(&self, max_filesize: Option<u64>) -> bool {
        max_filesize
            .is_none_or(|limit| std::fs::metadata(&self.abs_path).is_ok_and(|m| m.len() <= limit))
    }

    /// Check all configured filter rules: depth, filesize, and path-based rules.
    #[must_use]
    pub fn matches(&self, filter: &crate::search::filter::CandidateFilter) -> bool {
        self.within_depth(filter.max_depth())
            && self.within_filesize(filter.max_filesize())
            && filter.matches_candidate(self)
    }
}

impl PartialEq for Candidate {
    fn eq(&self, other: &Self) -> bool {
        self.rel_path == other.rel_path && self.abs_path == other.abs_path
    }
}

impl Eq for Candidate {}

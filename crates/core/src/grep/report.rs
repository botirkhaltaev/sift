use std::path::PathBuf;

use crate::grep::matched::Match;
use crate::grep::stats::Stats;

/// Result of a search run.
pub struct Report {
    pub matched: bool,
    pub matches: Vec<Match>,
    /// Unique rel-paths with at least one pattern hit.
    pub hit_paths: Vec<PathBuf>,
    pub stats: Option<Stats>,
}

impl Report {
    #[must_use]
    pub const fn matched(&self) -> bool {
        self.matched
    }

    #[must_use]
    pub const fn stats(&self) -> Option<&Stats> {
        self.stats.as_ref()
    }
}

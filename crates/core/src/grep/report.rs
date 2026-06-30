use std::path::PathBuf;

use crate::grep::stats::GrepStats;

/// Optional artifacts gathered during grep beyond primary output.
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GrepCollection {
    pub stats: bool,
    pub hits: bool,
}

impl Default for GrepCollection {
    fn default() -> Self {
        Self::none()
    }
}

impl GrepCollection {
    pub const fn none() -> Self {
        Self {
            stats: false,
            hits: false,
        }
    }

    pub const fn stats() -> Self {
        Self {
            stats: true,
            hits: false,
        }
    }

    pub const fn hits() -> Self {
        Self {
            stats: false,
            hits: true,
        }
    }

    pub const fn stats_and_hits() -> Self {
        Self {
            stats: true,
            hits: true,
        }
    }

    pub const fn with_stats(self, stats: bool) -> Self {
        Self {
            stats,
            hits: self.hits,
        }
    }

    pub const fn with_hits(self, hits: bool) -> Self {
        Self {
            stats: self.stats,
            hits,
        }
    }
}

#[derive(Debug)]
pub struct GrepOutcome {
    pub matched: bool,
    pub stats: Option<GrepStats>,
}

/// Final result of a grep execution.
pub struct GrepReport {
    pub outcome: GrepOutcome,
    /// Unique rel-paths with at least one pattern hit.
    pub hits: Vec<PathBuf>,
}

impl GrepReport {
    #[must_use]
    pub const fn matched(&self) -> bool {
        self.outcome.matched
    }

    #[must_use]
    pub const fn stats(&self) -> Option<&GrepStats> {
        self.outcome.stats.as_ref()
    }
}

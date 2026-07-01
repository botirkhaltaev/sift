use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use grep_printer::Stats as JsonStats;
use sift_core::grep::Stats;

/// Statistics for a printed search run (`--stats`, JSON summary).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OutputStats {
    pub search: Stats,
    pub bytes_printed: u64,
}

impl OutputStats {
    pub(in crate::format) const fn from_search(search: Stats, bytes_printed: u64) -> Self {
        Self {
            search,
            bytes_printed,
        }
    }

    pub(in crate::format) fn from_json(
        merged: &JsonStats,
        candidates_len: usize,
        bytes_searched_sum: u64,
        elapsed: Duration,
        summary_line_bytes: u64,
    ) -> Self {
        use std::convert::TryFrom;
        Self {
            search: Stats {
                matches: usize::try_from(merged.matches()).unwrap_or(usize::MAX),
                files_with_matches: usize::try_from(merged.searches_with_match())
                    .unwrap_or(usize::MAX),
                files_searched: candidates_len,
                bytes_searched: bytes_searched_sum,
                elapsed,
            },
            bytes_printed: merged.bytes_printed() + summary_line_bytes,
        }
    }

    pub fn write_stderr(&self) {
        eprintln!("{} matches", self.search.matches);
        eprintln!("{} files contained matches", self.search.files_with_matches);
        eprintln!("{} files searched", self.search.files_searched);
        eprintln!("{} bytes printed", self.bytes_printed);
        eprintln!("{} bytes searched", self.search.bytes_searched);
        eprintln!("{:.6}s elapsed", self.search.elapsed.as_secs_f64());
    }
}

#[derive(Debug)]
pub(in crate::format) struct TextStatsCounters {
    primary: AtomicUsize,
    files_with_matches: AtomicUsize,
    bytes_printed: AtomicU64,
    collect_stats: bool,
}

impl TextStatsCounters {
    #[must_use]
    pub(in crate::format) const fn new(collect_stats: bool) -> Self {
        Self {
            primary: AtomicUsize::new(0),
            files_with_matches: AtomicUsize::new(0),
            bytes_printed: AtomicU64::new(0),
            collect_stats,
        }
    }

    pub(in crate::format) fn primary(&self) -> Option<&AtomicUsize> {
        self.collect_stats.then_some(&self.primary)
    }

    pub(in crate::format) fn files_with_matches(&self) -> Option<&AtomicUsize> {
        self.collect_stats.then_some(&self.files_with_matches)
    }

    pub(in crate::format) fn bytes_printed(&self) -> Option<&AtomicU64> {
        self.collect_stats.then_some(&self.bytes_printed)
    }

    pub(in crate::format) fn finish(
        self,
        candidates_len: usize,
        bytes_searched: u64,
        elapsed: Duration,
    ) -> Option<OutputStats> {
        if !self.collect_stats {
            return None;
        }
        Some(OutputStats::from_search(
            Stats {
                matches: self.primary.load(Ordering::Relaxed),
                files_with_matches: self.files_with_matches.load(Ordering::Relaxed),
                files_searched: candidates_len,
                bytes_searched,
                elapsed,
            },
            self.bytes_printed.load(Ordering::Relaxed),
        ))
    }
}

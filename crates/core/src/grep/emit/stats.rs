use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use grep_printer::Stats as JsonStats;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SearchStats {
    pub matches: usize,
    pub files_with_matches: usize,
    pub files_searched: usize,
    pub bytes_printed: u64,
    pub bytes_searched: u64,
    pub elapsed: Duration,
}

impl SearchStats {
    pub(crate) fn fill_from_json(
        &mut self,
        merged: &JsonStats,
        candidates_len: usize,
        bytes_searched_sum: u64,
        elapsed: Duration,
        summary_line_bytes: u64,
    ) {
        use std::convert::TryFrom;
        self.matches = usize::try_from(merged.matches()).unwrap_or(usize::MAX);
        self.files_with_matches =
            usize::try_from(merged.searches_with_match()).unwrap_or(usize::MAX);
        self.files_searched = candidates_len;
        self.bytes_printed = merged.bytes_printed() + summary_line_bytes;
        self.bytes_searched = bytes_searched_sum;
        self.elapsed = elapsed;
    }
}

#[derive(Debug)]
pub struct TextStatsCounters {
    primary: AtomicUsize,
    files_with_matches: AtomicUsize,
    bytes_printed: AtomicU64,
    collect_stats: bool,
}

impl TextStatsCounters {
    pub const fn new(collect_stats: bool) -> Self {
        Self {
            primary: AtomicUsize::new(0),
            files_with_matches: AtomicUsize::new(0),
            bytes_printed: AtomicU64::new(0),
            collect_stats,
        }
    }

    pub fn primary(&self) -> Option<&AtomicUsize> {
        self.collect_stats.then_some(&self.primary)
    }

    pub fn files_with_matches(&self) -> Option<&AtomicUsize> {
        self.collect_stats.then_some(&self.files_with_matches)
    }

    pub fn bytes_printed(&self) -> Option<&AtomicU64> {
        self.collect_stats.then_some(&self.bytes_printed)
    }

    pub fn finish(
        self,
        candidates_len: usize,
        bytes_searched: u64,
        elapsed: Duration,
    ) -> Option<SearchStats> {
        if !self.collect_stats {
            return None;
        }
        Some(SearchStats {
            matches: self.primary.load(Ordering::Relaxed),
            files_with_matches: self.files_with_matches.load(Ordering::Relaxed),
            files_searched: candidates_len,
            bytes_printed: self.bytes_printed.load(Ordering::Relaxed),
            bytes_searched,
            elapsed,
        })
    }
}

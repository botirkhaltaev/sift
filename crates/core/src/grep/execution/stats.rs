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

#[derive(Clone, Copy)]
pub struct StatsCollection<'a> {
    pub primary: Option<&'a std::sync::atomic::AtomicUsize>,
    pub files_with_matches: Option<&'a std::sync::atomic::AtomicUsize>,
    pub bytes_printed: Option<&'a std::sync::atomic::AtomicU64>,
}

pub fn fill_json_search_stats(
    s: &mut SearchStats,
    merged: &JsonStats,
    candidates_len: usize,
    bytes_searched_sum: u64,
    elapsed: Duration,
    summary_line_bytes: u64,
) {
    use std::convert::TryFrom;
    s.matches = usize::try_from(merged.matches()).unwrap_or(usize::MAX);
    s.files_with_matches = usize::try_from(merged.searches_with_match()).unwrap_or(usize::MAX);
    s.files_searched = candidates_len;
    s.bytes_printed = merged.bytes_printed() + summary_line_bytes;
    s.bytes_searched = bytes_searched_sum;
    s.elapsed = elapsed;
}

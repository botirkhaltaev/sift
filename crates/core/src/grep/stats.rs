use std::time::Duration;

/// Search execution statistics.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Stats {
    pub matches: usize,
    pub files_with_matches: usize,
    pub files_searched: usize,
    pub bytes_printed: u64,
    pub bytes_searched: u64,
    pub elapsed: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatsMode {
    Off,
    On,
}

impl StatsMode {
    #[must_use]
    pub const fn collect(self) -> bool {
        matches!(self, Self::On)
    }
}

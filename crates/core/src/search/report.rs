use std::path::PathBuf;
use std::time::Duration;

use crate::search::{Match, SearchMode, SearchOutcome, Stats, StatsMode};

/// Result of a search run.
pub struct Report {
    /// Whether any regex match was found.
    pub matched: bool,
    /// Whether this search mode selected any file/result.
    pub selected: bool,
    pub matches: Vec<Match>,
    /// Unique rel-paths with at least one pattern hit.
    pub hit_paths: Vec<PathBuf>,
    pub files: Vec<FileReport>,
    pub stats: Option<Stats>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileReport {
    pub path: PathBuf,
    pub matched: bool,
    pub selected: bool,
    pub line_matches: usize,
    pub match_spans: usize,
    pub bytes_searched: u64,
    pub binary_byte_offset: Option<u64>,
}

#[derive(Clone, Copy)]
pub(crate) struct SearchSummary {
    pub mode: SearchMode,
    pub stats: StatsMode,
    pub inputs_len: usize,
    pub bytes_searched: u64,
    pub elapsed: Duration,
}

impl Report {
    pub(crate) fn empty(stats: StatsMode) -> Self {
        Self {
            matched: false,
            selected: false,
            matches: Vec::new(),
            hit_paths: Vec::new(),
            files: Vec::new(),
            stats: stats.collect().then(Stats::default),
        }
    }

    pub(crate) fn from_outcomes(mut outcomes: Vec<SearchOutcome>, summary: SearchSummary) -> Self {
        let mut line_matches = Vec::new();
        let mut hit_paths = Vec::new();
        let mut matched = false;
        let mut selected = false;
        let mut match_count = 0usize;
        let mut match_spans = 0usize;
        let mut files_with_matches = 0usize;
        let mut files = Vec::with_capacity(outcomes.len());

        for outcome in &mut outcomes {
            matched |= outcome.matched;
            let file_selected = summary.mode.selects(outcome.matched);
            selected |= file_selected;
            if outcome.matched {
                files_with_matches += 1;
                if let Some(path) = outcome.hit_path.take() {
                    hit_paths.push(path);
                }
            }
            match_count += outcome.matches.len();
            match_spans += outcome.match_spans;
            files.push(FileReport {
                path: outcome.path.clone(),
                matched: outcome.matched,
                selected: file_selected,
                line_matches: outcome.line_matches,
                match_spans: outcome.match_spans,
                bytes_searched: outcome.bytes_searched,
                binary_byte_offset: outcome.binary_byte_offset,
            });
            line_matches.append(&mut outcome.matches);
        }

        let stats = summary.stats.collect().then_some(Stats {
            matches: match summary.mode {
                SearchMode::CountMatches { .. } | SearchMode::Matches => match_spans,
                SearchMode::Lines
                | SearchMode::CountLines { .. }
                | SearchMode::FilesWithMatches
                | SearchMode::FilesWithoutMatch => match_count,
            },
            files_with_matches,
            files_searched: summary.inputs_len,
            bytes_printed: 0,
            bytes_searched: summary.bytes_searched,
            elapsed: summary.elapsed,
        });

        Self {
            matched,
            selected,
            matches: line_matches,
            hit_paths,
            files,
            stats,
        }
    }

    #[must_use]
    pub const fn matched(&self) -> bool {
        self.matched
    }

    #[must_use]
    pub const fn selected(&self) -> bool {
        self.selected
    }

    #[must_use]
    pub const fn stats(&self) -> Option<&Stats> {
        self.stats.as_ref()
    }
}

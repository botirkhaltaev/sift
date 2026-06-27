use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use grep_matcher::Matcher;
use grep_regex::RegexMatcher;
use grep_searcher::{Searcher, Sink, SinkMatch};
use rayon::prelude::*;

use crate::Candidate;
use crate::search::emit::format::{ANSI_PATH, ANSI_RESET};
use crate::search::emit::result::{ChunkOutput, FileResult};
use crate::search::emit::stats::TextStatsCounters;
use crate::search::output::SearchOutput;
use crate::search::output::mode::{OutputEmission, SearchMode, ZeroCountMode};
use crate::search::output::style::FilenameMode;
use crate::search::query::SearchQuery;
use crate::search::request::SearchCollection;

#[derive(Clone, Copy)]
pub struct FileSummary {
    pub matched: bool,
    pub count: usize,
}

impl FileSummary {
    #[must_use]
    pub fn tally(&self, mode: SearchMode) -> usize {
        match mode {
            SearchMode::Count | SearchMode::CountMatches => self.count,
            SearchMode::FilesWithMatches => usize::from(self.matched),
            SearchMode::FilesWithoutMatch => usize::from(!self.matched),
            SearchMode::Standard | SearchMode::OnlyMatching => 0,
        }
    }

    #[must_use]
    pub const fn is_success(&self, mode: SearchMode) -> bool {
        match mode {
            SearchMode::Count | SearchMode::CountMatches => self.count > 0,
            SearchMode::FilesWithMatches | SearchMode::Standard | SearchMode::OnlyMatching => {
                self.matched
            }
            SearchMode::FilesWithoutMatch => !self.matched,
        }
    }

    #[must_use]
    pub const fn had_positive_hit(&self, mode: SearchMode) -> bool {
        match mode {
            SearchMode::Count | SearchMode::CountMatches => self.count > 0,
            SearchMode::FilesWithMatches => self.matched,
            SearchMode::FilesWithoutMatch | SearchMode::Standard | SearchMode::OnlyMatching => {
                false
            }
        }
    }
}

pub struct SummarySink {
    mode: SearchMode,
    matcher: Option<RegexMatcher>,
    matched: bool,
    count: usize,
}

impl SummarySink {
    #[must_use]
    pub const fn new(mode: SearchMode, matcher: Option<RegexMatcher>) -> Self {
        Self {
            mode,
            matcher,
            matched: false,
            count: 0,
        }
    }

    #[must_use]
    pub fn finish(self) -> FileSummary {
        FileSummary {
            matched: self.matched,
            count: self.count,
        }
    }
}

impl Sink for SummarySink {
    type Error = io::Error;

    fn matched(&mut self, searcher: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, Self::Error> {
        std::hint::black_box(searcher);
        self.matched = true;
        if self.mode == SearchMode::CountMatches {
            if let Some(ref matcher) = self.matcher {
                let line = mat.bytes();
                let mut n = 0;
                let _ = matcher.find_iter(line, |_| {
                    n += 1;
                    true
                });
                self.count += n;
            }
        } else {
            self.count += 1;
        }
        Ok(matches!(
            self.mode,
            SearchMode::Count | SearchMode::CountMatches
        ))
    }
}

fn summary_search_file(
    searcher: &mut Searcher,
    matcher: &RegexMatcher,
    mode: SearchMode,
    path: &Path,
) -> FileSummary {
    let sink_matcher = if mode == SearchMode::CountMatches {
        Some(matcher.clone())
    } else {
        None
    };
    let mut sink = SummarySink::new(mode, sink_matcher);
    let _ = searcher.search_path(matcher, path, &mut sink);
    sink.finish()
}

fn write_summary_record(
    out: &mut Vec<u8>,
    output: SearchOutput,
    display_path: &str,
    result: FileSummary,
) -> io::Result<()> {
    if output.emission == OutputEmission::Quiet {
        return Ok(());
    }
    let records = output.records;
    match output.mode {
        SearchMode::Count | SearchMode::CountMatches => {
            if result.count == 0 && matches!(output.include_zero, ZeroCountMode::Omit) {
                return Ok(());
            }
            let print_filename = output.lines.filename_mode != FilenameMode::Never;
            if print_filename {
                if records.should_color() {
                    out.extend_from_slice(ANSI_PATH);
                }
                write!(out, "{display_path}")?;
                if records.should_color() {
                    out.extend_from_slice(ANSI_RESET);
                }
                write!(out, ":{}", result.count)?;
            } else {
                write!(out, "{}", result.count)?;
            }
            records.terminator.write_to(out);
            Ok(())
        }
        SearchMode::FilesWithMatches => {
            if result.matched {
                if records.should_color() {
                    out.extend_from_slice(ANSI_PATH);
                }
                write!(out, "{display_path}")?;
                if records.should_color() {
                    out.extend_from_slice(ANSI_RESET);
                }
                records.terminator.write_to(out);
            }
            Ok(())
        }
        SearchMode::FilesWithoutMatch => {
            if result.matched {
                return Ok(());
            }
            if records.should_color() {
                out.extend_from_slice(ANSI_PATH);
            }
            write!(out, "{display_path}")?;
            if records.should_color() {
                out.extend_from_slice(ANSI_RESET);
            }
            records.terminator.write_to(out);
            Ok(())
        }
        SearchMode::Standard | SearchMode::OnlyMatching => unreachable!(),
    }
}

struct SummaryWorker<'a> {
    matcher: &'a RegexMatcher,
    searcher: Searcher,
    output: SearchOutput,
    summary_counter: Option<&'a AtomicUsize>,
    files_with_matches: Option<&'a AtomicUsize>,
    collect_hits: bool,
}

impl<'a> SummaryWorker<'a> {
    fn new(scan: &'a SummaryScan<'a>, collect: SearchCollection) -> Self {
        Self {
            searcher: scan
                .search
                .build_searcher(false, scan.search.opts().max_results, false),
            matcher: scan.matcher,
            output: scan.output,
            summary_counter: scan.counters.primary(),
            files_with_matches: scan.counters.files_with_matches(),
            collect_hits: collect.hits,
        }
    }

    fn search_candidate(&mut self, candidate: &Candidate, stop: &AtomicBool) -> FileResult {
        if stop.load(Ordering::SeqCst) {
            return FileResult {
                output: ChunkOutput::empty(),
                json_stats: None,
                hit: None,
            };
        }

        let result = summary_search_file(
            &mut self.searcher,
            self.matcher,
            self.output.mode,
            candidate.abs_path(),
        );
        if let Some(c) = self.summary_counter {
            c.fetch_add(result.tally(self.output.mode), Ordering::Relaxed);
        }
        if let Some(c) = self.files_with_matches
            && result.had_positive_hit(self.output.mode)
        {
            c.fetch_add(1, Ordering::Relaxed);
        }
        let matched = result.is_success(self.output.mode);
        let mut bytes = Vec::new();
        let display = candidate.display_path(
            self.output.lines.path_display,
            self.output.records.path_separator,
        );
        let _ = write_summary_record(&mut bytes, self.output, &display, result);
        if self.output.emission == OutputEmission::Quiet && result.is_success(self.output.mode) {
            stop.store(true, Ordering::SeqCst);
        }

        FileResult {
            output: ChunkOutput {
                bytes,
                matched,
                heading: false,
            },
            json_stats: None,
            hit: (self.collect_hits && result.matched).then(|| candidate.rel_path().to_path_buf()),
        }
    }
}

pub struct SummaryScan<'a> {
    search: &'a SearchQuery,
    matcher: &'a RegexMatcher,
    output: SearchOutput,
    counters: &'a TextStatsCounters,
}

impl<'a> SummaryScan<'a> {
    pub const fn new(
        search: &'a SearchQuery,
        matcher: &'a RegexMatcher,
        output: SearchOutput,
        counters: &'a TextStatsCounters,
    ) -> Self {
        Self {
            search,
            matcher,
            output,
            counters,
        }
    }

    /// # Errors
    ///
    /// Returns an error if scanning or writing output fails.
    pub fn run(
        &self,
        candidates: &[Candidate],
        collect: SearchCollection,
    ) -> crate::Result<(bool, Vec<PathBuf>)> {
        let stop = AtomicBool::new(false);
        let n = candidates.len();
        let mut files = Vec::with_capacity(n);
        candidates
            .par_iter()
            .map_init(
                || SummaryWorker::new(self, collect),
                |worker: &mut SummaryWorker<'_>, candidate: &Candidate| {
                    worker.search_candidate(candidate, &stop)
                },
            )
            .collect_into_vec(&mut files);
        let mut hits = Vec::new();
        let mut outputs = Vec::with_capacity(files.len());
        let mut any_match = false;
        for file in files {
            if collect.hits
                && let Some(hit) = file.hit
            {
                hits.push(hit);
            }
            any_match |= file.output.matched;
            outputs.push(file.output);
        }
        ChunkOutput::flush_all(outputs, self.counters.bytes_printed())?;
        Ok((any_match, hits))
    }
}

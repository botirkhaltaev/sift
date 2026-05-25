use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use grep_matcher::Matcher;
use grep_regex::RegexMatcher;
use grep_searcher::{Searcher, Sink, SinkMatch};
use rayon::prelude::*;

use crate::grep::emit::format::{
    ANSI_PATH, ANSI_RESET, display_path_for_candidate, should_color, write_line_terminator,
};
use crate::grep::emit::result::{ChunkOutput, FileResult, flush_chunk_output};
use crate::grep::emit::stats::StatsCollection;
use crate::grep::filter::CandidateInfo;
use crate::grep::output::SearchOutput;
use crate::grep::output::mode::{OutputEmission, SearchMode, ZeroCountMode};
use crate::grep::output::style::FilenameMode;
use crate::grep::query::SearchQuery;

#[derive(Clone, Copy)]
pub struct FileSummary {
    pub matched: bool,
    pub count: usize,
}

#[inline]
pub const fn summary_file_had_positive_hit(mode: SearchMode, r: FileSummary) -> bool {
    match mode {
        SearchMode::Count | SearchMode::CountMatches => r.count > 0,
        SearchMode::FilesWithMatches => r.matched,
        SearchMode::FilesWithoutMatch | SearchMode::Standard | SearchMode::OnlyMatching => false,
    }
}

#[inline]
pub fn summary_matches_tally(mode: SearchMode, result: FileSummary) -> usize {
    match mode {
        SearchMode::Count | SearchMode::CountMatches => result.count,
        SearchMode::FilesWithMatches => usize::from(result.matched),
        SearchMode::FilesWithoutMatch => usize::from(!result.matched),
        SearchMode::Standard | SearchMode::OnlyMatching => 0,
    }
}

pub struct SummarySink {
    mode: SearchMode,
    matcher: Option<RegexMatcher>,
    matched: bool,
    count: usize,
}

impl SummarySink {
    pub const fn new(mode: SearchMode, matcher: Option<RegexMatcher>) -> Self {
        Self {
            mode,
            matcher,
            matched: false,
            count: 0,
        }
    }

    pub fn finish(self) -> FileSummary {
        FileSummary {
            matched: self.matched,
            count: self.count,
        }
    }
}

impl Sink for SummarySink {
    type Error = io::Error;

    fn matched(&mut self, _: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, Self::Error> {
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

pub fn summary_search_file(
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

pub const fn mode_is_success(mode: SearchMode, result: FileSummary) -> bool {
    match mode {
        SearchMode::Count | SearchMode::CountMatches => result.count > 0,
        SearchMode::FilesWithMatches | SearchMode::Standard | SearchMode::OnlyMatching => {
            result.matched
        }
        SearchMode::FilesWithoutMatch => !result.matched,
    }
}

pub fn write_summary_record(
    out: &mut Vec<u8>,
    output: SearchOutput,
    display_path: &str,
    result: FileSummary,
) -> io::Result<()> {
    if output.emission == OutputEmission::Quiet {
        return Ok(());
    }
    match output.mode {
        SearchMode::Count | SearchMode::CountMatches => {
            if result.count == 0 && matches!(output.include_zero, ZeroCountMode::Omit) {
                return Ok(());
            }
            let print_filename = output.lines.filename_mode != FilenameMode::Never;
            if print_filename {
                if should_color(output.records) {
                    out.extend_from_slice(ANSI_PATH);
                }
                write!(out, "{display_path}")?;
                if should_color(output.records) {
                    out.extend_from_slice(ANSI_RESET);
                }
                writeln!(out, ":{}", result.count)?;
            } else {
                writeln!(out, "{}", result.count)?;
            }
            Ok(())
        }
        SearchMode::FilesWithMatches => {
            if result.matched {
                if should_color(output.records) {
                    out.extend_from_slice(ANSI_PATH);
                }
                write!(out, "{display_path}")?;
                if should_color(output.records) {
                    out.extend_from_slice(ANSI_RESET);
                }
                write_line_terminator(out, output.records.terminator);
            }
            Ok(())
        }
        SearchMode::FilesWithoutMatch => {
            if result.matched {
                return Ok(());
            }
            if should_color(output.records) {
                out.extend_from_slice(ANSI_PATH);
            }
            write!(out, "{display_path}")?;
            if should_color(output.records) {
                out.extend_from_slice(ANSI_RESET);
            }
            write_line_terminator(out, output.records.terminator);
            Ok(())
        }
        SearchMode::Standard | SearchMode::OnlyMatching => unreachable!(),
    }
}

pub struct SummaryWorker<'a> {
    matcher: &'a RegexMatcher,
    searcher: Searcher,
    mode: SearchMode,
    summary_counter: Option<&'a AtomicUsize>,
    files_with_matches: Option<&'a AtomicUsize>,
}

impl<'a> SummaryWorker<'a> {
    pub fn new(
        search: &SearchQuery,
        matcher: &'a RegexMatcher,
        max_results: Option<usize>,
        mode: SearchMode,
        summary_counter: Option<&'a AtomicUsize>,
        files_with_matches: Option<&'a AtomicUsize>,
    ) -> Self {
        Self {
            searcher: search.build_searcher(false, max_results, false),
            matcher,
            mode,
            summary_counter,
            files_with_matches,
        }
    }

    pub fn search_file(&mut self, path: &Path) -> FileSummary {
        summary_search_file(&mut self.searcher, self.matcher, self.mode, path)
    }

    pub fn search_candidate(
        &mut self,
        candidate: &CandidateInfo,
        result_index: usize,
        output: SearchOutput,
        stop: &AtomicBool,
    ) -> FileResult {
        if stop.load(Ordering::SeqCst) {
            return FileResult {
                index: result_index,
                output: ChunkOutput::empty(),
                json_stats: None,
            };
        }

        let result = self.search_file(&candidate.abs_path);
        if let Some(c) = self.summary_counter {
            c.fetch_add(summary_matches_tally(self.mode, result), Ordering::Relaxed);
        }
        if let Some(c) = self.files_with_matches
            && summary_file_had_positive_hit(self.mode, result)
        {
            c.fetch_add(1, Ordering::Relaxed);
        }
        let matched = mode_is_success(output.mode, result);
        let mut bytes = Vec::new();
        let display = display_path_for_candidate(
            candidate,
            output.lines.path_display,
            output.records.path_separator,
        );
        let _ = write_summary_record(&mut bytes, output, &display, result);
        if output.emission == OutputEmission::Quiet && mode_is_success(output.mode, result) {
            stop.store(true, Ordering::SeqCst);
        }

        FileResult {
            index: result_index,
            output: ChunkOutput {
                bytes,
                matched,
                heading: false,
            },
            json_stats: None,
        }
    }
}

pub fn run_summary_with_info(
    search: &SearchQuery,
    candidates: &[CandidateInfo],
    matcher: &RegexMatcher,
    output: SearchOutput,
    stats: StatsCollection<'_>,
) -> crate::Result<bool> {
    let stop = AtomicBool::new(false);
    let n = candidates.len();
    let mut files = Vec::with_capacity(n);
    candidates
        .par_iter()
        .enumerate()
        .map_init(
            || {
                SummaryWorker::new(
                    search,
                    matcher,
                    search.opts.max_results,
                    output.mode,
                    stats.primary,
                    stats.files_with_matches,
                )
            },
            |worker: &mut SummaryWorker<'_>, (result_index, candidate): (usize, &CandidateInfo)| {
                worker.search_candidate(candidate, result_index, output, &stop)
            },
        )
        .collect_into_vec(&mut files);
    files.sort_by_key(|file| file.index);
    flush_chunk_output(
        files.into_iter().map(|file| file.output),
        stats.bytes_printed,
    )
}

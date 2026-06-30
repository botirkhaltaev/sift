use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::grep::GrepCollection;
use crate::grep::input::GrepInput;
use crate::grep::output::GrepOutput;
use crate::grep::output::mode::{GrepMode, OutputEmission, ZeroCountMode};
use crate::grep::output::style::FilenameMode;
use crate::grep::query::GrepQuery;
use crate::grep::query::matcher::GrepMatcher;
use crate::grep::sink::FileReporter;
use crate::grep::sink::result::{ChunkOutput, FileResult};
use crate::grep::sink::style::ANSI_RESET;
use crate::grep::stats::TextStatsCounters;
use grep_matcher::Matcher;
use grep_searcher::{Searcher, Sink, SinkMatch};

#[derive(Clone, Copy)]
pub struct FileSummary {
    pub matched: bool,
    pub count: usize,
}

impl FileSummary {
    #[must_use]
    pub fn tally(&self, mode: GrepMode) -> usize {
        match mode {
            GrepMode::Count | GrepMode::CountMatches => self.count,
            GrepMode::FilesWithMatches => usize::from(self.matched),
            GrepMode::FilesWithoutMatch => usize::from(!self.matched),
            GrepMode::Standard | GrepMode::OnlyMatching => 0,
        }
    }

    #[must_use]
    pub const fn is_success(&self, mode: GrepMode) -> bool {
        match mode {
            GrepMode::Count | GrepMode::CountMatches => self.count > 0,
            GrepMode::FilesWithMatches | GrepMode::Standard | GrepMode::OnlyMatching => {
                self.matched
            }
            GrepMode::FilesWithoutMatch => !self.matched,
        }
    }

    #[must_use]
    pub const fn had_positive_hit(&self, mode: GrepMode) -> bool {
        match mode {
            GrepMode::Count | GrepMode::CountMatches => self.count > 0,
            GrepMode::FilesWithMatches => self.matched,
            GrepMode::FilesWithoutMatch | GrepMode::Standard | GrepMode::OnlyMatching => false,
        }
    }
}

pub struct SummarySink {
    mode: GrepMode,
    matcher: Option<GrepMatcher>,
    matched: bool,
    count: usize,
}

impl SummarySink {
    #[must_use]
    pub const fn new(mode: GrepMode, matcher: Option<GrepMatcher>) -> Self {
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
        if self.mode == GrepMode::CountMatches {
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
            GrepMode::Count | GrepMode::CountMatches
        ))
    }
}

fn summary_search_file(
    searcher: &mut Searcher,
    matcher: &GrepMatcher,
    mode: GrepMode,
    path: &Path,
) -> FileSummary {
    let sink_matcher = if mode == GrepMode::CountMatches {
        Some(matcher.clone())
    } else {
        None
    };
    let mut sink = SummarySink::new(mode, sink_matcher);
    let _ = searcher.search_path(matcher, path, &mut sink);
    sink.finish()
}

fn summary_search_slice(
    searcher: &mut Searcher,
    matcher: &GrepMatcher,
    mode: GrepMode,
    bytes: &[u8],
) -> FileSummary {
    let sink_matcher = if mode == GrepMode::CountMatches {
        Some(matcher.clone())
    } else {
        None
    };
    let mut sink = SummarySink::new(mode, sink_matcher);
    let _ = searcher.search_slice(matcher, bytes, &mut sink);
    sink.finish()
}

fn write_summary_record(
    out: &mut Vec<u8>,
    output: GrepOutput,
    display_path: &str,
    result: FileSummary,
) -> io::Result<()> {
    if output.emission == OutputEmission::Quiet {
        return Ok(());
    }
    let records = output.records;
    match output.mode {
        GrepMode::Count | GrepMode::CountMatches => {
            if result.count == 0 && matches!(output.include_zero, ZeroCountMode::Omit) {
                return Ok(());
            }
            let print_filename = output.lines.filename_mode != FilenameMode::Never;
            if print_filename {
                if records.should_color() {
                    records.colors.path.write_start(out);
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
        GrepMode::FilesWithMatches => {
            if result.matched {
                if records.should_color() {
                    records.colors.path.write_start(out);
                }
                write!(out, "{display_path}")?;
                if records.should_color() {
                    out.extend_from_slice(ANSI_RESET);
                }
                records.terminator.write_to(out);
            }
            Ok(())
        }
        GrepMode::FilesWithoutMatch => {
            if result.matched {
                return Ok(());
            }
            if records.should_color() {
                records.colors.path.write_start(out);
            }
            write!(out, "{display_path}")?;
            if records.should_color() {
                out.extend_from_slice(ANSI_RESET);
            }
            records.terminator.write_to(out);
            Ok(())
        }
        GrepMode::Standard | GrepMode::OnlyMatching => unreachable!(),
    }
}

pub(in crate::grep) struct SummaryReporter<'a> {
    matcher: &'a GrepMatcher,
    searcher: Searcher,
    output: GrepOutput,
    summary_counter: Option<&'a AtomicUsize>,
    files_with_matches: Option<&'a AtomicUsize>,
    collect_hits: bool,
}

impl<'a> SummaryReporter<'a> {
    pub(in crate::grep) fn new(
        search: &'a GrepQuery,
        matcher: &'a GrepMatcher,
        output: GrepOutput,
        counters: &'a TextStatsCounters,
        collect: GrepCollection,
    ) -> Self {
        Self {
            searcher: search.build_searcher(false, search.opts().max_results, false),
            matcher,
            output,
            summary_counter: counters.primary(),
            files_with_matches: counters.files_with_matches(),
            collect_hits: collect.hits,
        }
    }

    fn record(
        &self,
        display: &str,
        result: FileSummary,
        stop: &AtomicBool,
        hit: Option<PathBuf>,
    ) -> FileResult {
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
        let _ = write_summary_record(&mut bytes, self.output.clone(), display, result);
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
            hit: result.matched.then_some(hit).flatten(),
        }
    }
}

impl FileReporter for SummaryReporter<'_> {
    fn report(&mut self, input: &GrepInput<'_>, stop: &AtomicBool) -> FileResult {
        if stop.load(Ordering::SeqCst) {
            return FileResult {
                output: ChunkOutput::empty(),
                json_stats: None,
                hit: None,
            };
        }
        match input {
            GrepInput::Path { candidate } => {
                let result = summary_search_file(
                    &mut self.searcher,
                    self.matcher,
                    self.output.mode,
                    candidate.abs_path(),
                );
                self.record(
                    &candidate.display_path(
                        self.output.lines.path_display,
                        self.output.records.path_separator,
                    ),
                    result,
                    stop,
                    (self.collect_hits).then(|| candidate.rel_path().to_path_buf()),
                )
            }
            GrepInput::Bytes {
                display_path,
                bytes,
                candidate,
            } => {
                let result =
                    summary_search_slice(&mut self.searcher, self.matcher, self.output.mode, bytes);
                let display = candidate.map_or_else(
                    || display_path.to_string(),
                    |candidate| {
                        candidate.display_path(
                            self.output.lines.path_display,
                            self.output.records.path_separator,
                        )
                    },
                );
                self.record(
                    &display,
                    result,
                    stop,
                    candidate
                        .filter(|_| self.collect_hits)
                        .map(|candidate| candidate.rel_path().to_path_buf()),
                )
            }
        }
    }
}

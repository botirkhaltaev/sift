use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::format::collection::PrintExtras;
use crate::format::output::PrintSpec;
use crate::format::output::mode::{OutputEmission, PrintMode, ZeroCountMode};
use crate::format::output::style::FilenameMode;
use crate::format::sink::InputPrinter;
use crate::format::sink::result::{ChunkOutput, FileResult};
use crate::format::sink::style::ANSI_RESET;
use crate::format::stats::TextStatsCounters;
use grep_matcher::Matcher as GrepMatcherTrait;
use grep_searcher::{Searcher, Sink, SinkMatch};
use sift_core::grep::{CompiledQuery, Input, Matcher, Query, SearcherConfig};

#[derive(Clone, Copy)]
pub struct FileSummary {
    pub matched: bool,
    pub count: usize,
    pub binary_byte_offset: Option<u64>,
}

impl FileSummary {
    #[must_use]
    pub fn tally(&self, mode: PrintMode) -> usize {
        match mode {
            PrintMode::Count | PrintMode::CountMatches => self.count,
            PrintMode::FilesWithMatches => usize::from(self.matched),
            PrintMode::FilesWithoutMatch => usize::from(!self.matched),
            PrintMode::Standard | PrintMode::OnlyMatching => 0,
        }
    }

    #[must_use]
    pub const fn is_success(&self, mode: PrintMode) -> bool {
        match mode {
            PrintMode::Count | PrintMode::CountMatches => self.count > 0,
            PrintMode::FilesWithMatches | PrintMode::Standard | PrintMode::OnlyMatching => {
                self.matched
            }
            PrintMode::FilesWithoutMatch => !self.matched,
        }
    }

    #[must_use]
    pub const fn had_positive_hit(&self, mode: PrintMode) -> bool {
        match mode {
            PrintMode::Count | PrintMode::CountMatches => self.count > 0,
            PrintMode::FilesWithMatches => self.matched,
            PrintMode::FilesWithoutMatch | PrintMode::Standard | PrintMode::OnlyMatching => false,
        }
    }

    #[must_use]
    pub const fn explicit_binary(mut self, explicit: bool) -> Self {
        if explicit && self.binary_byte_offset.is_some() {
            self.matched = true;
            if self.count == 0 {
                self.count = 1;
            }
        }
        self
    }
}

pub struct SummarySink {
    mode: PrintMode,
    matcher: Option<Matcher>,
    matched: bool,
    count: usize,
    binary_byte_offset: Option<u64>,
}

impl SummarySink {
    #[must_use]
    pub const fn new(mode: PrintMode, matcher: Option<Matcher>) -> Self {
        Self {
            mode,
            matcher,
            matched: false,
            count: 0,
            binary_byte_offset: None,
        }
    }

    #[must_use]
    pub fn finish(self) -> FileSummary {
        FileSummary {
            matched: self.matched,
            count: self.count,
            binary_byte_offset: self.binary_byte_offset,
        }
    }
}

impl Sink for SummarySink {
    type Error = io::Error;

    fn matched(&mut self, _searcher: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, Self::Error> {
        self.matched = true;
        if self.mode == PrintMode::CountMatches {
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
            PrintMode::Count | PrintMode::CountMatches
        ))
    }

    fn binary_data(
        &mut self,
        _searcher: &Searcher,
        binary_byte_offset: u64,
    ) -> Result<bool, Self::Error> {
        self.binary_byte_offset.get_or_insert(binary_byte_offset);
        Ok(true)
    }
}

fn write_summary_record(
    out: &mut Vec<u8>,
    output: PrintSpec,
    display_path: &str,
    result: FileSummary,
) -> io::Result<()> {
    if output.emission == OutputEmission::Quiet {
        return Ok(());
    }
    let records = output.records;
    match output.mode {
        PrintMode::Count | PrintMode::CountMatches => {
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
        PrintMode::FilesWithMatches => {
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
        PrintMode::FilesWithoutMatch => {
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
        PrintMode::Standard | PrintMode::OnlyMatching => unreachable!(),
    }
}

pub(in crate::format) struct AggregatePrinter<'a> {
    compiled: &'a CompiledQuery,
    matcher: &'a Matcher,
    searcher: Searcher,
    output: PrintSpec,
    summary_counter: Option<&'a AtomicUsize>,
    files_with_matches: Option<&'a AtomicUsize>,
    collect_hits: bool,
}

impl<'a> AggregatePrinter<'a> {
    pub(in crate::format) fn new(
        search: &'a Query,
        compiled: &'a CompiledQuery,
        output: PrintSpec,
        counters: &'a TextStatsCounters,
        collect: PrintExtras,
    ) -> Self {
        Self {
            compiled,
            searcher: SearcherConfig {
                line_numbers: false,
                max_matches: search.opts().max_results,
                include_context: false,
            }
            .searcher(search),
            matcher: compiled.matcher(),
            output,
            summary_counter: counters.primary(),
            files_with_matches: counters.files_with_matches(),
            collect_hits: collect.collect_hits(),
        }
    }

    fn search_summary(&mut self, input: &Input<'_>) -> FileSummary {
        let sink_matcher = if self.output.mode == PrintMode::CountMatches {
            Some(self.matcher.clone())
        } else {
            None
        };
        let mut sink = SummarySink::new(self.output.mode, sink_matcher);
        self.compiled
            .match_input(input, &mut self.searcher, &mut sink);
        sink.finish()
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

impl InputPrinter for AggregatePrinter<'_> {
    fn report(&mut self, input: &Input<'_>, stop: &AtomicBool) -> FileResult {
        if stop.load(Ordering::SeqCst) {
            return FileResult {
                output: ChunkOutput::empty(),
                json_stats: None,
                hit: None,
            };
        }
        match input {
            Input::Path {
                candidate,
                explicit,
            } => {
                let result = self.search_summary(input).explicit_binary(*explicit);
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
            Input::Bytes {
                path,
                bytes: _,
                candidate,
                explicit,
            } => {
                let result = self.search_summary(input).explicit_binary(*explicit);
                let display = candidate.map_or_else(
                    || path.to_string(),
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

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::format::collection::PrintExtras;
use crate::format::output::PrintSpec;
use crate::format::output::mode::{OutputEmission, PrintMode, ZeroCountMode};
use crate::format::output::style::{FilenameMode, RecordTerminator};
use crate::format::sink::InputPrinter;
use crate::format::sink::result::{ChunkOutput, FileResult};
use crate::format::stats::TextStatsCounters;
use grep_matcher::LineTerminator;
use grep_matcher::Matcher as GrepMatcherTrait;
use grep_printer::{Stats as PrinterStats, SummaryBuilder, SummaryKind};
use grep_searcher::{BinaryDetection, SearcherBuilder};
use sift_core::grep::{BinaryMode, CompiledQuery, Input, Query};

pub(in crate::format) struct AggregatePrinter<'a> {
    compiled: &'a CompiledQuery,
    search: &'a Query,
    output: PrintSpec,
    builder: SummaryBuilder,
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
            search,
            builder: summary_builder(&output),
            output,
            summary_counter: counters.primary(),
            files_with_matches: counters.files_with_matches(),
            collect_hits: collect.collect_hits(),
        }
    }

    fn search_summary(&self, input: &Input<'_>, display: &str) -> SummaryResult {
        let printer = self
            .builder
            .build(self.output.records.color_output().buffer());
        let binary_detection = if self.search.opts().null_data() {
            BinaryDetection::none()
        } else {
            match self.search.opts().binary_mode {
                BinaryMode::Quit
                    if matches!(
                        input,
                        Input::Path { explicit: true, .. } | Input::Bytes { explicit: true, .. }
                    ) =>
                {
                    BinaryDetection::convert(b'\x00')
                }
                BinaryMode::Quit => BinaryDetection::quit(b'\x00'),
                BinaryMode::Binary => BinaryDetection::convert(b'\x00'),
                BinaryMode::AsText => BinaryDetection::none(),
            }
        };
        let mut builder = SearcherBuilder::new();
        builder
            .encoding(self.search.opts().input_encoding.explicit())
            .bom_sniffing(self.search.opts().input_encoding.bom_sniffing())
            .binary_detection(binary_detection)
            .line_terminator(LineTerminator::byte(self.search.opts().line_terminator()))
            .invert_match(self.search.opts().invert_match())
            .line_number(false)
            .max_matches(self.search.opts().max_results.map(|n| n as u64));
        if self.search.opts().multiline() {
            builder.multi_line(true);
        }
        let mut searcher = builder.build();
        match self.compiled {
            CompiledQuery::Rust { matcher, .. } => {
                self.search_with_matcher(matcher, input, display, printer, &mut searcher)
            }
            CompiledQuery::Pcre2 { matcher, .. } => {
                self.search_with_matcher(matcher, input, display, printer, &mut searcher)
            }
        }
    }

    fn search_with_matcher<M: GrepMatcherTrait>(
        &self,
        matcher: &M,
        input: &Input<'_>,
        display: &str,
        mut printer: grep_printer::Summary<termcolor::Buffer>,
        searcher: &mut grep_searcher::Searcher,
    ) -> SummaryResult {
        let (success, stats) = {
            let mut sink = printer.sink_with_path(matcher, Path::new(display));
            match input {
                Input::Path { candidate, .. } => {
                    let _ = searcher.search_path(matcher, candidate.abs_path(), &mut sink);
                }
                Input::Bytes { bytes, .. } => {
                    let _ = searcher.search_slice(matcher, bytes, &mut sink);
                }
            }
            (sink.has_match(), sink.stats().cloned().unwrap_or_default())
        };
        SummaryResult::new(
            printer.into_inner().into_inner(),
            success,
            &stats,
            self.output.mode,
        )
    }

    fn record(&self, result: SummaryResult, stop: &AtomicBool, hit: Option<PathBuf>) -> FileResult {
        if let Some(c) = self.summary_counter {
            c.fetch_add(result.tally, Ordering::Relaxed);
        }
        if let Some(c) = self.files_with_matches
            && result.actual_matched
        {
            c.fetch_add(1, Ordering::Relaxed);
        }
        if self.output.emission == OutputEmission::Quiet && result.success {
            stop.store(true, Ordering::SeqCst);
        }

        FileResult {
            output: ChunkOutput {
                bytes: if self.output.emission == OutputEmission::Quiet {
                    Vec::new()
                } else {
                    result.bytes
                },
                matched: result.success,
                heading: false,
            },
            json_stats: None,
            hit: result.actual_matched.then_some(hit).flatten(),
        }
    }
}

struct SummaryResult {
    bytes: Vec<u8>,
    success: bool,
    actual_matched: bool,
    tally: usize,
}

impl SummaryResult {
    fn new(bytes: Vec<u8>, success: bool, stats: &PrinterStats, mode: PrintMode) -> Self {
        let actual_matched = stats.searches_with_match() > 0;
        let tally = match mode {
            PrintMode::Count => usize::try_from(stats.matched_lines()).unwrap_or(usize::MAX),
            PrintMode::CountMatches => usize::try_from(stats.matches()).unwrap_or(usize::MAX),
            PrintMode::FilesWithMatches => usize::from(actual_matched),
            PrintMode::FilesWithoutMatch => usize::from(!actual_matched),
            PrintMode::Standard | PrintMode::OnlyMatching => 0,
        };
        Self {
            bytes,
            success,
            actual_matched,
            tally,
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
                explicit: _,
            } => {
                let display = candidate.display_path(
                    self.output.lines.path_display,
                    self.output.records.path_separator,
                );
                let result = self.search_summary(input, &display);
                self.record(
                    result,
                    stop,
                    (self.collect_hits).then(|| candidate.rel_path().to_path_buf()),
                )
            }
            Input::Bytes {
                path,
                bytes: _,
                candidate,
                explicit: _,
            } => {
                let display = candidate.map_or_else(
                    || path.to_string(),
                    |candidate| {
                        candidate.display_path(
                            self.output.lines.path_display,
                            self.output.records.path_separator,
                        )
                    },
                );
                let result = self.search_summary(input, &display);
                self.record(
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

fn summary_builder(output: &PrintSpec) -> SummaryBuilder {
    let mut builder = SummaryBuilder::new();
    builder
        .kind(summary_kind(output.mode))
        .stats(true)
        .path(output.lines.filename_mode != FilenameMode::Never)
        .exclude_zero(matches!(output.include_zero, ZeroCountMode::Omit))
        .separator_path(output.records.path_separator)
        .color_specs(output.records.colors.as_grep())
        .hyperlink(
            output
                .records
                .hyperlink
                .config(output.records.hyperlink_host.clone()),
        );
    if matches!(output.records.terminator, RecordTerminator::Nul) {
        builder.path_terminator(Some(b'\0'));
    }
    builder
}

const fn summary_kind(mode: PrintMode) -> SummaryKind {
    match mode {
        PrintMode::Count => SummaryKind::Count,
        PrintMode::CountMatches => SummaryKind::CountMatches,
        PrintMode::FilesWithMatches => SummaryKind::PathWithMatch,
        PrintMode::FilesWithoutMatch => SummaryKind::PathWithoutMatch,
        PrintMode::Standard | PrintMode::OnlyMatching => unreachable!(),
    }
}

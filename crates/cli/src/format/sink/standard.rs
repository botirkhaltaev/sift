use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::format::collection::PrintExtras;
use crate::format::output::PrintSpec;
use crate::format::output::format::ColumnOverflow;
use crate::format::output::mode::{OutputEmission, PrintMode};
use crate::format::output::style::{FilenameMode, LineStyleFlags, PrintSeparators};
use crate::format::sink::InputPrinter;
use crate::format::sink::result::{ChunkOutput, FileResult};
use crate::format::stats::TextStatsCounters;
use grep_matcher::LineTerminator;
use grep_matcher::Matcher as GrepMatcherTrait;
use grep_printer::StandardBuilder;
use grep_searcher::{BinaryDetection, SearcherBuilder};
use sift_core::grep::{BinaryMode, CompiledQuery, Input, Query};

pub(in crate::format) struct LinePrinter<'a> {
    compiled: &'a CompiledQuery,
    search: &'a Query,
    output: PrintSpec,
    builder: StandardBuilder,
    match_counter: Option<&'a AtomicUsize>,
    files_with_matches: Option<&'a AtomicUsize>,
    path_display: crate::format::output::style::PathDisplay,
    path_separator: Option<u8>,
    emission: OutputEmission,
    collect_hits: bool,
}

enum GrepTarget {
    File {
        display: String,
        hit: Option<PathBuf>,
    },
    Bytes {
        display: String,
        hit: Option<PathBuf>,
    },
}

impl GrepTarget {
    fn display(&self) -> &str {
        match self {
            Self::File { display, .. } | Self::Bytes { display, .. } => display,
        }
    }

    fn hit(self, matched: bool) -> Option<PathBuf> {
        match self {
            Self::File { hit, .. } | Self::Bytes { hit, .. } if matched => hit,
            Self::File { .. } | Self::Bytes { .. } => None,
        }
    }
}

impl<'a> LinePrinter<'a> {
    pub(in crate::format) fn new(
        search: &'a Query,
        compiled: &'a CompiledQuery,
        output: &PrintSpec,
        separators: &'a PrintSeparators,
        counters: &'a TextStatsCounters,
        collect: PrintExtras,
    ) -> Self {
        Self {
            compiled,
            search,
            output: output.clone(),
            builder: standard_builder(output, separators, search.opts().replace.as_deref()),
            match_counter: counters.primary(),
            files_with_matches: counters.files_with_matches(),
            path_display: output.lines.path_display,
            path_separator: output.records.path_separator,
            emission: output.emission,
            collect_hits: collect.collect_hits(),
        }
    }

    fn search_target(
        &self,
        input: &Input<'_>,
        target: GrepTarget,
        stop: &AtomicBool,
    ) -> FileResult {
        if stop.load(Ordering::SeqCst) {
            return FileResult {
                output: ChunkOutput::empty(),
                json_stats: None,
                hit: None,
            };
        }

        let display = target.display().to_string();
        let report = self.search_with_printer(input, &display);

        if let Some(c) = self.match_counter {
            c.fetch_add(report.match_count, Ordering::Relaxed);
        }
        if report.matched
            && let Some(c) = self.files_with_matches
        {
            c.fetch_add(1, Ordering::Relaxed);
        }
        if self.emission == OutputEmission::Quiet && report.matched {
            stop.store(true, Ordering::SeqCst);
        }

        FileResult {
            output: ChunkOutput {
                bytes: if self.emission == OutputEmission::Quiet {
                    Vec::new()
                } else {
                    report.bytes
                },
                matched: report.matched,
                heading: report.matched
                    && self.output.lines.heading()
                    && self.output.lines.filename_mode != FilenameMode::Never
                    && self.emission != OutputEmission::Quiet,
            },
            json_stats: None,
            hit: target.hit(report.matched),
        }
    }

    fn search_with_printer(&self, input: &Input<'_>, display: &str) -> TextSearchResult {
        let printer = self
            .builder
            .build(self.output.records.color_output().buffer());
        let before_context = self.search.opts().before_context;
        let after_context = self.search.opts().after_context;
        let line_number =
            self.output.lines.line_number() || before_context > 0 || after_context > 0;
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
            .line_number(line_number)
            .before_context(before_context)
            .after_context(after_context)
            .max_matches(self.search.opts().max_results.map(|n| n as u64));
        if self.search.opts().multiline() {
            builder.multi_line(true);
        }
        let mut searcher = builder.build();
        match self.compiled {
            CompiledQuery::Rust { matcher, .. } => {
                Self::search_with_matcher(matcher, input, display, printer, &mut searcher)
            }
            CompiledQuery::Pcre2 { matcher, .. } => {
                Self::search_with_matcher(matcher, input, display, printer, &mut searcher)
            }
        }
    }

    fn search_with_matcher<M: GrepMatcherTrait>(
        matcher: &M,
        input: &Input<'_>,
        display: &str,
        mut printer: grep_printer::Standard<termcolor::Buffer>,
        searcher: &mut grep_searcher::Searcher,
    ) -> TextSearchResult {
        let (has_match, match_count) = {
            let mut sink = printer.sink_with_path(matcher, Path::new(display));
            match input {
                Input::Path { candidate, .. } => {
                    let _ = searcher.search_path(matcher, candidate.abs_path(), &mut sink);
                }
                Input::Bytes { bytes, .. } => {
                    let _ = searcher.search_slice(matcher, bytes, &mut sink);
                }
            }
            (
                sink.has_match(),
                usize::try_from(sink.match_count()).unwrap_or(usize::MAX),
            )
        };
        TextSearchResult {
            bytes: printer.into_inner().into_inner(),
            matched: has_match,
            match_count,
        }
    }
}

struct TextSearchResult {
    bytes: Vec<u8>,
    matched: bool,
    match_count: usize,
}

impl InputPrinter for LinePrinter<'_> {
    fn report(&mut self, input: &Input<'_>, stop: &AtomicBool) -> FileResult {
        match input {
            Input::Path { candidate, .. } => self.search_target(
                input,
                GrepTarget::File {
                    display: candidate.display_path(self.path_display, self.path_separator),
                    hit: (self.collect_hits).then(|| candidate.rel_path().to_path_buf()),
                },
                stop,
            ),
            Input::Bytes {
                path,
                bytes: _,
                candidate,
                explicit: _,
            } => {
                let display = candidate.map_or_else(
                    || path.to_string(),
                    |candidate| candidate.display_path(self.path_display, self.path_separator),
                );
                self.search_target(
                    input,
                    GrepTarget::Bytes {
                        display,
                        hit: candidate
                            .filter(|_| self.collect_hits)
                            .map(|candidate| candidate.rel_path().to_path_buf()),
                    },
                    stop,
                )
            }
        }
    }
}

fn standard_builder(
    output: &PrintSpec,
    separators: &PrintSeparators,
    replacement: Option<&str>,
) -> StandardBuilder {
    let mut builder = StandardBuilder::new();
    builder
        .color_specs(output.records.colors.as_grep())
        .hyperlink(
            output
                .records
                .hyperlink
                .config(output.records.hyperlink_host.clone()),
        )
        .heading(output.lines.heading() && output.lines.filename_mode != FilenameMode::Never)
        .path(output.lines.filename_mode != FilenameMode::Never)
        .only_matching(output.mode == PrintMode::OnlyMatching)
        .column(output.lines.flags.contains(LineStyleFlags::COLUMN))
        .byte_offset(output.lines.byte_offset())
        .trim_ascii(output.lines.trim())
        .separator_context(separators.context_separator.clone())
        .separator_field_match(separators.field_match_separator.clone())
        .separator_field_context(separators.field_context_separator.clone())
        .separator_path(output.records.path_separator)
        .replacement(replacement.map(|replacement| replacement.as_bytes().to_vec()));
    if let Some(columns) = output.lines.columns {
        builder
            .max_columns(Some(columns.max))
            .max_columns_preview(columns.overflow == ColumnOverflow::Preview);
    }
    if matches!(
        output.records.terminator,
        crate::format::output::style::RecordTerminator::Nul
    ) {
        builder.path_terminator(Some(b'\0'));
    }
    builder
}

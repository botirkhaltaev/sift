use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::format::collection::PrintExtras;
use crate::format::output::PrintSpec;
use crate::format::output::format::ColumnAction;
use crate::format::output::mode::OutputEmission;
use crate::format::output::style::{
    AnsiStyle, FilenameMode, HyperlinkValues, LineStyleFlags, PrintRecordStyle, PrintSeparators,
};
use crate::format::sink::InputPrinter;
use crate::format::sink::result::{ChunkOutput, FileResult};
use crate::format::sink::style::ANSI_RESET;
use crate::format::stats::TextStatsCounters;
use grep_matcher::{Captures, Matcher as GrepMatcherTrait};
use grep_searcher::{Searcher, Sink, SinkContext, SinkMatch};
use sift_core::grep::{BinaryMode, CompiledQuery, Input, Matcher, Query, SearcherConfig};

#[derive(Clone, Copy)]
pub struct SinkConfig {
    pub before_context: usize,
    pub after_context: usize,
    pub binary_mode: BinaryMode,
}

pub struct StandardSink<'a> {
    matcher: &'a Matcher,
    output: PrintSpec,
    show_line_numbers: bool,
    display_path: String,
    hyperlink_path: String,
    bytes: &'a mut Vec<u8>,
    separators: &'a PrintSeparators,
    matched: bool,
    match_count: usize,
    binary_byte_offset: Option<u64>,
    binary_mode: BinaryMode,
    replace: Option<&'a str>,
    trim: bool,
    line_terminator: u8,
}

struct SinkTarget {
    display_path: String,
    hyperlink_path: String,
}

impl<'a> StandardSink<'a> {
    fn new(
        matcher: &'a Matcher,
        output: &PrintSpec,
        target: SinkTarget,
        bytes: &'a mut Vec<u8>,
        separators: &'a PrintSeparators,
        replace: Option<&'a str>,
        config: SinkConfig,
    ) -> Self {
        Self {
            matcher,
            output: output.clone(),
            show_line_numbers: output.lines.line_number()
                || config.before_context > 0
                || config.after_context > 0,
            display_path: target.display_path,
            hyperlink_path: target.hyperlink_path,
            bytes,
            separators,
            matched: false,
            match_count: 0,
            binary_byte_offset: None,
            binary_mode: config.binary_mode,
            replace,
            trim: output.lines.trim(),
            line_terminator: b'\n',
        }
    }

    #[must_use]
    const fn with_line_terminator(mut self, line_terminator: u8) -> Self {
        self.line_terminator = line_terminator;
        self
    }
}

impl Sink for StandardSink<'_> {
    type Error = io::Error;

    fn matched(&mut self, _searcher: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, Self::Error> {
        self.matched = true;
        self.match_count += 1;

        if self.output.emission == OutputEmission::Quiet {
            return Ok(true);
        }

        if matches!(
            self.output.mode,
            crate::format::output::mode::PrintMode::OnlyMatching
        ) {
            if self.binary_mode == BinaryMode::Binary && self.binary_byte_offset.is_some() {
                return Ok(true);
            }
            return self.handle_only_matching(mat);
        }

        let line_bytes = mat.bytes();
        if self.handle_max_columns(line_bytes, mat)? {
            return Ok(true);
        }

        let col = self.compute_first_column(mat);
        if self.binary_mode == BinaryMode::Binary && self.binary_byte_offset.is_some() {
            return Ok(true);
        }
        self.write_prefix(
            mat.line_number(),
            false,
            col,
            if self.output.lines.byte_offset() {
                Some(mat.absolute_byte_offset())
            } else {
                None
            },
        )?;
        self.write_line_content(mat.bytes())
    }

    fn context(
        &mut self,
        _searcher: &Searcher,
        ctx: &SinkContext<'_>,
    ) -> Result<bool, Self::Error> {
        if self.output.emission == OutputEmission::Quiet {
            return Ok(true);
        }
        if matches!(
            self.output.mode,
            crate::format::output::mode::PrintMode::OnlyMatching
        ) {
            return Ok(true);
        }
        let ctx_bytes = ctx.bytes();
        let columns = self.output.lines.columns;
        match columns.map(|c| c.classify(ctx_bytes)) {
            Some(ColumnAction::Omit) => return Ok(true),
            Some(ColumnAction::Preview) => {
                let limit = columns.unwrap();
                self.write_prefix(
                    ctx.line_number(),
                    true,
                    None,
                    if self.output.lines.byte_offset() {
                        Some(ctx.absolute_byte_offset())
                    } else {
                        None
                    },
                )?;
                let truncated = limit.truncate(ctx_bytes);
                self.bytes.write_all(&truncated)?;
                return Ok(true);
            }
            Some(ColumnAction::Normal) | None => {}
        }
        self.write_prefix(
            ctx.line_number(),
            true,
            None,
            if self.output.lines.byte_offset() {
                Some(ctx.absolute_byte_offset())
            } else {
                None
            },
        )?;
        let line_bytes = ctx.bytes();
        if self.trim {
            let s = String::from_utf8_lossy(line_bytes);
            let trimmed = s.trim_start();
            self.bytes.write_all(trimmed.as_bytes())?;
            if !trimmed.as_bytes().ends_with(&[self.line_terminator]) {
                self.bytes.write_all(&[self.line_terminator])?;
            }
        } else {
            self.bytes.write_all(line_bytes)?;
            if !line_bytes.ends_with(&[self.line_terminator]) {
                self.bytes.write_all(&[self.line_terminator])?;
            }
        }
        Ok(true)
    }

    fn context_break(&mut self, _searcher: &Searcher) -> Result<bool, Self::Error> {
        if self.output.emission == OutputEmission::Quiet {
            return Ok(true);
        }
        if matches!(
            self.output.mode,
            crate::format::output::mode::PrintMode::OnlyMatching
        ) {
            return Ok(true);
        }
        if let Some(ref sep) = self.separators.context_separator {
            self.bytes.write_all(sep)?;
            self.bytes.write_all(&[self.line_terminator])?;
        }
        Ok(true)
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

impl StandardSink<'_> {
    fn handle_only_matching(&mut self, mat: &SinkMatch<'_>) -> Result<bool, io::Error> {
        let show_column = self.output.lines.flags.contains(LineStyleFlags::COLUMN);
        let line_number = mat.line_number();
        let line = mat.bytes();
        let byte_offset = mat.absolute_byte_offset();
        let mut matches = Vec::new();
        self.matcher
            .find_iter(line, |m: grep_matcher::Match| {
                matches.push(m);
                true
            })
            .map_err(io::Error::other)?;
        for m in matches {
            let col = if show_column {
                Some(m.start() + 1)
            } else {
                None
            };
            let matched_slice = &line[m.start()..m.end()];
            let text = if let Some(rep) = self.replace {
                apply_replace(self.matcher, matched_slice, rep)
            } else {
                String::from_utf8_lossy(matched_slice).into_owned()
            };
            self.write_prefix(
                line_number,
                false,
                col,
                if self.output.lines.byte_offset() {
                    Some(byte_offset)
                } else {
                    None
                },
            )?;
            self.bytes.write_all(text.as_bytes())?;
            self.bytes.write_all(&[self.line_terminator])?;
        }
        Ok(true)
    }

    fn handle_max_columns(
        &mut self,
        line_bytes: &[u8],
        mat: &SinkMatch<'_>,
    ) -> Result<bool, io::Error> {
        let columns = self.output.lines.columns;
        match columns.map(|c| c.classify(line_bytes)) {
            Some(ColumnAction::Omit) => return Ok(true),
            Some(ColumnAction::Preview) => {
                let limit = columns.unwrap();
                self.write_prefix(
                    mat.line_number(),
                    false,
                    None,
                    if self.output.lines.byte_offset() {
                        Some(mat.absolute_byte_offset())
                    } else {
                        None
                    },
                )?;
                let truncated = limit.truncate(line_bytes);
                self.bytes.write_all(&truncated)?;
                return Ok(true);
            }
            Some(ColumnAction::Normal) | None => {}
        }
        Ok(false)
    }

    fn compute_first_column(&self, mat: &SinkMatch<'_>) -> Option<usize> {
        if !self.output.lines.flags.contains(LineStyleFlags::COLUMN) {
            return None;
        }
        let line = mat.bytes();
        let mut first_col = None;
        let _ = self.matcher.find_iter(line, |m: grep_matcher::Match| {
            first_col = Some(m.start() + 1);
            false
        });
        first_col
    }

    fn write_prefix(
        &mut self,
        line_number: Option<u64>,
        is_context_line: bool,
        column: Option<usize>,
        byte_offset: Option<u64>,
    ) -> io::Result<()> {
        let color = self.output.records.should_color();
        let print_filename = self.output.lines.filename_mode != FilenameMode::Never;
        let field_sep = if is_context_line {
            &self.separators.field_context_separator
        } else {
            &self.separators.field_match_separator
        };
        if print_filename {
            if color {
                self.write_style_start(self.output.records.colors.path);
            }
            self.start_hyperlink(line_number, column)?;
            write!(self.bytes, "{}", self.display_path)?;
            self.end_hyperlink();
            if color {
                self.write_style_end();
            }
            self.bytes.extend_from_slice(field_sep);
        }
        if self.show_line_numbers {
            if color {
                self.write_style_start(self.output.records.colors.line);
            }
            write!(self.bytes, "{}", line_number.unwrap_or(0))?;
            if color {
                self.write_style_end();
            }
            self.bytes.extend_from_slice(field_sep);
        }
        if let Some(col) = column {
            if color {
                self.write_style_start(self.output.records.colors.column);
            }
            write!(self.bytes, "{col}")?;
            if color {
                self.write_style_end();
            }
            self.bytes.extend_from_slice(field_sep);
        }
        if let Some(offset) = byte_offset {
            if color {
                self.write_style_start(self.output.records.colors.line);
            }
            let sep = if is_context_line { '-' } else { ':' };
            write!(self.bytes, "{offset}{sep}")?;
            if color {
                self.write_style_end();
            }
        }
        Ok(())
    }

    fn write_line_content(&mut self, line_bytes: &[u8]) -> Result<bool, io::Error> {
        if let Some(rep) = self.replace {
            let text = apply_replace(self.matcher, line_bytes, rep);
            self.bytes.write_all(text.as_bytes())?;
            if !text.as_bytes().ends_with(&[self.line_terminator]) {
                self.bytes.write_all(&[self.line_terminator])?;
            }
        } else if self.trim {
            let trimmed = String::from_utf8_lossy(line_bytes);
            let trimmed = trimmed.trim_start();
            self.bytes.write_all(trimmed.as_bytes())?;
            if !trimmed.as_bytes().ends_with(&[self.line_terminator]) {
                self.bytes.write_all(&[self.line_terminator])?;
            }
        } else {
            self.write_matched_line(line_bytes)?;
            if !line_bytes.ends_with(&[self.line_terminator]) {
                self.bytes.write_all(&[self.line_terminator])?;
            }
        }
        Ok(true)
    }

    fn write_matched_line(&mut self, line_bytes: &[u8]) -> io::Result<()> {
        if !self.output.records.should_color() {
            return self.bytes.write_all(line_bytes);
        }
        let mut spans = Vec::new();
        self.matcher
            .find_iter(line_bytes, |m: grep_matcher::Match| {
                spans.push((m.start(), m.end()));
                true
            })
            .map_err(io::Error::other)?;
        let mut cursor = 0;
        for (start, end) in spans {
            self.bytes.write_all(&line_bytes[cursor..start])?;
            self.write_style_start(self.output.records.colors.matched);
            self.bytes.write_all(&line_bytes[start..end])?;
            self.write_style_end();
            cursor = end;
        }
        self.bytes.write_all(&line_bytes[cursor..])
    }

    fn write_style_start(&mut self, style: AnsiStyle) {
        if style.is_plain() {
            self.bytes.extend_from_slice(b"\x1b[0m");
        } else {
            style.write_start(self.bytes);
        }
    }

    fn write_style_end(&mut self) {
        self.bytes.extend_from_slice(ANSI_RESET);
    }

    fn start_hyperlink(
        &mut self,
        line_number: Option<u64>,
        column: Option<usize>,
    ) -> io::Result<()> {
        if let Some(link) = self.output.records.hyperlink.render(HyperlinkValues {
            path: &self.hyperlink_path,
            line: line_number,
            column,
            host: self.output.records.hyperlink_host.as_deref(),
        }) {
            write!(self.bytes, "\x1b]8;;{link}\x1b\\")?;
        }
        Ok(())
    }

    fn end_hyperlink(&mut self) {
        if !self.output.records.hyperlink.is_empty() {
            self.bytes.extend_from_slice(b"\x1b]8;;\x1b\\");
        }
    }
}

pub(in crate::format) struct LinePrinter<'a> {
    compiled: &'a CompiledQuery,
    matcher: &'a Matcher,
    searcher: Searcher,
    output: PrintSpec,
    separators: &'a PrintSeparators,
    bytes: Vec<u8>,
    match_counter: Option<&'a AtomicUsize>,
    files_with_matches: Option<&'a AtomicUsize>,
    replace: Option<String>,
    sink_config: SinkConfig,
    records: PrintRecordStyle,
    lines_flags: LineStyleFlags,
    filename_mode: FilenameMode,
    path_display: crate::format::output::style::PathDisplay,
    path_separator: Option<u8>,
    emission: OutputEmission,
    collect_hits: bool,
    line_terminator: u8,
}

enum GrepTarget {
    File {
        display: String,
        hyperlink: String,
        hit: Option<PathBuf>,
        explicit: bool,
    },
    Bytes {
        display: String,
        hyperlink: String,
        hit: Option<PathBuf>,
        explicit: bool,
    },
}

impl GrepTarget {
    fn display(&self) -> &str {
        match self {
            Self::File { display, .. } | Self::Bytes { display, .. } => display,
        }
    }

    fn hyperlink(&self) -> &str {
        match self {
            Self::File { hyperlink, .. } | Self::Bytes { hyperlink, .. } => hyperlink,
        }
    }

    fn hit(self, matched: bool) -> Option<PathBuf> {
        match self {
            Self::File { hit, .. } if matched => hit,
            Self::Bytes { hit, .. } if matched => hit,
            Self::File { .. } | Self::Bytes { .. } => None,
        }
    }

    const fn explicit_file(&self) -> bool {
        match self {
            Self::File { explicit, .. } | Self::Bytes { explicit, .. } => *explicit,
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
            searcher: SearcherConfig {
                line_numbers: output.lines.line_number(),
                max_matches: search.opts().max_results,
                include_context: true,
            }
            .searcher(search),
            matcher: compiled.matcher(),
            output: output.clone(),
            separators,
            bytes: Vec::new(),
            match_counter: counters.primary(),
            files_with_matches: counters.files_with_matches(),
            replace: search.opts().replace.clone(),
            sink_config: SinkConfig {
                before_context: search.opts().before_context,
                after_context: search.opts().after_context,
                binary_mode: search.opts().binary_mode,
            },
            records: output.records.clone(),
            lines_flags: output.lines.flags,
            filename_mode: output.lines.filename_mode,
            path_display: output.lines.path_display,
            path_separator: output.records.path_separator,
            emission: output.emission,
            collect_hits: collect.collect_hits(),
            line_terminator: search.opts().line_terminator(),
        }
    }

    fn search_target(
        &mut self,
        input: &Input<'_>,
        target: GrepTarget,
        stop: &AtomicBool,
    ) -> FileResult {
        self.bytes.clear();
        if stop.load(Ordering::SeqCst) {
            return FileResult {
                output: ChunkOutput::empty(),
                json_stats: None,
                hit: None,
            };
        }

        let display = target.display().to_string();

        let (sink_matched, binary_byte_offset) = {
            let heading = self.lines_flags.contains(LineStyleFlags::HEADING)
                && self.filename_mode != FilenameMode::Never;
            let mut sink_output = self.output.clone();
            if heading {
                sink_output.lines.filename_mode = FilenameMode::Never;
            }
            let hyperlink = target.hyperlink().to_string();
            let mut sink = StandardSink::new(
                self.matcher,
                &sink_output,
                SinkTarget {
                    display_path: display.clone(),
                    hyperlink_path: hyperlink,
                },
                &mut self.bytes,
                self.separators,
                self.replace.as_deref(),
                self.sink_config,
            )
            .with_line_terminator(self.line_terminator);
            self.compiled
                .match_input(input, &mut self.searcher, &mut sink);
            let n = sink.match_count;
            let binary_byte_offset = sink.binary_byte_offset;
            if let Some(c) = self.match_counter {
                c.fetch_add(n, Ordering::Relaxed);
            }
            (sink.matched, binary_byte_offset)
        };

        let binary_match = binary_byte_offset.is_some()
            && (self.sink_config.binary_mode == BinaryMode::Binary
                || (self.sink_config.binary_mode == BinaryMode::Quit && target.explicit_file()));
        let matched = sink_matched || binary_match;
        if matched && let Some(c) = self.files_with_matches {
            c.fetch_add(1, Ordering::Relaxed);
        }

        if self.emission == OutputEmission::Quiet && matched {
            stop.store(true, Ordering::SeqCst);
        }

        FileResult {
            output: ChunkOutput {
                bytes: if matched && binary_byte_offset.is_some() {
                    self.binary_output(binary_byte_offset, &display)
                } else if matched
                    && self.lines_flags.contains(LineStyleFlags::HEADING)
                    && self.filename_mode != FilenameMode::Never
                    && self.emission != OutputEmission::Quiet
                {
                    let mut out = Vec::new();
                    if self.records.should_color() {
                        self.records.colors.path.write_start(&mut out);
                    }
                    let _ = write!(out, "{display}");
                    if self.records.should_color() {
                        out.extend_from_slice(ANSI_RESET);
                    }
                    self.records.terminator.write_to(&mut out);
                    out.extend(std::mem::take(&mut self.bytes));
                    out
                } else {
                    std::mem::take(&mut self.bytes)
                },
                matched,
                heading: matched
                    && self.lines_flags.contains(LineStyleFlags::HEADING)
                    && self.filename_mode != FilenameMode::Never
                    && self.emission != OutputEmission::Quiet,
            },
            json_stats: None,
            hit: target.hit(matched),
        }
    }

    fn binary_output(&mut self, offset: Option<u64>, display: &str) -> Vec<u8> {
        let Some(offset) = offset else {
            return std::mem::take(&mut self.bytes);
        };
        self.bytes.clear();
        if self.emission == OutputEmission::Quiet {
            return Vec::new();
        }
        if self.filename_mode != FilenameMode::Never {
            if self.records.should_color() {
                self.records.colors.path.write_start(&mut self.bytes);
            }
            let _ = write!(self.bytes, "{display}");
            if self.records.should_color() {
                self.bytes.extend_from_slice(ANSI_RESET);
            }
            self.bytes.extend_from_slice(b": ");
        }
        let _ = write!(
            self.bytes,
            "binary file matches (found \"\\0\" byte around offset {offset})"
        );
        self.records.terminator.write_to(&mut self.bytes);
        std::mem::take(&mut self.bytes)
    }
}

impl InputPrinter for LinePrinter<'_> {
    fn report(&mut self, input: &Input<'_>, stop: &AtomicBool) -> FileResult {
        match input {
            Input::Path {
                candidate,
                explicit,
            } => self.search_target(
                input,
                GrepTarget::File {
                    display: candidate.display_path(self.path_display, self.path_separator),
                    hyperlink: candidate.abs_path().display().to_string(),
                    hit: (self.collect_hits).then(|| candidate.rel_path().to_path_buf()),
                    explicit: *explicit,
                },
                stop,
            ),
            Input::Bytes {
                path,
                bytes: _,
                candidate,
                explicit,
            } => {
                let display = candidate.map_or_else(
                    || path.to_string(),
                    |candidate| candidate.display_path(self.path_display, self.path_separator),
                );
                let hyperlink = candidate.map_or_else(
                    || path.to_string(),
                    |candidate| candidate.abs_path().display().to_string(),
                );
                self.search_target(
                    input,
                    GrepTarget::Bytes {
                        display,
                        hyperlink,
                        hit: candidate
                            .filter(|_| self.collect_hits)
                            .map(|candidate| candidate.rel_path().to_path_buf()),
                        explicit: *explicit,
                    },
                    stop,
                )
            }
        }
    }
}

fn apply_replace(matcher: &Matcher, line: &[u8], replacement: &str) -> String {
    let Ok(mut caps) = matcher.new_captures() else {
        return String::from_utf8_lossy(line).into_owned();
    };
    let mut dst = Vec::new();
    let _ = matcher.replace_with_captures(line, &mut caps, &mut dst, |caps, dst| {
        caps.interpolate(
            |name| matcher.capture_index(name),
            line,
            replacement.as_bytes(),
            dst,
        );
        true
    });
    String::from_utf8_lossy(&dst).into_owned()
}

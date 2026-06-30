use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::grep::GrepCollection;
use crate::grep::input::GrepInput;
use crate::grep::output::GrepOutput;
use crate::grep::output::format::ColumnAction;
use crate::grep::output::mode::OutputEmission;
use crate::grep::output::style::{
    AnsiStyle, FilenameMode, GrepRecordStyle, GrepSeparators, HyperlinkValues, LineStyleFlags,
};
use crate::grep::query::GrepQuery;
use crate::grep::query::matcher::GrepMatcher;
use crate::grep::sink::FileReporter;
use crate::grep::sink::result::{ChunkOutput, FileResult};
use crate::grep::sink::style::ANSI_RESET;
use crate::grep::stats::TextStatsCounters;
use grep_matcher::{Captures, Matcher};
use grep_searcher::{Searcher, Sink, SinkContext, SinkMatch};

#[derive(Clone, Copy)]
pub struct SinkConfig {
    pub before_context: usize,
    pub after_context: usize,
}

pub struct StandardSink<'a> {
    matcher: &'a GrepMatcher,
    output: GrepOutput,
    show_line_numbers: bool,
    display_path: String,
    hyperlink_path: String,
    bytes: &'a mut Vec<u8>,
    separators: &'a GrepSeparators,
    matched: bool,
    match_count: usize,
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
        matcher: &'a GrepMatcher,
        output: &GrepOutput,
        target: SinkTarget,
        bytes: &'a mut Vec<u8>,
        separators: &'a GrepSeparators,
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

    fn matched(&mut self, searcher: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, Self::Error> {
        std::hint::black_box(searcher);
        self.matched = true;
        self.match_count += 1;

        if self.output.emission == OutputEmission::Quiet {
            return Ok(true);
        }

        if matches!(
            self.output.mode,
            crate::grep::output::mode::GrepMode::OnlyMatching
        ) {
            return self.handle_only_matching(mat);
        }

        let line_bytes = mat.bytes();
        if self.handle_max_columns(line_bytes, mat)? {
            return Ok(true);
        }

        let col = self.compute_first_column(mat);
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

    fn context(&mut self, searcher: &Searcher, ctx: &SinkContext<'_>) -> Result<bool, Self::Error> {
        std::hint::black_box(searcher);
        if self.output.emission == OutputEmission::Quiet {
            return Ok(true);
        }
        if matches!(
            self.output.mode,
            crate::grep::output::mode::GrepMode::OnlyMatching
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

    fn context_break(&mut self, searcher: &Searcher) -> Result<bool, Self::Error> {
        std::hint::black_box(searcher);
        if self.output.emission == OutputEmission::Quiet {
            return Ok(true);
        }
        if matches!(
            self.output.mode,
            crate::grep::output::mode::GrepMode::OnlyMatching
        ) {
            return Ok(true);
        }
        if let Some(ref sep) = self.separators.context_separator {
            self.bytes.write_all(sep)?;
            self.bytes.write_all(&[self.line_terminator])?;
        }
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

pub(in crate::grep) struct StandardReporter<'a> {
    matcher: &'a GrepMatcher,
    searcher: Searcher,
    output: GrepOutput,
    separators: &'a GrepSeparators,
    bytes: Vec<u8>,
    match_counter: Option<&'a AtomicUsize>,
    files_with_matches: Option<&'a AtomicUsize>,
    replace: Option<String>,
    sink_config: SinkConfig,
    records: GrepRecordStyle,
    lines_flags: LineStyleFlags,
    filename_mode: FilenameMode,
    path_display: crate::grep::output::style::PathDisplay,
    path_separator: Option<u8>,
    emission: OutputEmission,
    collect_hits: bool,
    line_terminator: u8,
}

enum GrepTarget<'a> {
    File {
        display: String,
        hyperlink: String,
        abs_path: &'a Path,
        hit: Option<PathBuf>,
    },
    Bytes {
        display: String,
        hyperlink: String,
        bytes: &'a [u8],
        hit: Option<PathBuf>,
    },
}

impl GrepTarget<'_> {
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
}

impl<'a> StandardReporter<'a> {
    pub(in crate::grep) fn new(
        search: &'a GrepQuery,
        matcher: &'a GrepMatcher,
        output: &GrepOutput,
        separators: &'a GrepSeparators,
        counters: &'a TextStatsCounters,
        collect: GrepCollection,
    ) -> Self {
        Self {
            searcher: search.build_searcher(
                output.lines.line_number(),
                search.opts().max_results,
                true,
            ),
            matcher,
            output: output.clone(),
            separators,
            bytes: Vec::new(),
            match_counter: counters.primary(),
            files_with_matches: counters.files_with_matches(),
            replace: search.opts().replace.clone(),
            sink_config: SinkConfig {
                before_context: search.opts().before_context,
                after_context: search.opts().after_context,
            },
            records: output.records.clone(),
            lines_flags: output.lines.flags,
            filename_mode: output.lines.filename_mode,
            path_display: output.lines.path_display,
            path_separator: output.records.path_separator,
            emission: output.emission,
            collect_hits: collect.hits,
            line_terminator: search.opts().line_terminator(),
        }
    }

    fn search_target(&mut self, target: GrepTarget<'_>, stop: &AtomicBool) -> FileResult {
        self.bytes.clear();
        if stop.load(Ordering::SeqCst) {
            return FileResult {
                output: ChunkOutput::empty(),
                json_stats: None,
                hit: None,
            };
        }

        let display = target.display().to_string();

        let matched = {
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
            let _ = match &target {
                GrepTarget::File { abs_path, .. } => {
                    self.searcher.search_path(self.matcher, abs_path, &mut sink)
                }
                GrepTarget::Bytes { bytes, .. } => {
                    self.searcher.search_slice(self.matcher, bytes, &mut sink)
                }
            };
            let n = sink.match_count;
            if let Some(c) = self.match_counter {
                c.fetch_add(n, Ordering::Relaxed);
            }
            if sink.matched
                && let Some(c) = self.files_with_matches
            {
                c.fetch_add(1, Ordering::Relaxed);
            }
            sink.matched
        };

        if self.emission == OutputEmission::Quiet && matched {
            stop.store(true, Ordering::SeqCst);
        }

        FileResult {
            output: ChunkOutput {
                bytes: if matched
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
}

impl FileReporter for StandardReporter<'_> {
    fn report(&mut self, input: &GrepInput<'_>, stop: &AtomicBool) -> FileResult {
        match input {
            GrepInput::Path { candidate } => self.search_target(
                GrepTarget::File {
                    display: candidate.display_path(self.path_display, self.path_separator),
                    hyperlink: candidate.abs_path().display().to_string(),
                    abs_path: candidate.abs_path(),
                    hit: (self.collect_hits).then(|| candidate.rel_path().to_path_buf()),
                },
                stop,
            ),
            GrepInput::Bytes {
                display_path,
                bytes,
                candidate,
            } => {
                let display = candidate.map_or_else(
                    || display_path.to_string(),
                    |candidate| candidate.display_path(self.path_display, self.path_separator),
                );
                let hyperlink = candidate.map_or_else(
                    || display_path.to_string(),
                    |candidate| candidate.abs_path().display().to_string(),
                );
                self.search_target(
                    GrepTarget::Bytes {
                        display,
                        hyperlink,
                        bytes,
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

fn apply_replace(matcher: &GrepMatcher, line: &[u8], replacement: &str) -> String {
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

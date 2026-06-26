use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use grep_matcher::{Captures, Matcher};
use grep_regex::RegexMatcher;
use grep_searcher::{Searcher, Sink, SinkContext, SinkMatch};
use rayon::prelude::*;

use crate::Candidate;
use crate::search::emit::format::{ANSI_LINE, ANSI_PATH, ANSI_RESET};
use crate::search::emit::result::{ChunkOutput, FileResult};
use crate::search::emit::stats::TextStatsCounters;
use crate::search::output::SearchOutput;
use crate::search::output::format::ColumnAction;
use crate::search::output::mode::OutputEmission;
use crate::search::output::style::{
    FilenameMode, LineStyleFlags, SearchRecordStyle, SearchSeparators,
};
use crate::search::query::SearchQuery;
use crate::search::request::SearchCollection;

#[derive(Clone, Copy)]
pub struct SinkConfig {
    pub before_context: usize,
    pub after_context: usize,
}

pub struct StandardSink<'a> {
    matcher: &'a RegexMatcher,
    output: SearchOutput,
    show_line_numbers: bool,
    display_path: String,
    bytes: &'a mut Vec<u8>,
    separators: &'a SearchSeparators,
    matched: bool,
    match_count: usize,
    replace: Option<&'a str>,
    trim: bool,
    line_terminator: u8,
}

impl<'a> StandardSink<'a> {
    pub const fn new(
        matcher: &'a RegexMatcher,
        output: SearchOutput,
        display_path: String,
        bytes: &'a mut Vec<u8>,
        separators: &'a SearchSeparators,
        replace: Option<&'a str>,
        config: SinkConfig,
    ) -> Self {
        Self {
            matcher,
            output,
            show_line_numbers: output.lines.line_number()
                || config.before_context > 0
                || config.after_context > 0,
            display_path,
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
            crate::search::output::mode::SearchMode::OnlyMatching
        ) {
            return Ok(self.handle_only_matching(mat));
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
            crate::search::output::mode::SearchMode::OnlyMatching
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
            crate::search::output::mode::SearchMode::OnlyMatching
        ) {
            return Ok(true);
        }
        if let Some(ref sep) = self.separators.context_separator {
            self.bytes.write_all(sep)?;
            self.bytes.write_all(b"\n")?;
        }
        Ok(true)
    }
}

impl StandardSink<'_> {
    fn handle_only_matching(&mut self, mat: &SinkMatch<'_>) -> bool {
        let show_column = self.output.lines.flags.contains(LineStyleFlags::COLUMN);
        let line_number = mat.line_number();
        let line = mat.bytes();
        let byte_offset = mat.absolute_byte_offset();
        let _ = self.matcher.find_iter(line, |m: grep_matcher::Match| {
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
            let _ = self.write_prefix(
                line_number,
                false,
                col,
                if self.output.lines.byte_offset() {
                    Some(byte_offset)
                } else {
                    None
                },
            );
            let _ = self.bytes.write_all(text.as_bytes());
            let _ = self.bytes.write_all(&[self.line_terminator]);
            true
        });
        true
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
                self.bytes.extend_from_slice(ANSI_PATH);
            }
            write!(self.bytes, "{}", self.display_path)?;
            if color {
                self.bytes.extend_from_slice(ANSI_RESET);
            }
            self.bytes.extend_from_slice(field_sep);
        }
        if self.show_line_numbers {
            if color {
                self.bytes.extend_from_slice(ANSI_LINE);
            }
            write!(self.bytes, "{}", line_number.unwrap_or(0))?;
            if color {
                self.bytes.extend_from_slice(ANSI_RESET);
            }
            self.bytes.extend_from_slice(field_sep);
        }
        if let Some(col) = column {
            if color {
                self.bytes.extend_from_slice(ANSI_LINE);
            }
            write!(self.bytes, "{col}")?;
            if color {
                self.bytes.extend_from_slice(ANSI_RESET);
            }
            self.bytes.extend_from_slice(field_sep);
        }
        if let Some(offset) = byte_offset {
            if color {
                self.bytes.extend_from_slice(ANSI_LINE);
            }
            let sep = if is_context_line { '-' } else { ':' };
            write!(self.bytes, "{offset}{sep}")?;
            if color {
                self.bytes.extend_from_slice(ANSI_RESET);
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
            self.bytes.write_all(line_bytes)?;
            if !line_bytes.ends_with(&[self.line_terminator]) {
                self.bytes.write_all(&[self.line_terminator])?;
            }
        }
        Ok(true)
    }
}

struct StandardWorker<'a> {
    matcher: &'a RegexMatcher,
    searcher: Searcher,
    output: SearchOutput,
    separators: &'a SearchSeparators,
    bytes: Vec<u8>,
    match_counter: Option<&'a AtomicUsize>,
    files_with_matches: Option<&'a AtomicUsize>,
    replace: Option<String>,
    sink_config: SinkConfig,
    records: SearchRecordStyle,
    lines_flags: LineStyleFlags,
    filename_mode: FilenameMode,
    path_display: crate::search::output::style::PathDisplay,
    path_separator: Option<u8>,
    emission: OutputEmission,
    collect_hits: bool,
    line_terminator: u8,
}

impl<'a> StandardWorker<'a> {
    fn new(scan: &'a StandardScan<'a>, collect: SearchCollection) -> Self {
        Self {
            searcher: scan.search.build_searcher(
                scan.output.lines.line_number(),
                scan.search.opts().max_results,
                true,
            ),
            matcher: scan.matcher,
            output: scan.output,
            separators: scan.separators,
            bytes: Vec::new(),
            match_counter: scan.counters.primary(),
            files_with_matches: scan.counters.files_with_matches(),
            replace: scan.search.opts().replace.clone(),
            sink_config: SinkConfig {
                before_context: scan.search.opts().before_context,
                after_context: scan.search.opts().after_context,
            },
            records: scan.output.records,
            lines_flags: scan.output.lines.flags,
            filename_mode: scan.output.lines.filename_mode,
            path_display: scan.output.lines.path_display,
            path_separator: scan.output.records.path_separator,
            emission: scan.output.emission,
            collect_hits: collect.hits,
            line_terminator: scan.search.opts().line_terminator(),
        }
    }

    fn search_candidate(&mut self, candidate: &Candidate, stop: &AtomicBool) -> FileResult {
        self.bytes.clear();
        if stop.load(Ordering::SeqCst) {
            return FileResult {
                output: ChunkOutput::empty(),
                json_stats: None,
                hit: None,
            };
        }

        let matched = {
            let heading = self.lines_flags.contains(LineStyleFlags::HEADING)
                && self.filename_mode != FilenameMode::Never;
            let mut sink_output = self.output;
            if heading {
                sink_output.lines.filename_mode = FilenameMode::Never;
            }
            let display = candidate.display_path(self.path_display, self.path_separator);
            let mut sink = StandardSink::new(
                self.matcher,
                sink_output,
                display,
                &mut self.bytes,
                self.separators,
                self.replace.as_deref(),
                self.sink_config,
            )
            .with_line_terminator(self.line_terminator);
            let _ = self
                .searcher
                .search_path(self.matcher, candidate.abs_path(), &mut sink);
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
                        out.extend_from_slice(ANSI_PATH);
                    }
                    let display = candidate.display_path(self.path_display, self.path_separator);
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
            hit: (matched && self.collect_hits).then(|| candidate.rel_path().to_path_buf()),
        }
    }
}

pub struct StandardScan<'a> {
    search: &'a SearchQuery,
    matcher: &'a RegexMatcher,
    output: SearchOutput,
    separators: &'a SearchSeparators,
    counters: &'a TextStatsCounters,
}

impl<'a> StandardScan<'a> {
    pub const fn new(
        search: &'a SearchQuery,
        matcher: &'a RegexMatcher,
        output: SearchOutput,
        separators: &'a SearchSeparators,
        counters: &'a TextStatsCounters,
    ) -> Self {
        Self {
            search,
            matcher,
            output,
            separators,
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
                || StandardWorker::new(self, collect),
                |worker: &mut StandardWorker<'_>, candidate: &Candidate| {
                    worker.search_candidate(candidate, &stop)
                },
            )
            .collect_into_vec(&mut files);
        let mut hits = Vec::new();
        let mut outputs = Vec::with_capacity(files.len());
        for file in files {
            if collect.hits
                && let Some(hit) = file.hit
            {
                hits.push(hit);
            }
            outputs.push(file.output);
        }
        let any_match = ChunkOutput::flush_all(outputs, self.counters.bytes_printed())?;
        Ok((any_match, hits))
    }
}

fn apply_replace(matcher: &RegexMatcher, line: &[u8], replacement: &str) -> String {
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

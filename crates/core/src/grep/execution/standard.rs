use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use grep_matcher::{Captures, Matcher};
use grep_regex::RegexMatcher;
use grep_searcher::{Searcher, Sink, SinkContext, SinkMatch};
use rayon::prelude::*;

use crate::grep::execution::format::{
    ANSI_LINE, ANSI_PATH, ANSI_RESET, ColumnAction, check_max_columns, display_path_for_candidate,
    should_color, truncate_line, write_line_terminator,
};
use crate::grep::execution::stats::StatsCollection;
use crate::grep::filter::CandidateInfo;
use crate::grep::output::SearchOutput;
use crate::grep::output::mode::OutputEmission;
use crate::grep::output::style::{LineStyleFlags, SearchSeparators};
use crate::grep::search::CompiledSearch;

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
        }
    }
}

impl Sink for StandardSink<'_> {
    type Error = io::Error;

    fn matched(&mut self, _: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, Self::Error> {
        self.matched = true;
        self.match_count += 1;

        if self.output.emission == OutputEmission::Quiet {
            return Ok(true);
        }

        if matches!(
            self.output.mode,
            crate::grep::output::mode::SearchMode::OnlyMatching
        ) {
            return Ok(self.handle_only_matching(mat));
        }

        let line_bytes = mat.bytes();
        if self.handle_max_columns(line_bytes, mat)? {
            return Ok(true);
        }

        let col = self.compute_first_column(mat);
        write_standard_prefix(
            self.bytes,
            self.output,
            self.display_path.as_str(),
            mat.line_number(),
            self.show_line_numbers,
            &PrefixCtx {
                is_context_line: false,
                column: col,
                separators: self.separators,
            },
            if self.output.lines.byte_offset() {
                Some(mat.absolute_byte_offset())
            } else {
                None
            },
        )?;
        self.write_line_content(mat.bytes())
    }

    fn context(&mut self, _: &Searcher, ctx: &SinkContext<'_>) -> Result<bool, Self::Error> {
        if self.output.emission == OutputEmission::Quiet {
            return Ok(true);
        }
        if matches!(
            self.output.mode,
            crate::grep::output::mode::SearchMode::OnlyMatching
        ) {
            return Ok(true);
        }
        let ctx_bytes = ctx.bytes();
        let columns = self.output.lines.columns;
        match check_max_columns(ctx_bytes, columns) {
            ColumnAction::Omit => return Ok(true),
            ColumnAction::Preview => {
                let limit = columns.map_or(0, |c| c.max);
                write_standard_prefix(
                    self.bytes,
                    self.output,
                    self.display_path.as_str(),
                    ctx.line_number(),
                    self.show_line_numbers,
                    &PrefixCtx {
                        is_context_line: true,
                        column: None,
                        separators: self.separators,
                    },
                    if self.output.lines.byte_offset() {
                        Some(ctx.absolute_byte_offset())
                    } else {
                        None
                    },
                )?;
                let truncated = truncate_line(ctx_bytes, limit);
                self.bytes.write_all(&truncated)?;
                return Ok(true);
            }
            ColumnAction::Normal => {}
        }
        write_standard_prefix(
            self.bytes,
            self.output,
            self.display_path.as_str(),
            ctx.line_number(),
            self.show_line_numbers,
            &PrefixCtx {
                is_context_line: true,
                column: None,
                separators: self.separators,
            },
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
            if !trimmed.ends_with('\n') {
                self.bytes.write_all(b"\n")?;
            }
        } else {
            self.bytes.write_all(line_bytes)?;
            if !line_bytes.ends_with(b"\n") {
                self.bytes.write_all(b"\n")?;
            }
        }
        Ok(true)
    }

    fn context_break(&mut self, _: &Searcher) -> Result<bool, Self::Error> {
        if self.output.emission == OutputEmission::Quiet {
            return Ok(true);
        }
        if matches!(
            self.output.mode,
            crate::grep::output::mode::SearchMode::OnlyMatching
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
            let _ = write_standard_prefix(
                self.bytes,
                self.output,
                self.display_path.as_str(),
                line_number,
                self.show_line_numbers,
                &PrefixCtx {
                    is_context_line: false,
                    column: col,
                    separators: self.separators,
                },
                if self.output.lines.byte_offset() {
                    Some(byte_offset)
                } else {
                    None
                },
            );
            let _ = self.bytes.write_all(text.as_bytes());
            let _ = self.bytes.write_all(b"\n");
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
        match check_max_columns(line_bytes, columns) {
            ColumnAction::Omit => return Ok(true),
            ColumnAction::Preview => {
                let limit = columns.map_or(0, |c| c.max);
                write_standard_prefix(
                    self.bytes,
                    self.output,
                    self.display_path.as_str(),
                    mat.line_number(),
                    self.show_line_numbers,
                    &PrefixCtx {
                        is_context_line: false,
                        column: None,
                        separators: self.separators,
                    },
                    if self.output.lines.byte_offset() {
                        Some(mat.absolute_byte_offset())
                    } else {
                        None
                    },
                )?;
                let truncated = truncate_line(line_bytes, limit);
                self.bytes.write_all(&truncated)?;
                return Ok(true);
            }
            ColumnAction::Normal => {}
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

    fn write_line_content(&mut self, line_bytes: &[u8]) -> Result<bool, io::Error> {
        if let Some(rep) = self.replace {
            let text = apply_replace(self.matcher, line_bytes, rep);
            self.bytes.write_all(text.as_bytes())?;
            if !text.ends_with('\n') {
                self.bytes.write_all(b"\n")?;
            }
        } else if self.trim {
            let trimmed = String::from_utf8_lossy(line_bytes);
            let trimmed = trimmed.trim_start();
            self.bytes.write_all(trimmed.as_bytes())?;
            if !trimmed.ends_with('\n') {
                self.bytes.write_all(b"\n")?;
            }
        } else {
            self.bytes.write_all(line_bytes)?;
            if !line_bytes.ends_with(b"\n") {
                self.bytes.write_all(b"\n")?;
            }
        }
        Ok(true)
    }
}

pub struct StandardWorker<'a> {
    matcher: &'a RegexMatcher,
    searcher: Searcher,
    output: SearchOutput,
    separators: &'a SearchSeparators,
    bytes: Vec<u8>,
    match_counter: Option<&'a AtomicUsize>,
    files_with_matches: Option<&'a AtomicUsize>,
    replace: Option<String>,
    sink_config: SinkConfig,
}

impl<'a> StandardWorker<'a> {
    pub fn new(
        search: &CompiledSearch,
        matcher: &'a RegexMatcher,
        output: SearchOutput,
        separators: &'a SearchSeparators,
        match_counter: Option<&'a AtomicUsize>,
        files_with_matches: Option<&'a AtomicUsize>,
    ) -> Self {
        Self {
            searcher: search.build_searcher(
                output.lines.line_number(),
                search.opts.max_results,
                true,
            ),
            matcher,
            output,
            separators,
            bytes: Vec::new(),
            match_counter,
            files_with_matches,
            replace: search.opts.replace.clone(),
            sink_config: SinkConfig {
                before_context: search.opts.before_context,
                after_context: search.opts.after_context,
            },
        }
    }

    pub fn search_candidate(
        &mut self,
        candidate: &CandidateInfo,
        result_index: usize,
        stop: &AtomicBool,
    ) -> FileResult {
        self.bytes.clear();
        if stop.load(Ordering::SeqCst) {
            return FileResult {
                index: result_index,
                output: ChunkOutput::empty(),
                json_stats: None,
            };
        }

        let matched = {
            let heading = self.output.lines.heading()
                && self.output.lines.filename_mode
                    != crate::grep::output::style::FilenameMode::Never;
            let mut sink_output = self.output;
            if heading {
                sink_output.lines.filename_mode = crate::grep::output::style::FilenameMode::Never;
            }
            let display = display_path_for_candidate(
                candidate,
                self.output.lines.path_display,
                self.output.records.path_separator,
            );
            let mut sink = StandardSink::new(
                self.matcher,
                sink_output,
                display,
                &mut self.bytes,
                self.separators,
                self.replace.as_deref(),
                self.sink_config,
            );
            let _ = self
                .searcher
                .search_path(self.matcher, &candidate.abs_path, &mut sink);
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

        if self.output.emission == OutputEmission::Quiet && matched {
            stop.store(true, Ordering::SeqCst);
        }

        FileResult {
            index: result_index,
            output: ChunkOutput {
                bytes: if matched
                    && self.output.lines.heading()
                    && self.output.lines.filename_mode
                        != crate::grep::output::style::FilenameMode::Never
                    && self.output.emission != OutputEmission::Quiet
                {
                    let mut out = Vec::new();
                    if should_color(self.output.records) {
                        out.extend_from_slice(ANSI_PATH);
                    }
                    let display = display_path_for_candidate(
                        candidate,
                        self.output.lines.path_display,
                        self.output.records.path_separator,
                    );
                    let _ = write!(out, "{display}");
                    if should_color(self.output.records) {
                        out.extend_from_slice(ANSI_RESET);
                    }
                    write_line_terminator(&mut out, self.output.records.terminator);
                    out.extend(std::mem::take(&mut self.bytes));
                    out
                } else {
                    std::mem::take(&mut self.bytes)
                },
                matched,
                heading: matched
                    && self.output.lines.heading()
                    && self.output.lines.filename_mode
                        != crate::grep::output::style::FilenameMode::Never
                    && self.output.emission != OutputEmission::Quiet,
            },
            json_stats: None,
        }
    }
}

pub struct FileResult {
    pub index: usize,
    pub output: ChunkOutput,
    pub json_stats: Option<grep_printer::Stats>,
}

pub struct ChunkOutput {
    pub bytes: Vec<u8>,
    pub matched: bool,
    pub heading: bool,
}

impl ChunkOutput {
    pub const fn empty() -> Self {
        Self {
            bytes: Vec::new(),
            matched: false,
            heading: false,
        }
    }
}

pub fn flush_chunk_output(
    outputs: impl IntoIterator<Item = ChunkOutput>,
    bytes_printed: Option<&AtomicU64>,
) -> crate::Result<bool> {
    let mut stdout = io::stdout().lock();
    let mut any_match = false;
    let mut emitted = false;
    for output in outputs {
        any_match |= output.matched;
        if output.bytes.is_empty() {
            continue;
        }
        if output.heading && emitted {
            stdout.write_all(b"\n")?;
            if let Some(p) = bytes_printed {
                p.fetch_add(1, Ordering::Relaxed);
            }
        }
        let n = output.bytes.len() as u64;
        if let Some(p) = bytes_printed {
            p.fetch_add(n, Ordering::Relaxed);
        }
        stdout.write_all(&output.bytes)?;
        emitted = true;
    }
    Ok(any_match)
}

pub fn run_standard_with_info(
    search: &CompiledSearch,
    candidates: &[CandidateInfo],
    matcher: &RegexMatcher,
    output: SearchOutput,
    separators: &SearchSeparators,
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
                StandardWorker::new(
                    search,
                    matcher,
                    output,
                    separators,
                    stats.primary,
                    stats.files_with_matches,
                )
            },
            |worker: &mut StandardWorker<'_>,
             (result_index, candidate): (usize, &CandidateInfo)| {
                worker.search_candidate(candidate, result_index, &stop)
            },
        )
        .collect_into_vec(&mut files);
    files.sort_by_key(|file| file.index);
    flush_chunk_output(
        files.into_iter().map(|file| file.output),
        stats.bytes_printed,
    )
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

pub struct PrefixCtx<'a> {
    pub is_context_line: bool,
    pub column: Option<usize>,
    pub separators: &'a SearchSeparators,
}

pub fn write_standard_prefix(
    out: &mut Vec<u8>,
    output: SearchOutput,
    path: &str,
    line_number: Option<u64>,
    show_line_numbers: bool,
    prefix: &PrefixCtx<'_>,
    byte_offset: Option<u64>,
) -> io::Result<()> {
    let color = should_color(output.records);
    let print_filename =
        output.lines.filename_mode != crate::grep::output::style::FilenameMode::Never;
    let field_sep = if prefix.is_context_line {
        &prefix.separators.field_context_separator
    } else {
        &prefix.separators.field_match_separator
    };
    if print_filename {
        if color {
            out.extend_from_slice(ANSI_PATH);
        }
        write!(out, "{path}")?;
        if color {
            out.extend_from_slice(ANSI_RESET);
        }
        out.extend_from_slice(field_sep);
    }
    if show_line_numbers {
        if color {
            out.extend_from_slice(ANSI_LINE);
        }
        write!(out, "{}", line_number.unwrap_or(0))?;
        if color {
            out.extend_from_slice(ANSI_RESET);
        }
        out.extend_from_slice(field_sep);
    }
    if let Some(col) = prefix.column {
        if color {
            out.extend_from_slice(ANSI_LINE);
        }
        write!(out, "{col}")?;
        if color {
            out.extend_from_slice(ANSI_RESET);
        }
        out.extend_from_slice(field_sep);
    }
    if let Some(offset) = byte_offset {
        if color {
            out.extend_from_slice(ANSI_LINE);
        }
        let sep = if prefix.is_context_line { '-' } else { ':' };
        write!(out, "{offset}{sep}")?;
        if color {
            out.extend_from_slice(ANSI_RESET);
        }
    }
    Ok(())
}

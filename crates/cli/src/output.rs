use clap::{ArgAction, Args};
use sift_core::{
    ColorChoice, FilenameMode, LineStyleFlags, OutputEmission, SearchLineStyle, SearchMode,
    SearchOutput, SearchOutputFormat, SearchRecordStyle, SearchSeparators, SearchStats,
};

use crate::cli::Cli;

// ── Clap declarations (output flags) ──

#[derive(Args)]
pub struct SeparatorDecl {
    #[arg(
        long = "context-separator",
        value_name = "SEPARATOR",
        allow_hyphen_values = true
    )]
    pub context_sep: Option<String>,
    #[arg(long = "no-context-separator")]
    pub suppress_context_sep: bool,
    #[arg(long = "field-match-separator", value_name = "SEPARATOR")]
    pub field_match: Option<String>,
    #[arg(long = "field-context-separator", value_name = "SEPARATOR")]
    pub field_context: Option<String>,
}

#[derive(Args)]
pub struct LineNumberDecl {
    #[arg(short = 'n', long = "line-number")]
    pub line_number: bool,
    #[arg(short = 'N', long = "no-line-number")]
    pub no_line_number: bool,
}

#[derive(Args)]
pub struct FilenameDecl {
    #[arg(short = 'I', long = "no-filename")]
    pub no_filename: bool,
    #[arg(short = 'H', long = "with-filename")]
    pub with_filename: bool,
}

#[derive(Args)]
pub struct HeadingDecl {
    #[arg(long = "heading")]
    pub heading: bool,
    #[arg(long = "no-heading")]
    pub no_heading: bool,
}

#[derive(Args)]
pub struct ColumnDecl {
    #[arg(long = "column")]
    pub column: bool,
    #[arg(long = "vimgrep")]
    pub vimgrep: bool,
    #[arg(short = 'p', long = "pretty")]
    pub pretty: bool,
}

#[derive(Args)]
pub struct ColumnsDecl {
    #[arg(short = 'M', long = "max-columns", value_name = "NUM")]
    pub max_columns: Option<u64>,
    #[arg(long = "max-columns-preview")]
    pub max_columns_preview: bool,
}

#[derive(Args)]
pub struct ReplaceDecl {
    #[arg(short = 'r', long = "replace", value_name = "REPLACEMENT")]
    pub replace: Option<String>,
    #[arg(long = "trim")]
    pub trim: bool,
    #[arg(long = "passthru", visible_alias = "passthrough")]
    pub passthru: bool,
}

#[derive(Args)]
pub struct ExtraOutputDecl {
    #[arg(long = "include-zero")]
    pub include_zero: bool,
    #[arg(short = 'b', long = "byte-offset")]
    pub byte_offset: bool,
}

/// `-0` / `--null` and `--color` for clap; effective null/color use argv resolvers.
#[derive(Args)]
pub struct NullColorDecl {
    #[arg(short = '0', long = "null", action = ArgAction::SetTrue)]
    pub _null: bool,
    #[arg(long = "color", value_name = "WHEN")]
    pub _color: Option<String>,
}

/// Declares `--json` / `--no-json` for clap.
#[derive(Args)]
pub struct JsonDecl {
    #[arg(long = "json", action = ArgAction::SetTrue)]
    pub _json: bool,
    #[arg(long = "no-json", action = ArgAction::SetTrue)]
    pub _no_json: bool,
}

/// Declares `--stats` for clap.
#[derive(Args)]
pub struct StatsDecl {
    #[arg(long = "stats", action = ArgAction::SetTrue)]
    pub _stats: bool,
}

// ── Context types for output configuration ──

#[derive(Clone, Copy)]
pub struct SearchModeCtx {
    pub effective_mode: SearchMode,
    pub quiet: bool,
}

#[derive(Clone, Copy)]
pub struct SearchLineResolveCtx {
    pub heading: bool,
    pub with_filename: Option<bool>,
    pub is_path_mode: bool,
    pub column: bool,
    pub line_number: Option<bool>,
}

#[derive(Clone, Copy)]
pub struct SearchFormatCtx {
    pub null_data: bool,
    pub color: ColorChoice,
}

#[derive(Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct SearchOutputCtx {
    pub mode: SearchModeCtx,
    pub lines: SearchLineResolveCtx,
    pub format: SearchFormatCtx,
    pub output_format: SearchOutputFormat,
    pub separators: SearchSeparators,
    pub print_stats: bool,
    pub byte_offset: bool,
    pub trim: bool,
    pub include_zero: bool,
    pub path_separator: Option<u8>,
    pub max_columns: Option<u64>,
    pub max_columns_preview: bool,
}

// ── Argv-order resolvers ──

pub fn parse_usize_token(s: &str) -> Option<usize> {
    s.parse().ok()
}

pub fn resolve_glob_case_insensitive_from_args(args: &[String]) -> bool {
    let mut last_idx = 0usize;
    let mut result = false;
    for (i, arg) in args.iter().enumerate() {
        let bytes = arg.as_bytes();
        let is_long = bytes.len() > 2 && bytes[0] == b'-' && bytes[1] == b'-';
        if is_long {
            let suffix = &bytes[2..];
            let flag = if suffix == b"glob-case-insensitive" {
                Some((i, true))
            } else if suffix == b"no-glob-case-insensitive" {
                Some((i, false))
            } else {
                None
            };
            if let Some((idx, val)) = flag
                && idx > last_idx
            {
                last_idx = idx;
                result = val;
            }
        }
    }
    result
}

pub fn parse_color_when(s: &str) -> ColorChoice {
    match s {
        "never" => ColorChoice::Never,
        "always" => ColorChoice::Always,
        _ => ColorChoice::Auto,
    }
}

pub fn resolve_null_from_args(args: &[String]) -> bool {
    let mut result = false;
    for arg in args {
        match arg.as_str() {
            "-0" | "--null" => result = true,
            _ => {}
        }
    }
    result
}

pub fn resolve_color_from_args(args: &[String]) -> ColorChoice {
    let mut result = ColorChoice::Auto;
    let mut i = 0usize;
    while i < args.len() {
        if let Some(rest) = args[i].strip_prefix("--color=") {
            result = parse_color_when(rest);
            i += 1;
            continue;
        }
        if args[i] == "--color"
            && let Some(v) = args.get(i + 1)
        {
            result = parse_color_when(v);
            i += 2;
            continue;
        }
        i += 1;
    }
    result
}

pub fn resolve_stats_from_args(args: &[String]) -> bool {
    let mut last_idx = 0usize;
    let mut result = false;
    for (i, arg) in args.iter().enumerate() {
        if arg == "--stats" && i >= last_idx {
            last_idx = i;
            result = true;
        }
    }
    result
}

pub fn resolve_json_from_args(args: &[String]) -> bool {
    let mut last_idx = 0usize;
    let mut result = false;
    for (i, arg) in args.iter().enumerate() {
        if arg == "--json" && i >= last_idx {
            last_idx = i;
            result = true;
        }
        if arg == "--no-json" && i >= last_idx {
            last_idx = i;
            result = false;
        }
    }
    result
}

pub fn resolve_heading_from_args(args: &[String]) -> bool {
    let mut last_idx = 0usize;
    let mut result = false;
    for (i, arg) in args.iter().enumerate() {
        let bytes = arg.as_bytes();
        if bytes.len() > 2 && bytes[0] == b'-' && bytes[1] == b'-' {
            let value = match &bytes[2..] {
                b"heading" => Some(true),
                b"no-heading" => Some(false),
                _ => None,
            };
            if let Some(value) = value
                && i > last_idx
            {
                last_idx = i;
                result = value;
            }
        }
    }
    result
}

pub fn resolve_line_number_from_args(args: &[String]) -> Option<bool> {
    let mut last_idx = 0usize;
    let mut result = None;
    for (i, arg) in args.iter().enumerate() {
        let bytes = arg.as_bytes();
        let value = if bytes.len() == 2 && bytes[0] == b'-' {
            match bytes.get(1) {
                Some(&b'n') => Some(true),
                Some(&b'N') => Some(false),
                _ => None,
            }
        } else if bytes.len() > 2 && bytes[0] == b'-' && bytes[1] == b'-' {
            match &bytes[2..] {
                b"line-number" => Some(true),
                b"no-line-number" => Some(false),
                _ => None,
            }
        } else {
            None
        };
        if let Some(value) = value
            && i > last_idx
        {
            last_idx = i;
            result = Some(value);
        }
    }
    result
}

pub fn resolve_with_filename_from_args(args: &[String]) -> Option<bool> {
    let mut last_idx = 0usize;
    let mut result = None;
    for (i, arg) in args.iter().enumerate() {
        let bytes = arg.as_bytes();
        let value = if bytes.len() == 2 && bytes[0] == b'-' {
            match bytes[1] {
                b'H' => Some(true),
                b'I' => Some(false),
                _ => None,
            }
        } else if bytes.len() > 2 && bytes[0] == b'-' && bytes[1] == b'-' {
            match &bytes[2..] {
                b"with-filename" => Some(true),
                b"no-filename" => Some(false),
                _ => None,
            }
        } else {
            None
        };
        if let Some(value) = value
            && i > last_idx
        {
            last_idx = i;
            result = Some(value);
        }
    }
    result
}

/// `-A` / `-B` / `-C` and long forms; argv order with later flags overriding (ripgrep-style).
pub fn resolve_context_from_args(args: &[String]) -> (usize, usize) {
    let mut before = 0usize;
    let mut after = 0usize;
    let mut i = 0usize;
    while i < args.len() {
        let arg = args[i].as_str();
        if let Some(rest) = arg.strip_prefix("--after-context=") {
            if let Some(n) = parse_usize_token(rest) {
                after = n;
            }
            i += 1;
            continue;
        }
        if let Some(rest) = arg.strip_prefix("--before-context=") {
            if let Some(n) = parse_usize_token(rest) {
                before = n;
            }
            i += 1;
            continue;
        }
        if let Some(rest) = arg.strip_prefix("--context=") {
            if let Some(n) = parse_usize_token(rest) {
                before = n;
                after = n;
            }
            i += 1;
            continue;
        }
        match arg {
            "-A" | "--after-context" => {
                if let Some(n) = args.get(i + 1).and_then(|s| parse_usize_token(s)) {
                    after = n;
                    i += 2;
                    continue;
                }
            }
            "-B" | "--before-context" => {
                if let Some(n) = args.get(i + 1).and_then(|s| parse_usize_token(s)) {
                    before = n;
                    i += 2;
                    continue;
                }
            }
            "-C" | "--context" => {
                if let Some(n) = args.get(i + 1).and_then(|s| parse_usize_token(s)) {
                    before = n;
                    after = n;
                    i += 2;
                    continue;
                }
            }
            _ => {}
        }
        if let Some(body) = arg.strip_prefix("-A")
            && !body.is_empty()
            && let Some(n) = parse_usize_token(body)
        {
            after = n;
            i += 1;
            continue;
        }
        if let Some(body) = arg.strip_prefix("-B")
            && !body.is_empty()
            && let Some(n) = parse_usize_token(body)
        {
            before = n;
            i += 1;
            continue;
        }
        if let Some(body) = arg.strip_prefix("-C")
            && !body.is_empty()
            && let Some(n) = parse_usize_token(body)
        {
            before = n;
            after = n;
            i += 1;
            continue;
        }
        i += 1;
    }
    (before, after)
}

// ── Output helpers ──

pub const fn search_output(
    format: SearchOutputFormat,
    effective_mode: SearchMode,
    quiet: bool,
    lines: SearchLineStyle,
    records: SearchRecordStyle,
    include_zero: bool,
) -> SearchOutput {
    SearchOutput {
        format,
        mode: effective_mode,
        emission: if quiet {
            OutputEmission::Quiet
        } else {
            OutputEmission::Normal
        },
        lines,
        records,
        passthru: false,
        include_zero,
    }
}

pub fn build_line_style_flags(out: &SearchOutputCtx, line_number: bool) -> LineStyleFlags {
    let mut f = LineStyleFlags::empty();
    if out.lines.heading {
        f |= LineStyleFlags::HEADING;
    }
    if line_number {
        f |= LineStyleFlags::LINE_NUMBER;
    }
    if out.lines.column {
        f |= LineStyleFlags::COLUMN;
    }
    if out.byte_offset {
        f |= LineStyleFlags::BYTE_OFFSET;
    }
    if out.trim {
        f |= LineStyleFlags::TRIM;
    }
    f
}

pub const fn effective_filename_mode(
    with_filename: Option<bool>,
    is_path_mode: bool,
    corpus_is_single_file: bool,
) -> FilenameMode {
    if is_path_mode || matches!(with_filename, Some(true)) {
        FilenameMode::Always
    } else if matches!(with_filename, Some(false)) || corpus_is_single_file {
        FilenameMode::Never
    } else {
        FilenameMode::Always
    }
}

pub const fn resolve_effective_line_number(
    clap_line_number: bool,
    line_number_override: Option<bool>,
    output_format: SearchOutputFormat,
) -> bool {
    if matches!(output_format, SearchOutputFormat::Json) {
        return true;
    }
    match line_number_override {
        Some(val) => val,
        None => clap_line_number,
    }
}

pub fn write_search_stats(stats: &SearchStats) {
    eprintln!("{} matches", stats.matches);
    eprintln!("{} files contained matches", stats.files_with_matches);
    eprintln!("{} files searched", stats.files_searched);
    eprintln!("{} bytes printed", stats.bytes_printed);
    eprintln!("{} bytes searched", stats.bytes_searched);
    eprintln!("{:.6}s elapsed", stats.elapsed.as_secs_f64());
}

pub fn unescape_separator(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push(b'\n'),
                Some('t') => out.push(b'\t'),
                Some('\\') | None => out.push(b'\\'),
                Some('0') => out.push(0),
                Some(other) => {
                    out.push(b'\\');
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(other.encode_utf8(&mut buf).as_bytes());
                }
            }
        } else {
            let mut buf = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        }
    }
    out
}

// ── Cli method implementations ──

impl Cli {
    pub fn resolve_separators(&self) -> SearchSeparators {
        let context_separator = if self.separator_decl.suppress_context_sep {
            None
        } else if let Some(ref s) = self.separator_decl.context_sep {
            Some(unescape_separator(s))
        } else {
            Some(b"--".to_vec())
        };
        let field_match_separator = self
            .separator_decl
            .field_match
            .as_ref()
            .map_or_else(|| b":".to_vec(), |s| unescape_separator(s));
        let field_context_separator = self
            .separator_decl
            .field_context
            .as_ref()
            .map_or_else(|| b"-".to_vec(), |s| unescape_separator(s));
        SearchSeparators {
            context_separator,
            field_match_separator,
            field_context_separator,
        }
    }

    pub fn build_output_and_filter(
        &self,
        args: &[String],
        effective_mode: SearchMode,
        quiet: bool,
        line_number_override: Option<bool>,
    ) -> (SearchOutputCtx, crate::filter::SearchFilterCtx) {
        let glob_case_insensitive = resolve_glob_case_insensitive_from_args(args);
        let ignore_res = crate::ignore::resolve_visibility_and_ignore(args);
        let null_data = resolve_null_from_args(args);
        let color = resolve_color_from_args(args);
        let heading = resolve_heading_from_args(args);
        let with_filename = resolve_with_filename_from_args(args);
        let use_json = resolve_json_from_args(args);
        let print_stats = resolve_stats_from_args(args) || use_json;

        let pretty = self.column_decl.pretty;
        let vimgrep = self.column_decl.vimgrep;
        let column = self.column_decl.column || vimgrep;

        let is_path_mode = matches!(
            effective_mode,
            SearchMode::FilesWithMatches | SearchMode::FilesWithoutMatch
        );
        let effective_heading = heading || pretty;
        let effective_color = if pretty && color == ColorChoice::Auto {
            ColorChoice::Always
        } else {
            color
        };

        let out = SearchOutputCtx {
            mode: SearchModeCtx {
                effective_mode,
                quiet,
            },
            lines: SearchLineResolveCtx {
                heading: effective_heading,
                with_filename,
                is_path_mode,
                column,
                line_number: line_number_override,
            },
            format: SearchFormatCtx {
                null_data,
                color: effective_color,
            },
            output_format: if use_json {
                SearchOutputFormat::Json
            } else {
                SearchOutputFormat::Text
            },
            separators: self.resolve_separators(),
            print_stats,
            byte_offset: self.extra_output.byte_offset,
            trim: self.replace_decl.trim,
            include_zero: self.extra_output.include_zero,
            path_separator: self
                .threading
                .path_separator
                .as_ref()
                .and_then(|s| s.as_bytes().first().copied()),
            max_columns: self.columns_decl.max_columns,
            max_columns_preview: self.columns_decl.max_columns_preview,
        };
        let filter = crate::filter::SearchFilterCtx {
            hidden: ignore_res.hidden,
            ignore_sources: ignore_res.sources,
            require_git: ignore_res.require_git,
            glob_case_insensitive,
            msg_flags: ignore_res.msg_flags,
        };
        (out, filter)
    }
}

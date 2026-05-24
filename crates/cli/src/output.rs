use clap::{ArgAction, Args};
use sift_core::{
    ColorChoice, FilenameMode, LineStyleFlags, OutputEmission, PassthruMode, SearchLineStyle,
    SearchMode, SearchOutput, SearchOutputFormat, SearchRecordStyle, SearchSeparators, SearchStats,
    ZeroCountMode,
};

/// Describes the filename display context for deciding whether to show paths.
#[derive(Clone, Copy)]
pub enum FilenameContext {
    PathMode,
    DirectoryCorpus,
    SingleFileCorpus,
}

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
    pub null_flag: bool,
    #[arg(long = "color", value_name = "WHEN")]
    pub color_flag: Option<String>,
}

/// Declares `--json` / `--no-json` for clap.
#[derive(Args)]
pub struct JsonDecl {
    #[arg(long = "json", action = ArgAction::SetTrue)]
    pub json_flag: bool,
    #[arg(long = "no-json", action = ArgAction::SetTrue)]
    pub no_json_flag: bool,
}

/// Declares `--stats` for clap.
#[derive(Args)]
pub struct StatsDecl {
    #[arg(long = "stats", action = ArgAction::SetTrue)]
    pub stats_flag: bool,
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

#[must_use]
pub fn parse_usize_token(s: &str) -> Option<usize> {
    s.parse().ok()
}

#[must_use]
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

#[must_use]
pub fn parse_color_when(s: &str) -> ColorChoice {
    match s {
        "never" => ColorChoice::Never,
        "always" => ColorChoice::Always,
        _ => ColorChoice::Auto,
    }
}

#[must_use]
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

#[must_use]
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

#[must_use]
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

#[must_use]
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

#[must_use]
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

#[must_use]
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

#[must_use]
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
#[must_use]
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

#[must_use]
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
        passthru: PassthruMode::Disabled,
        include_zero: if include_zero {
            ZeroCountMode::Include
        } else {
            ZeroCountMode::Omit
        },
    }
}

#[must_use]
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

#[must_use]
pub const fn effective_filename_mode(
    with_filename: Option<bool>,
    context: FilenameContext,
) -> FilenameMode {
    if matches!(context, FilenameContext::PathMode) || matches!(with_filename, Some(true)) {
        FilenameMode::Always
    } else if matches!(with_filename, Some(false))
        || matches!(context, FilenameContext::SingleFileCorpus)
    {
        FilenameMode::Never
    } else {
        FilenameMode::Always
    }
}

#[must_use]
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

#[must_use]
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
    #[must_use]
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

    #[must_use]
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use sift_core::RecordTerminator;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(ToString::to_string).collect()
    }

    // ── parse_usize_token ──

    #[test]
    fn parse_usize_token_valid() {
        assert_eq!(parse_usize_token("42"), Some(42));
    }

    #[test]
    fn parse_usize_token_zero() {
        assert_eq!(parse_usize_token("0"), Some(0));
    }

    #[test]
    fn parse_usize_token_invalid() {
        assert_eq!(parse_usize_token("abc"), None);
    }

    #[test]
    fn parse_usize_token_empty() {
        assert_eq!(parse_usize_token(""), None);
    }

    // ── parse_color_when ──

    #[test]
    fn parse_color_when_never() {
        assert!(matches!(parse_color_when("never"), ColorChoice::Never));
    }

    #[test]
    fn parse_color_when_always() {
        assert!(matches!(parse_color_when("always"), ColorChoice::Always));
    }

    #[test]
    fn parse_color_when_auto() {
        assert!(matches!(parse_color_when("auto"), ColorChoice::Auto));
    }

    #[test]
    fn parse_color_when_unknown_defaults_auto() {
        assert!(matches!(parse_color_when("xyz"), ColorChoice::Auto));
    }

    // ── resolve_null_from_args ──

    #[test]
    fn resolve_null_no_null() {
        assert!(!resolve_null_from_args(&args(&["sift", "pat"])));
    }

    #[test]
    fn resolve_null_short() {
        assert!(resolve_null_from_args(&args(&["sift", "-0", "pat"])));
    }

    #[test]
    fn resolve_null_long() {
        assert!(resolve_null_from_args(&args(&["sift", "--null", "pat"])));
    }

    // ── resolve_color_from_args ──

    #[test]
    fn resolve_color_default_auto() {
        assert!(matches!(
            resolve_color_from_args(&args(&["sift", "pat"])),
            ColorChoice::Auto
        ));
    }

    #[test]
    fn resolve_color_never_long() {
        assert!(matches!(
            resolve_color_from_args(&args(&["sift", "--color=never", "pat"])),
            ColorChoice::Never
        ));
    }

    #[test]
    fn resolve_color_always_long() {
        assert!(matches!(
            resolve_color_from_args(&args(&["sift", "--color=always", "pat"])),
            ColorChoice::Always
        ));
    }

    #[test]
    fn resolve_color_separate_arg() {
        assert!(matches!(
            resolve_color_from_args(&args(&["sift", "--color", "never", "pat"])),
            ColorChoice::Never
        ));
    }

    #[test]
    fn resolve_color_last_wins() {
        assert!(matches!(
            resolve_color_from_args(&args(&["sift", "--color=never", "--color=always", "pat"])),
            ColorChoice::Always
        ));
    }

    // ── resolve_stats_from_args ──

    #[test]
    fn resolve_stats_no_flag() {
        assert!(!resolve_stats_from_args(&args(&["sift", "pat"])));
    }

    #[test]
    fn resolve_stats_flag() {
        assert!(resolve_stats_from_args(&args(&["sift", "--stats", "pat"])));
    }

    // ── resolve_json_from_args ──

    #[test]
    fn resolve_json_no_flag() {
        assert!(!resolve_json_from_args(&args(&["sift", "pat"])));
    }

    #[test]
    fn resolve_json_flag() {
        assert!(resolve_json_from_args(&args(&["sift", "--json", "pat"])));
    }

    #[test]
    fn resolve_json_last_wins_true() {
        assert!(resolve_json_from_args(&args(&[
            "sift",
            "--no-json",
            "--json",
            "pat"
        ])));
    }

    #[test]
    fn resolve_json_last_wins_false() {
        assert!(!resolve_json_from_args(&args(&[
            "sift",
            "--json",
            "--no-json",
            "pat"
        ])));
    }

    // ── resolve_heading_from_args ──

    #[test]
    fn resolve_heading_no_flag() {
        assert!(!resolve_heading_from_args(&args(&["sift", "pat"])));
    }

    #[test]
    fn resolve_heading_flag() {
        assert!(resolve_heading_from_args(&args(&[
            "sift",
            "--heading",
            "pat"
        ])));
    }

    #[test]
    fn resolve_heading_no_heading_flag() {
        assert!(!resolve_heading_from_args(&args(&[
            "sift",
            "--no-heading",
            "pat"
        ])));
    }

    #[test]
    fn resolve_heading_last_wins() {
        assert!(!resolve_heading_from_args(&args(&[
            "sift",
            "--heading",
            "--no-heading",
            "pat"
        ])));
    }

    // ── resolve_line_number_from_args ──

    #[test]
    fn resolve_line_number_no_flag() {
        assert_eq!(resolve_line_number_from_args(&args(&["sift", "pat"])), None);
    }

    #[test]
    fn resolve_line_number_short_n() {
        assert_eq!(
            resolve_line_number_from_args(&args(&["sift", "-n", "pat"])),
            Some(true)
        );
    }

    #[test]
    fn resolve_line_number_short_n_upper() {
        assert_eq!(
            resolve_line_number_from_args(&args(&["sift", "-N", "pat"])),
            Some(false)
        );
    }

    #[test]
    fn resolve_line_number_long() {
        assert_eq!(
            resolve_line_number_from_args(&args(&["sift", "--line-number", "pat"])),
            Some(true)
        );
    }

    #[test]
    fn resolve_line_number_no_long() {
        assert_eq!(
            resolve_line_number_from_args(&args(&["sift", "--no-line-number", "pat"])),
            Some(false)
        );
    }

    #[test]
    fn resolve_line_number_last_wins() {
        assert_eq!(
            resolve_line_number_from_args(&args(&["sift", "-n", "-N", "pat"])),
            Some(false)
        );
    }

    // ── resolve_with_filename_from_args ──

    #[test]
    fn resolve_with_filename_no_flag() {
        assert_eq!(
            resolve_with_filename_from_args(&args(&["sift", "pat"])),
            None
        );
    }

    #[test]
    fn resolve_with_filename_short_h() {
        assert_eq!(
            resolve_with_filename_from_args(&args(&["sift", "-H", "pat"])),
            Some(true)
        );
    }

    #[test]
    fn resolve_with_filename_short_i() {
        assert_eq!(
            resolve_with_filename_from_args(&args(&["sift", "-I", "pat"])),
            Some(false)
        );
    }

    #[test]
    fn resolve_with_filename_long_with() {
        assert_eq!(
            resolve_with_filename_from_args(&args(&["sift", "--with-filename", "pat"])),
            Some(true)
        );
    }

    #[test]
    fn resolve_with_filename_long_no() {
        assert_eq!(
            resolve_with_filename_from_args(&args(&["sift", "--no-filename", "pat"])),
            Some(false)
        );
    }

    #[test]
    fn resolve_with_filename_last_wins() {
        assert_eq!(
            resolve_with_filename_from_args(&args(&["sift", "-H", "-I", "pat"])),
            Some(false)
        );
    }

    // ── resolve_context_from_args ──

    #[test]
    fn resolve_context_default() {
        assert_eq!(resolve_context_from_args(&args(&["sift", "pat"])), (0, 0));
    }

    #[test]
    fn resolve_context_after_short() {
        assert_eq!(
            resolve_context_from_args(&args(&["sift", "-A", "5", "pat"])),
            (0, 5)
        );
    }

    #[test]
    fn resolve_context_before_short() {
        assert_eq!(
            resolve_context_from_args(&args(&["sift", "-B", "3", "pat"])),
            (3, 0)
        );
    }

    #[test]
    fn resolve_context_both_short() {
        assert_eq!(
            resolve_context_from_args(&args(&["sift", "-C", "2", "pat"])),
            (2, 2)
        );
    }

    #[test]
    fn resolve_context_combined() {
        assert_eq!(
            resolve_context_from_args(&args(&["sift", "-B1", "-A2", "pat"])),
            (1, 2)
        );
    }

    #[test]
    fn resolve_context_last_wins() {
        assert_eq!(
            resolve_context_from_args(&args(&["sift", "-A1", "-A5", "pat"])),
            (0, 5)
        );
    }

    #[test]
    fn resolve_context_context_overrides_individual() {
        assert_eq!(
            resolve_context_from_args(&args(&["sift", "-A5", "-C2", "pat"])),
            (2, 2)
        );
    }

    #[test]
    fn resolve_context_after_long() {
        assert_eq!(
            resolve_context_from_args(&args(&["sift", "--after-context=5", "pat"])),
            (0, 5)
        );
    }

    #[test]
    fn resolve_context_before_long() {
        assert_eq!(
            resolve_context_from_args(&args(&["sift", "--before-context=3", "pat"])),
            (3, 0)
        );
    }

    #[test]
    fn resolve_context_context_long() {
        assert_eq!(
            resolve_context_from_args(&args(&["sift", "--context=2", "pat"])),
            (2, 2)
        );
    }

    #[test]
    fn resolve_context_inline_after() {
        assert_eq!(
            resolve_context_from_args(&args(&["sift", "-A5", "pat"])),
            (0, 5)
        );
    }

    #[test]
    fn resolve_context_inline_before() {
        assert_eq!(
            resolve_context_from_args(&args(&["sift", "-B3", "pat"])),
            (3, 0)
        );
    }

    #[test]
    fn resolve_context_inline_context() {
        assert_eq!(
            resolve_context_from_args(&args(&["sift", "-C2", "pat"])),
            (2, 2)
        );
    }

    // ── resolve_glob_case_insensitive_from_args ──

    #[test]
    fn glob_case_insensitive_default() {
        assert!(!resolve_glob_case_insensitive_from_args(&args(&[
            "sift", "pat"
        ])));
    }

    #[test]
    fn glob_case_insensitive_flag() {
        assert!(resolve_glob_case_insensitive_from_args(&args(&[
            "sift",
            "--glob-case-insensitive",
            "pat"
        ])));
    }

    #[test]
    fn glob_case_no_insensitive_flag() {
        assert!(!resolve_glob_case_insensitive_from_args(&args(&[
            "sift",
            "--no-glob-case-insensitive",
            "pat"
        ])));
    }

    #[test]
    fn glob_case_insensitive_last_wins() {
        assert!(!resolve_glob_case_insensitive_from_args(&args(&[
            "sift",
            "--glob-case-insensitive",
            "--no-glob-case-insensitive",
            "pat"
        ])));
    }

    // ── effective_filename_mode ──

    #[test]
    fn filename_mode_path_mode() {
        assert!(matches!(
            effective_filename_mode(None, FilenameContext::PathMode),
            FilenameMode::Always
        ));
    }

    #[test]
    fn filename_mode_with_filename_true() {
        assert!(matches!(
            effective_filename_mode(Some(true), FilenameContext::DirectoryCorpus),
            FilenameMode::Always
        ));
    }

    #[test]
    fn filename_mode_with_filename_false() {
        assert!(matches!(
            effective_filename_mode(Some(false), FilenameContext::DirectoryCorpus),
            FilenameMode::Never
        ));
    }

    #[test]
    fn filename_mode_default() {
        assert!(matches!(
            effective_filename_mode(None, FilenameContext::DirectoryCorpus),
            FilenameMode::Always
        ));
    }

    #[test]
    fn filename_mode_single_file_defaults_to_never() {
        assert!(matches!(
            effective_filename_mode(None, FilenameContext::SingleFileCorpus),
            FilenameMode::Never
        ));
    }

    #[test]
    fn filename_mode_single_file_respects_explicit_true() {
        assert!(matches!(
            effective_filename_mode(Some(true), FilenameContext::SingleFileCorpus),
            FilenameMode::Always
        ));
    }

    // ── resolve_effective_line_number ──

    #[test]
    fn effective_line_number_json() {
        assert!(resolve_effective_line_number(
            false,
            None,
            SearchOutputFormat::Json
        ));
    }

    #[test]
    fn effective_line_number_override_true() {
        assert!(resolve_effective_line_number(
            false,
            Some(true),
            SearchOutputFormat::Text
        ));
    }

    #[test]
    fn effective_line_number_override_false() {
        assert!(!resolve_effective_line_number(
            true,
            Some(false),
            SearchOutputFormat::Text
        ));
    }

    #[test]
    fn effective_line_number_fallback_true() {
        assert!(resolve_effective_line_number(
            true,
            None,
            SearchOutputFormat::Text
        ));
    }

    #[test]
    fn effective_line_number_fallback_false() {
        assert!(!resolve_effective_line_number(
            false,
            None,
            SearchOutputFormat::Text
        ));
    }

    // ── unescape_separator ──

    #[test]
    fn unescape_separator_plain() {
        assert_eq!(unescape_separator("hello"), b"hello");
    }

    #[test]
    fn unescape_separator_newline() {
        assert_eq!(unescape_separator(r"\n"), b"\n");
    }

    #[test]
    fn unescape_separator_tab() {
        assert_eq!(unescape_separator(r"\t"), b"\t");
    }

    #[test]
    fn unescape_separator_backslash() {
        assert_eq!(unescape_separator(r"\\"), b"\\");
    }

    #[test]
    fn unescape_separator_null() {
        assert_eq!(unescape_separator(r"\0"), b"\0");
    }

    #[test]
    fn unescape_separator_unknown_escape() {
        assert_eq!(unescape_separator(r"\x"), b"\\x");
    }

    #[test]
    fn unescape_separator_trailing_backslash() {
        assert_eq!(unescape_separator(r"ab\"), b"ab\\");
    }

    #[test]
    fn unescape_separator_mixed() {
        assert_eq!(unescape_separator(r"a\nb\tc"), b"a\nb\tc");
    }

    // ── build_line_style_flags ──

    fn ctx_with_lines(lines: SearchLineResolveCtx) -> SearchOutputCtx {
        SearchOutputCtx {
            mode: SearchModeCtx {
                effective_mode: SearchMode::Standard,
                quiet: false,
            },
            lines,
            format: SearchFormatCtx {
                null_data: false,
                color: ColorChoice::Auto,
            },
            output_format: SearchOutputFormat::Text,
            separators: SearchSeparators {
                context_separator: None,
                field_match_separator: vec![],
                field_context_separator: vec![],
            },
            print_stats: false,
            byte_offset: false,
            trim: false,
            include_zero: false,
            path_separator: None,
            max_columns: None,
            max_columns_preview: false,
        }
    }

    // Move search_output tests after this block

    #[test]
    fn line_style_flags_empty() {
        let out = ctx_with_lines(SearchLineResolveCtx {
            heading: false,
            with_filename: None,
            is_path_mode: false,
            column: false,
            line_number: None,
        });
        let flags = build_line_style_flags(&out, false);
        assert!(flags.is_empty());
    }

    #[test]
    fn line_style_flags_heading() {
        let out = ctx_with_lines(SearchLineResolveCtx {
            heading: true,
            with_filename: None,
            is_path_mode: false,
            column: false,
            line_number: None,
        });
        let flags = build_line_style_flags(&out, false);
        assert!(flags.contains(LineStyleFlags::HEADING));
        assert!(!flags.contains(LineStyleFlags::LINE_NUMBER));
    }

    #[test]
    fn line_style_flags_line_number() {
        let out = ctx_with_lines(SearchLineResolveCtx {
            heading: false,
            with_filename: None,
            is_path_mode: false,
            column: false,
            line_number: None,
        });
        let flags = build_line_style_flags(&out, true);
        assert!(flags.contains(LineStyleFlags::LINE_NUMBER));
    }

    #[test]
    fn line_style_flags_column() {
        let out = ctx_with_lines(SearchLineResolveCtx {
            heading: false,
            with_filename: None,
            is_path_mode: false,
            column: true,
            line_number: None,
        });
        let flags = build_line_style_flags(&out, false);
        assert!(flags.contains(LineStyleFlags::COLUMN));
    }

    #[test]
    fn line_style_flags_byte_offset() {
        let mut out = ctx_with_lines(SearchLineResolveCtx {
            heading: false,
            with_filename: None,
            is_path_mode: false,
            column: false,
            line_number: None,
        });
        out.byte_offset = true;
        let flags = build_line_style_flags(&out, false);
        assert!(flags.contains(LineStyleFlags::BYTE_OFFSET));
    }

    #[test]
    fn line_style_flags_trim() {
        let mut out = ctx_with_lines(SearchLineResolveCtx {
            heading: false,
            with_filename: None,
            is_path_mode: false,
            column: false,
            line_number: None,
        });
        out.trim = true;
        let flags = build_line_style_flags(&out, false);
        assert!(flags.contains(LineStyleFlags::TRIM));
    }

    #[test]
    fn line_style_flags_all() {
        let mut out = ctx_with_lines(SearchLineResolveCtx {
            heading: true,
            with_filename: None,
            is_path_mode: false,
            column: true,
            line_number: None,
        });
        out.byte_offset = true;
        out.trim = true;
        let flags = build_line_style_flags(&out, true);
        assert!(flags.contains(LineStyleFlags::HEADING));
        assert!(flags.contains(LineStyleFlags::LINE_NUMBER));
        assert!(flags.contains(LineStyleFlags::COLUMN));
        assert!(flags.contains(LineStyleFlags::BYTE_OFFSET));
        assert!(flags.contains(LineStyleFlags::TRIM));
    }

    // ── search_output ──

    #[test]
    fn search_output_quiet() {
        let result = search_output(
            SearchOutputFormat::Text,
            SearchMode::Standard,
            true,
            SearchLineStyle {
                filename_mode: FilenameMode::Always,
                flags: LineStyleFlags::empty(),
                path_display: sift_core::PathDisplay::Relative,
                columns: None,
            },
            SearchRecordStyle {
                terminator: RecordTerminator::Newline,
                color: ColorChoice::Auto,
                path_separator: None,
            },
            false,
        );
        assert!(matches!(result.emission, OutputEmission::Quiet));
        assert!(matches!(result.passthru, PassthruMode::Disabled));
    }

    #[test]
    fn search_output_normal() {
        let result = search_output(
            SearchOutputFormat::Text,
            SearchMode::Standard,
            false,
            SearchLineStyle {
                filename_mode: FilenameMode::Always,
                flags: LineStyleFlags::empty(),
                path_display: sift_core::PathDisplay::Relative,
                columns: None,
            },
            SearchRecordStyle {
                terminator: RecordTerminator::Newline,
                color: ColorChoice::Auto,
                path_separator: None,
            },
            true,
        );
        assert!(matches!(result.emission, OutputEmission::Normal));
        assert!(matches!(result.include_zero, ZeroCountMode::Include));
    }

    // ── Cli::resolve_separators ──

    #[test]
    fn resolve_separators_default_context_sep() {
        let cli = crate::cli::Cli::try_parse_from(["sift", "pat"]).unwrap();
        let sep = cli.resolve_separators();
        assert_eq!(sep.context_separator, Some(b"--".to_vec()));
    }

    #[test]
    fn resolve_separators_custom_separator() {
        let cli =
            crate::cli::Cli::try_parse_from(["sift", "--context-separator", "===", "pat"]).unwrap();
        let sep = cli.resolve_separators();
        assert_eq!(sep.context_separator, Some(b"===".to_vec()));
    }

    #[test]
    fn resolve_separators_suppress_context_sep() {
        let cli =
            crate::cli::Cli::try_parse_from(["sift", "--no-context-separator", "pat"]).unwrap();
        let sep = cli.resolve_separators();
        assert_eq!(sep.context_separator, None);
    }

    #[test]
    fn resolve_separators_field_match_default() {
        let cli = crate::cli::Cli::try_parse_from(["sift", "pat"]).unwrap();
        let sep = cli.resolve_separators();
        assert_eq!(sep.field_match_separator, b":");
    }

    #[test]
    fn resolve_separators_field_context_default() {
        let cli = crate::cli::Cli::try_parse_from(["sift", "pat"]).unwrap();
        let sep = cli.resolve_separators();
        assert_eq!(sep.field_context_separator, b"-");
    }
}

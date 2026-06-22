use clap::{ArgAction, Args};
use sift_core::search::{
    ColorChoice, FilenameMode, LineStyleFlags, OutputEmission, PassthruMode, SearchLineStyle,
    SearchMode, SearchOutput, SearchOutputFormat, SearchRecordStyle, SearchSeparators, SearchStats,
    ZeroCountMode,
};
use std::path::PathBuf;

/// Describes the filename display context for deciding whether to show paths.
#[derive(Clone, Copy)]
pub enum FilenameContext {
    PathMode,
    DirectoryCorpus,
    SingleFileCorpus,
}

use super::argv::Argv;
use super::filter::SearchFilterCtx;

/// Output-related flags resolved from clap declarations.
#[derive(Clone)]
pub struct OutputConfig {
    pub column: ColumnDecl,
    pub columns: ColumnsDecl,
    pub extra: ExtraOutputDecl,
    pub replace_trim: bool,
    pub path_separator: Option<String>,
    pub line_number: bool,
    pub separators: SeparatorDecl,
    pub search_paths: Vec<PathBuf>,
}

impl OutputConfig {
    #[must_use]
    pub fn separators(&self) -> SearchSeparators {
        let context_separator = if self.separators.suppress_context_sep {
            None
        } else if let Some(ref s) = self.separators.context_sep {
            Some(unescape_separator(s))
        } else {
            Some(b"--".to_vec())
        };
        let field_match_separator = self
            .separators
            .field_match
            .as_ref()
            .map_or_else(|| b":".to_vec(), |s| unescape_separator(s));
        let field_context_separator = self
            .separators
            .field_context
            .as_ref()
            .map_or_else(|| b"-".to_vec(), |s| unescape_separator(s));
        SearchSeparators {
            context_separator,
            field_match_separator,
            field_context_separator,
        }
    }
}

// ── Clap declarations (output flags) ──

#[derive(Args, Clone)]
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

#[derive(Args, Clone)]
pub struct ColumnDecl {
    #[arg(long = "column")]
    pub column: bool,
    #[arg(long = "vimgrep")]
    pub vimgrep: bool,
    #[arg(short = 'p', long = "pretty")]
    pub pretty: bool,
}

#[derive(Args, Clone)]
pub struct ColumnsDecl {
    #[arg(short = 'M', long = "max-columns", value_name = "NUM")]
    pub max_columns: Option<u64>,
    #[arg(long = "max-columns-preview")]
    pub max_columns_preview: bool,
}

#[derive(Args, Clone)]
pub struct ReplaceDecl {
    #[arg(short = 'r', long = "replace", value_name = "REPLACEMENT")]
    pub replace: Option<String>,
    #[arg(long = "trim")]
    pub trim: bool,
    #[arg(long = "passthru", visible_alias = "passthrough")]
    pub passthru: bool,
}

#[derive(Args, Clone)]
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

#[derive(Clone, Copy)]
pub struct SearchRecordFlags {
    pub byte_offset: bool,
    pub trim: bool,
    pub include_zero: bool,
}

#[derive(Clone, Copy)]
pub struct SearchColumnResolve {
    pub max_columns_preview: bool,
}

#[derive(Clone)]
pub struct SearchOutputCtx {
    pub mode: SearchModeCtx,
    pub lines: SearchLineResolveCtx,
    pub format: SearchFormatCtx,
    pub output_format: SearchOutputFormat,
    pub separators: SearchSeparators,
    pub print_stats: bool,
    pub record: SearchRecordFlags,
    pub path_separator: Option<u8>,
    pub max_columns: Option<u64>,
    pub columns: SearchColumnResolve,
}

// ── Argv-order resolution ──

/// Output-related flags resolved from raw argv (ripgrep last-wins).
#[derive(Clone, Copy, Default)]
pub struct OutputModeFlags {
    pub stats: bool,
    pub json: bool,
    pub heading: bool,
}

#[derive(Clone, Copy, Default)]
pub struct OutputPathFlags {
    pub glob_case_insensitive: bool,
    pub null_data: bool,
}

pub struct OutputArgv {
    pub mode: OutputModeFlags,
    pub path: OutputPathFlags,
    pub color: ColorChoice,
    pub line_number: Option<bool>,
    pub with_filename: Option<bool>,
}

impl OutputArgv {
    #[must_use]
    pub fn resolve(argv: &Argv<'_>) -> Self {
        let tokens = argv.as_slice();
        Self {
            mode: OutputModeFlags {
                stats: Self::stats(tokens),
                json: Self::json(tokens),
                heading: Self::heading(tokens),
            },
            path: OutputPathFlags {
                glob_case_insensitive: Self::glob_case_insensitive(tokens),
                null_data: Self::null_data(tokens),
            },
            color: Self::color(tokens),
            line_number: Self::line_number(tokens),
            with_filename: Self::with_filename(tokens),
        }
    }

    fn glob_case_insensitive(args: &[String]) -> bool {
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

    fn null_data(args: &[String]) -> bool {
        let mut result = false;
        for arg in args {
            match arg.as_str() {
                "-0" | "--null" => result = true,
                _ => {}
            }
        }
        result
    }

    fn color(args: &[String]) -> ColorChoice {
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

    fn stats(args: &[String]) -> bool {
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

    fn json(args: &[String]) -> bool {
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

    fn heading(args: &[String]) -> bool {
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

    fn line_number(args: &[String]) -> Option<bool> {
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

    fn with_filename(args: &[String]) -> Option<bool> {
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
}

fn parse_color_when(s: &str) -> ColorChoice {
    match s {
        "never" => ColorChoice::Never,
        "always" => ColorChoice::Always,
        _ => ColorChoice::Auto,
    }
}

fn unescape_separator(s: &str) -> Vec<u8> {
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

impl SearchOutputCtx {
    #[must_use]
    pub fn resolve(
        config: &OutputConfig,
        argv: &Argv<'_>,
        effective_mode: SearchMode,
        quiet: bool,
        line_number_override: Option<bool>,
    ) -> (Self, SearchFilterCtx) {
        let output_argv = OutputArgv::resolve(argv);
        let pretty = config.column.pretty;
        let vimgrep = config.column.vimgrep;
        let column = config.column.column || vimgrep;

        let is_path_mode = matches!(
            effective_mode,
            SearchMode::FilesWithMatches | SearchMode::FilesWithoutMatch
        );
        let effective_heading = output_argv.mode.heading || pretty;
        let effective_color = if pretty && output_argv.color == ColorChoice::Auto {
            ColorChoice::Always
        } else {
            output_argv.color
        };
        let print_stats = output_argv.mode.stats || output_argv.mode.json;

        let out = Self {
            mode: SearchModeCtx {
                effective_mode,
                quiet,
            },
            lines: SearchLineResolveCtx {
                heading: effective_heading,
                with_filename: output_argv.with_filename,
                is_path_mode,
                column,
                line_number: line_number_override,
            },
            format: SearchFormatCtx {
                null_data: output_argv.path.null_data,
                color: effective_color,
            },
            output_format: if output_argv.mode.json {
                SearchOutputFormat::Json
            } else {
                SearchOutputFormat::Text
            },
            separators: config.separators(),
            print_stats,
            record: SearchRecordFlags {
                byte_offset: config.extra.byte_offset,
                trim: config.replace_trim,
                include_zero: config.extra.include_zero,
            },
            path_separator: config
                .path_separator
                .as_ref()
                .and_then(|s| s.as_bytes().first().copied()),
            max_columns: config.columns.max_columns,
            columns: SearchColumnResolve {
                max_columns_preview: config.columns.max_columns_preview,
            },
        };
        let filter = SearchFilterCtx::resolve(argv);
        (out, filter)
    }

    #[must_use]
    pub fn to_core_output(
        &self,
        config: &OutputConfig,
        filename_ctx: FilenameContext,
    ) -> SearchOutput {
        use super::paths::CorpusScope;
        let path_display = CorpusScope::path_display(&config.search_paths);
        let line_number = Self::effective_line_number(
            config.line_number,
            self.lines.line_number,
            self.output_format,
        );
        let line_flags = Self::line_style_flags(self, line_number);
        Self::core_output(
            self.output_format,
            self.mode.effective_mode,
            self.mode.quiet,
            SearchLineStyle {
                filename_mode: Self::filename_mode(self.lines.with_filename, filename_ctx),
                flags: line_flags,
                path_display,
                columns: self.max_columns.map(|max| sift_core::search::ColumnLimit {
                    max,
                    overflow: if self.columns.max_columns_preview {
                        sift_core::search::ColumnOverflow::Preview
                    } else {
                        sift_core::search::ColumnOverflow::Omit
                    },
                }),
            },
            SearchRecordStyle {
                terminator: if self.format.null_data {
                    sift_core::search::RecordTerminator::Nul
                } else {
                    sift_core::search::RecordTerminator::Newline
                },
                color: self.format.color,
                path_separator: self.path_separator,
            },
            self.record.include_zero,
        )
    }

    pub fn write_stats(stats: &SearchStats) {
        eprintln!("{} matches", stats.matches);
        eprintln!("{} files contained matches", stats.files_with_matches);
        eprintln!("{} files searched", stats.files_searched);
        eprintln!("{} bytes printed", stats.bytes_printed);
        eprintln!("{} bytes searched", stats.bytes_searched);
        eprintln!("{:.6}s elapsed", stats.elapsed.as_secs_f64());
    }

    #[must_use]
    const fn core_output(
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
    fn line_style_flags(out: &Self, line_number: bool) -> LineStyleFlags {
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
        if out.record.byte_offset {
            f |= LineStyleFlags::BYTE_OFFSET;
        }
        if out.record.trim {
            f |= LineStyleFlags::TRIM;
        }
        f
    }

    #[must_use]
    const fn filename_mode(with_filename: Option<bool>, context: FilenameContext) -> FilenameMode {
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
    const fn effective_line_number(
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grep::pattern::PatternArgv;
    use clap::Parser;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(ToString::to_string).collect()
    }

    fn out_argv(items: &[&str]) -> OutputArgv {
        OutputArgv::resolve(&Argv::new(&args(items)))
    }

    #[test]
    fn output_argv_null_short() {
        assert!(out_argv(&["sift", "-0", "pat"]).path.null_data);
    }

    #[test]
    fn output_argv_color_last_wins() {
        assert!(matches!(
            out_argv(&["sift", "--color=never", "--color=always", "pat"]).color,
            ColorChoice::Always
        ));
    }

    #[test]
    fn output_argv_json_toggle() {
        assert!(!out_argv(&["sift", "--json", "--no-json", "pat"]).mode.json);
    }

    #[test]
    fn output_argv_line_number_last_wins() {
        assert_eq!(
            out_argv(&["sift", "-n", "-N", "pat"]).line_number,
            Some(false)
        );
    }

    #[test]
    fn context_lines_after_short() {
        assert_eq!(
            PatternArgv::context(&Argv::new(&args(&["sift", "-A", "5", "pat"]))),
            (0, 5)
        );
    }

    #[test]
    fn filename_mode_single_file_defaults_to_never() {
        assert!(matches!(
            SearchOutputCtx::filename_mode(None, FilenameContext::SingleFileCorpus),
            FilenameMode::Never
        ));
    }

    #[test]
    fn effective_line_number_json() {
        assert!(SearchOutputCtx::effective_line_number(
            false,
            None,
            SearchOutputFormat::Json
        ));
    }

    fn output_config(args: &[&str]) -> OutputConfig {
        crate::cli::Cli::try_parse_from(args)
            .unwrap()
            .grep_config()
            .output
    }

    #[test]
    fn resolve_separators_default_context_sep() {
        let sep = output_config(&["sift", "pat"]).separators();
        assert_eq!(sep.context_separator, Some(b"--".to_vec()));
    }
}

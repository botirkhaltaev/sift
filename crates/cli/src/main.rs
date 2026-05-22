//! Thin CLI over `sift-core`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Arg, ArgAction, Args, Command, FromArgMatches, Parser, Subcommand, value_parser};
use sift_core::{
    BinaryMode, CaseMode, ColorChoice, CompiledSearch, Error as SiftError, FilenameMode,
    GlobConfig, HiddenMode, IgnoreConfig, IgnoreSources, Index, IndexBuilder, LineStyleFlags,
    OutputEmission, PathDisplay, SearchFilter, SearchFilterConfig, SearchLineStyle,
    SearchMatchFlags, SearchMode, SearchOptions, SearchOutput, SearchOutputFormat,
    SearchRecordStyle, SearchSeparators, SearchStats, TypeDef, VisibilityConfig,
};

#[derive(Parser)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    #[command(flatten)]
    patterns: PatternArgs,
    #[command(flatten)]
    search_scope: SearchScope,
    #[command(flatten)]
    regex1: RegexFlagsA,
    #[command(flatten)]
    regex2: RegexFlagsB,
    #[command(flatten)]
    line_number_decl: LineNumberDecl,
    #[command(flatten)]
    search_flags: SearchFlags,
    #[command(flatten)]
    filename_decl: FilenameDecl,
    #[command(flatten)]
    heading_decl: HeadingDecl,
    #[command(flatten)]
    column_decl: ColumnDecl,
    #[command(flatten)]
    glob_flags: GlobFlags,
    #[command(flatten)]
    ignore_no: IgnoreNoDecl,
    #[command(flatten)]
    ignore_vcs: IgnoreVcsDecl,
    #[command(flatten)]
    ignore_dot: IgnoreDotDecl,
    #[command(flatten)]
    ignore_git: IgnoreGitDecl,
    #[command(flatten)]
    ignore_global: IgnoreGlobalDecl,
    #[command(flatten)]
    ignore_exclude: IgnoreExcludeDecl,
    #[command(flatten)]
    ignore_parent: IgnoreParentDecl,
    #[command(flatten)]
    ignore_files_decl: IgnoreFilesDecl,
    #[command(flatten)]
    messages_decl: MessagesDecl,
    #[command(flatten)]
    ignore_messages_decl: IgnoreMessagesDecl,
    #[command(flatten)]
    unrestricted: UnrestrictedDecl,
    #[command(flatten)]
    context_decl: ContextDecl,
    #[command(flatten)]
    null_color: NullColorDecl,
    #[command(flatten)]
    paths: PathArgs,
    #[command(flatten)]
    stats_decl: StatsDecl,
    #[command(flatten)]
    json_decl: JsonDecl,
    #[command(flatten)]
    separator_decl: SeparatorDecl,
    #[command(flatten)]
    filter_decl: FilterDecl,
    #[command(flatten)]
    binary_decl: BinaryDecl,
    #[command(flatten)]
    replace_decl: ReplaceDecl,
    #[command(flatten)]
    extra_output: ExtraOutputDecl,
    #[command(flatten)]
    threading: ThreadingDecl,
    #[command(flatten)]
    multiline_decl: MultilineDecl,
    #[command(flatten)]
    walker_decl: WalkerDecl,
}

#[derive(Args)]
struct SeparatorDecl {
    #[arg(
        long = "context-separator",
        value_name = "SEPARATOR",
        allow_hyphen_values = true
    )]
    context_sep: Option<String>,
    #[arg(long = "no-context-separator")]
    suppress_context_sep: bool,
    #[arg(long = "field-match-separator", value_name = "SEPARATOR")]
    field_match: Option<String>,
    #[arg(long = "field-context-separator", value_name = "SEPARATOR")]
    field_context: Option<String>,
}

/// Declares `--json` / `--no-json` for clap; effective value uses [`resolve_json_from_args`].
#[derive(Args)]
struct JsonDecl {
    #[arg(long = "json", action = ArgAction::SetTrue)]
    _json: bool,
    #[arg(long = "no-json", action = ArgAction::SetTrue)]
    _no_json: bool,
}

/// Declares `--stats` for clap; effective value uses [`resolve_stats_from_args`].
#[derive(Args)]
struct StatsDecl {
    #[arg(long = "stats", action = ArgAction::SetTrue)]
    _stats: bool,
}

/// Threading and output-buffering flags.
#[derive(Args)]
struct ThreadingDecl {
    /// Number of threads to use for searching.
    #[arg(short = 'j', long = "threads", value_name = "NUM")]
    threads: Option<usize>,
    /// Accepted for ripgrep compatibility; sift always line-buffers.
    #[arg(long = "line-buffered")]
    line_buffered: bool,
    /// Accepted for ripgrep compatibility; no behavioral effect in sift.
    #[arg(long = "block-buffered")]
    block_buffered: bool,
    /// Override the path separator in output.
    #[arg(long = "path-separator", value_name = "SEPARATOR")]
    path_separator: Option<String>,
}

/// Filesystem-level flags for the walker.
#[derive(Args)]
struct WalkerDecl {
    /// Do not cross filesystem boundaries.
    #[arg(long = "one-file-system")]
    one_file_system: bool,
    /// Accepted for ripgrep compatibility; sift does not use mmap.
    #[arg(long = "mmap")]
    mmap: bool,
    /// Accepted for ripgrep compatibility; sift does not use mmap.
    #[arg(long = "no-mmap")]
    no_mmap: bool,
}

/// Multiline and CRLF flags.
#[derive(Args)]
struct MultilineDecl {
    /// Enable multiline matching.
    #[arg(short = 'U', long = "multiline")]
    multiline: bool,
    /// Make `.` match `\n` in multiline mode.
    #[arg(long = "multiline-dotall")]
    multiline_dotall: bool,
    /// Treat CRLF (`\r\n`) as a line terminator.
    #[arg(long = "crlf")]
    crlf: bool,
}

/// `-0` / `--null` and `--color` for clap; effective null/color use argv resolvers.
#[derive(Args)]
struct NullColorDecl {
    #[arg(short = '0', long = "null", action = ArgAction::SetTrue)]
    _null: bool,
    #[arg(long = "color", value_name = "WHEN")]
    _color: Option<String>,
}

/// Declares `-A`/`-B`/`-C` for clap; effective values use [`resolve_context_from_args`].
#[derive(Args)]
struct ContextDecl {
    #[arg(short = 'A', long = "after-context", value_name = "NUM", action = ArgAction::Append)]
    _after: Vec<usize>,
    #[arg(short = 'B', long = "before-context", value_name = "NUM", action = ArgAction::Append)]
    _before: Vec<usize>,
    #[arg(short = 'C', long = "context", value_name = "NUM", action = ArgAction::Append)]
    _context: Vec<usize>,
}

/// Clap declarations only; effective values come from [`resolve_visibility_and_ignore`].
#[derive(Args)]
struct IgnoreNoDecl {
    #[arg(long = "no-ignore", action = ArgAction::SetTrue)]
    _no_ignore: bool,
    #[arg(long = "ignore", action = ArgAction::SetTrue)]
    _ignore: bool,
}

#[derive(Args)]
struct IgnoreVcsDecl {
    #[arg(long = "no-ignore-vcs", action = ArgAction::SetTrue)]
    _no_ignore_vcs: bool,
    #[arg(long = "ignore-vcs", action = ArgAction::SetTrue)]
    _ignore_vcs: bool,
}

#[derive(Args)]
struct IgnoreDotDecl {
    #[arg(long = "no-ignore-dot", action = ArgAction::SetTrue)]
    _no_ignore_dot: bool,
    #[arg(long = "ignore-dot", action = ArgAction::SetTrue)]
    _ignore_dot: bool,
}

#[derive(Args)]
struct IgnoreGitDecl {
    #[arg(long = "no-require-git", action = ArgAction::SetTrue)]
    _no_require_git: bool,
    #[arg(long = "require-git", action = ArgAction::SetTrue)]
    _require_git: bool,
}

/// Clap-only; resolved via argv.
#[derive(Args)]
struct IgnoreGlobalDecl {
    #[arg(long = "no-ignore-global", action = ArgAction::SetTrue)]
    _no_ignore_global: bool,
    #[arg(long = "ignore-global", action = ArgAction::SetTrue)]
    _ignore_global: bool,
}

/// Clap-only; resolved via argv.
#[derive(Args)]
struct IgnoreExcludeDecl {
    #[arg(long = "no-ignore-exclude", action = ArgAction::SetTrue)]
    _no_ignore_exclude: bool,
    #[arg(long = "ignore-exclude", action = ArgAction::SetTrue)]
    _ignore_exclude: bool,
}

/// Clap-only; resolved via argv.
#[derive(Args)]
struct IgnoreParentDecl {
    #[arg(long = "no-ignore-parent", action = ArgAction::SetTrue)]
    _no_ignore_parent: bool,
    #[arg(long = "ignore-parent", action = ArgAction::SetTrue)]
    _ignore_parent: bool,
}

/// Clap-only; resolved via argv.
#[derive(Args)]
struct IgnoreFilesDecl {
    /// Ignore any --ignore-file flags (even ones that come after).
    #[arg(long = "no-ignore-files", action = ArgAction::SetTrue)]
    _no_ignore_files: bool,
    #[arg(long = "ignore-files", action = ArgAction::SetTrue)]
    _ignore_files: bool,
}

/// Clap-only; resolved via argv.
#[derive(Args)]
struct MessagesDecl {
    /// Suppress all error messages related to opening and reading files.
    #[arg(long = "no-messages", action = ArgAction::SetTrue)]
    _no_messages: bool,
    #[arg(long = "messages", action = ArgAction::SetTrue)]
    _messages: bool,
}

/// Clap-only; resolved via argv.
#[derive(Args)]
struct IgnoreMessagesDecl {
    /// Suppress error messages related to parsing ignore files.
    #[arg(long = "no-ignore-messages", action = ArgAction::SetTrue)]
    _no_ignore_messages: bool,
    #[arg(long = "ignore-messages", action = ArgAction::SetTrue)]
    _ignore_messages: bool,
}

#[derive(Args)]
struct UnrestrictedDecl {
    #[arg(short = 'u', long = "unrestricted", action = ArgAction::Count)]
    _unrestricted: u8,
}

#[derive(Args)]
struct BinaryDecl {
    /// Search binary files as if they were text.
    #[arg(short = 'a', long = "text")]
    text: bool,
    /// Search binary files; NUL bytes are converted, matches after NUL are shown.
    #[arg(long = "binary")]
    binary: bool,
}

#[derive(Args)]
struct PatternArgs {
    #[arg(short = 'e', long = "regexp", value_name = "PATTERN")]
    regexp: Vec<String>,
    #[arg(short = 'f', long = "file", value_name = "FILE")]
    pattern_file: Option<PathBuf>,
    #[arg(value_name = "PATTERN")]
    pattern: Option<String>,
}

#[derive(Args)]
struct SearchScope {
    #[arg(value_name = "PATH", num_args = 0..)]
    paths: Vec<PathBuf>,
}

#[derive(Args)]
struct RegexFlagsA {
    #[arg(short = 'v', long)]
    invert_match: bool,

    #[arg(short = 'w', long)]
    word_regexp: bool,
}

#[derive(Args)]
struct RegexFlagsB {
    #[arg(short = 'x', long)]
    line_regexp: bool,
}

#[derive(Args)]
struct LineNumberDecl {
    #[arg(short = 'n', long = "line-number")]
    line_number: bool,
    #[arg(short = 'N', long = "no-line-number")]
    no_line_number: bool,
}

#[derive(Args)]
struct FilenameDecl {
    #[arg(short = 'I', long = "no-filename")]
    no_filename: bool,
    #[arg(short = 'H', long = "with-filename")]
    with_filename: bool,
}

#[derive(Args)]
struct HeadingDecl {
    #[arg(long = "heading")]
    heading: bool,
    #[arg(long = "no-heading")]
    no_heading: bool,
}

#[derive(Args)]
struct ColumnDecl {
    #[arg(long = "column")]
    column: bool,
    #[arg(long = "vimgrep")]
    vimgrep: bool,
    #[arg(short = 'p', long = "pretty")]
    pretty: bool,
}

#[derive(Args)]
struct ReplaceDecl {
    #[arg(short = 'r', long = "replace", value_name = "REPLACEMENT")]
    replace: Option<String>,
    #[arg(long = "trim")]
    trim: bool,
    #[arg(long = "passthru", visible_alias = "passthrough")]
    passthru: bool,
}

#[derive(Args)]
struct ExtraOutputDecl {
    #[arg(long = "include-zero")]
    include_zero: bool,
    #[arg(short = 'b', long = "byte-offset")]
    byte_offset: bool,
}

fn resolve_glob_case_insensitive_from_args(args: &[String]) -> bool {
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

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    struct MessageFlags: u8 {
        const NO_MESSAGES        = 1 << 0;
        const NO_IGNORE_MESSAGES = 1 << 1;
        const NO_IGNORE_FILES    = 1 << 2;
    }
}

/// Resolved visibility / ignore state from argv.
struct IgnoreResolution {
    hidden: bool,
    sources: IgnoreSources,
    require_git: bool,
    msg_flags: MessageFlags,
}

/// Hidden files, ignore rules, and `require_git` — processed in argv order (ripgrep-style).
fn resolve_visibility_and_ignore(args: &[String]) -> IgnoreResolution {
    const DEFAULT_SOURCES: IgnoreSources = IgnoreSources::DOT
        .union(IgnoreSources::VCS)
        .union(IgnoreSources::EXCLUDE)
        .union(IgnoreSources::GLOBAL)
        .union(IgnoreSources::PARENT);

    let mut sources = DEFAULT_SOURCES;
    let mut require_git = true;
    let mut hidden = false;
    let mut u_count: u8 = 0;
    let mut msg_flags = MessageFlags::empty();

    for arg in args {
        if arg == "--unrestricted" {
            u_count = u_count.saturating_add(1).min(3);
            if u_count == 1 {
                sources = IgnoreSources::empty();
            } else if u_count == 2 {
                hidden = true;
            }
            continue;
        }
        if arg.len() >= 2 {
            let bytes = arg.as_bytes();
            if bytes[0] == b'-' && bytes.get(1) != Some(&b'-') && arg[1..].chars().all(|c| c == 'u')
            {
                for _ in 0..arg.len().saturating_sub(1) {
                    u_count = u_count.saturating_add(1).min(3);
                    if u_count == 1 {
                        sources = IgnoreSources::empty();
                    } else if u_count == 2 {
                        hidden = true;
                    }
                }
                continue;
            }
        }

        match arg.as_str() {
            "--no-ignore" => sources = IgnoreSources::empty(),
            "--ignore" => sources = DEFAULT_SOURCES,
            "--no-ignore-vcs" => sources.remove(IgnoreSources::VCS),
            "--ignore-vcs" => sources.insert(IgnoreSources::VCS),
            "--no-ignore-dot" => sources.remove(IgnoreSources::DOT),
            "--ignore-dot" => sources.insert(IgnoreSources::DOT),
            "--no-ignore-global" => sources.remove(IgnoreSources::GLOBAL),
            "--ignore-global" => sources.insert(IgnoreSources::GLOBAL),
            "--no-ignore-exclude" => sources.remove(IgnoreSources::EXCLUDE),
            "--ignore-exclude" => sources.insert(IgnoreSources::EXCLUDE),
            "--no-ignore-parent" => sources.remove(IgnoreSources::PARENT),
            "--ignore-parent" => sources.insert(IgnoreSources::PARENT),
            "--no-require-git" => require_git = false,
            "--require-git" => require_git = true,
            "--hidden" | "-." => hidden = true,
            "--no-hidden" => hidden = false,
            "--no-messages" => msg_flags.insert(MessageFlags::NO_MESSAGES),
            "--messages" => msg_flags.remove(MessageFlags::NO_MESSAGES),
            "--no-ignore-messages" => msg_flags.insert(MessageFlags::NO_IGNORE_MESSAGES),
            "--ignore-messages" => msg_flags.remove(MessageFlags::NO_IGNORE_MESSAGES),
            "--no-ignore-files" => msg_flags.insert(MessageFlags::NO_IGNORE_FILES),
            "--ignore-files" => msg_flags.remove(MessageFlags::NO_IGNORE_FILES),
            _ => {}
        }
    }

    IgnoreResolution {
        hidden,
        sources,
        require_git,
        msg_flags,
    }
}

fn parse_usize_token(s: &str) -> Option<usize> {
    s.parse().ok()
}

/// `-A` / `-B` / `-C` and long forms; argv order with later flags overriding (ripgrep-style).
fn resolve_context_from_args(args: &[String]) -> (usize, usize) {
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

fn parse_color_when(s: &str) -> ColorChoice {
    match s {
        "never" => ColorChoice::Never,
        "always" => ColorChoice::Always,
        _ => ColorChoice::Auto,
    }
}

/// `--color when` / `--color=when`; argv order, last wins.
fn resolve_stats_from_args(args: &[String]) -> bool {
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

/// `--json` / `--no-json`; later flag wins (ripgrep-style).
fn resolve_json_from_args(args: &[String]) -> bool {
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

fn resolve_color_from_args(args: &[String]) -> ColorChoice {
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

/// `-0` / `--null`; later flag wins (`--null` then `--no-null` style not in rg — we only enable).
fn resolve_null_from_args(args: &[String]) -> bool {
    let mut result = false;
    for arg in args {
        match arg.as_str() {
            "-0" | "--null" => result = true,
            _ => {}
        }
    }
    result
}

fn resolve_heading_from_args(args: &[String]) -> bool {
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

fn resolve_line_number_from_args(args: &[String]) -> Option<bool> {
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

fn resolve_with_filename_from_args(args: &[String]) -> Option<bool> {
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

#[derive(Clone)]
struct GlobFlags {
    glob: Vec<String>,
}

impl GlobFlags {
    const fn new() -> Self {
        Self { glob: Vec::new() }
    }
}

impl Args for GlobFlags {
    fn augment_args(cmd: Command) -> Command {
        cmd.arg(
            Arg::new("glob")
                .short('g')
                .long("glob")
                .action(ArgAction::Append)
                .num_args(1),
        )
        .arg(
            Arg::new("glob_case_insensitive")
                .long("glob-case-insensitive")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("no_glob_case_insensitive")
                .long("no-glob-case-insensitive")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("hidden")
                .short('.')
                .long("hidden")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("no_hidden")
                .long("no-hidden")
                .action(ArgAction::SetTrue),
        )
    }

    fn augment_args_for_update(cmd: Command) -> Command {
        Self::augment_args(cmd)
    }
}

impl FromArgMatches for GlobFlags {
    fn from_arg_matches(matches: &clap::ArgMatches) -> Result<Self, clap::Error> {
        let glob = matches
            .get_many::<String>("glob")
            .map(|v| v.cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        Ok(Self { glob })
    }

    fn update_from_arg_matches(&mut self, matches: &clap::ArgMatches) -> Result<(), clap::Error> {
        *self = Self::from_arg_matches(matches)?;
        Ok(())
    }
}

impl Default for GlobFlags {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Args)]
struct PathArgs {
    #[arg(short = 'm', long = "max-count", value_name = "NUM")]
    max_count: Option<usize>,
    #[arg(long, default_value = ".sift")]
    sift_dir: PathBuf,
    /// Follow symbolic links.
    #[arg(short = 'L', long = "follow")]
    follow: bool,
}

#[derive(Args)]
struct FilterDecl {
    #[arg(long = "max-depth", value_name = "NUM")]
    max_depth: Option<usize>,
    #[arg(long = "max-filesize", value_name = "NUM+SUFFIX?")]
    max_filesize: Option<String>,
    #[arg(long = "iglob", action = ArgAction::Append, value_name = "GLOB")]
    iglob: Vec<String>,
    #[arg(long = "ignore-file", action = ArgAction::Append, value_name = "PATH")]
    ignore_file: Vec<PathBuf>,
    /// Print each file that would be searched (no actual search).
    #[arg(long = "files")]
    files: bool,
    #[arg(short = 't', long = "type", action = ArgAction::Append, value_name = "TYPE")]
    type_include: Vec<String>,
    #[arg(short = 'T', long = "type-not", action = ArgAction::Append, value_name = "TYPE")]
    type_exclude: Vec<String>,
    #[arg(long = "type-list")]
    type_list: bool,
    #[arg(long = "type-add", action = ArgAction::Append, value_name = "TYPE_SPEC")]
    type_add: Vec<String>,
    #[arg(long = "type-clear", action = ArgAction::Append, value_name = "TYPE")]
    type_clear: Vec<String>,
    #[arg(long = "sort", value_name = "SORTBY")]
    sort: Option<String>,
    #[arg(long = "sortr", value_name = "SORTBY")]
    sortr: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    Build {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
}

#[derive(Clone)]
pub struct SearchFlags {
    pub case_mode: CaseMode,
    pub fixed_strings: bool,
}

fn resolve_case_mode_from_args(args: &[String]) -> CaseMode {
    let mut last_idx = 0usize;
    let mut result = CaseMode::Sensitive;
    let case_flags = [
        ("ci", CaseMode::Insensitive),
        ("cs", CaseMode::Sensitive),
        ("sc", CaseMode::Smart),
    ];
    for (i, arg) in args.iter().enumerate() {
        let bytes = arg.as_bytes();
        let is_short = bytes.len() == 2 && bytes[0] == b'-';
        let is_long = bytes.len() > 2 && bytes[0] == b'-' && bytes[1] == b'-';
        let flag = if is_short {
            match bytes.get(1) {
                Some(&b'i') => Some("ci"),
                Some(&b's') => Some("cs"),
                Some(&b'S') => Some("sc"),
                _ => None,
            }
        } else if is_long {
            let suffix = &bytes[2..];
            if suffix == b"ignore-case" {
                Some("ci")
            } else if suffix == b"case-sensitive" {
                Some("cs")
            } else if suffix == b"smart-case" {
                Some("sc")
            } else {
                None
            }
        } else {
            None
        };
        if let Some(name) = flag {
            for (id, mode) in &case_flags {
                if *id == name {
                    if i > last_idx {
                        last_idx = i;
                        result = *mode;
                    }
                    break;
                }
            }
        }
    }
    result
}

fn resolve_invert_match_from_args(args: &[String]) -> bool {
    for arg in args {
        if arg == "--" {
            return false;
        }
        let bytes = arg.as_bytes();
        let is_long = bytes.len() > 2 && bytes[0] == b'-' && bytes[1] == b'-';
        if is_long && &bytes[2..] == b"invert-match" {
            return true;
        }
        let is_short = bytes.len() == 2 && bytes[0] == b'-';
        if is_short && bytes[1] == b'v' {
            return true;
        }
    }
    false
}

fn resolve_output_mode(args: &[String], invert_match: bool) -> (SearchMode, bool, bool) {
    let mut last_idx = 0usize;
    let mut mode = SearchMode::Standard;
    let mut quiet = false;
    let mut saw_only_matching = false;

    for (i, arg) in args.iter().enumerate() {
        let bytes = arg.as_bytes();
        let is_short = bytes.len() == 2 && bytes[0] == b'-';
        let is_long = bytes.len() > 2 && bytes[0] == b'-' && bytes[1] == b'-';

        let flag = if is_short {
            match bytes.get(1) {
                Some(&b'c') => Some((i, "count")),
                Some(&b'l') => Some((i, "files_with_matches")),
                Some(&b'o') => Some((i, "only_matching")),
                Some(&b'q') => Some((i, "quiet")),
                _ => None,
            }
        } else if is_long {
            let suffix = &arg[2..];
            match suffix {
                "count" => Some((i, "count")),
                "count-matches" => Some((i, "count_matches")),
                "files-with-matches" => Some((i, "files_with_matches")),
                "files-without-match" => Some((i, "files_without_match")),
                "only-matching" => Some((i, "only_matching")),
                "quiet" => Some((i, "quiet")),
                _ => None,
            }
        } else {
            None
        };

        if let Some((idx, name)) = flag
            && idx > last_idx
        {
            last_idx = idx;
            match name {
                "count" => mode = SearchMode::Count,
                "count_matches" => mode = SearchMode::CountMatches,
                "files_with_matches" => mode = SearchMode::FilesWithMatches,
                "files_without_match" => mode = SearchMode::FilesWithoutMatch,
                "only_matching" => saw_only_matching = true,
                "quiet" => quiet = true,
                _ => {}
            }
        }
    }

    if mode == SearchMode::Standard && saw_only_matching {
        mode = SearchMode::OnlyMatching;
    }

    if mode == SearchMode::OnlyMatching && invert_match {
        mode = SearchMode::Count;
    }

    if mode == SearchMode::Count && saw_only_matching {
        mode = SearchMode::CountMatches;
    }

    let only_matching = mode == SearchMode::OnlyMatching;
    if only_matching {
        mode = SearchMode::Standard;
    }

    (mode, only_matching, quiet)
}

impl Args for SearchFlags {
    fn augment_args(cmd: Command) -> Command {
        cmd.arg(
            Arg::new("ci")
                .short('i')
                .long("ignore-case")
                .action(ArgAction::Append)
                .num_args(0..=0)
                .value_parser(value_parser!(bool))
                .default_missing_value("true"),
        )
        .arg(
            Arg::new("cs")
                .short('s')
                .long("case-sensitive")
                .action(ArgAction::Append)
                .num_args(0..=0)
                .value_parser(value_parser!(bool))
                .default_missing_value("true"),
        )
        .arg(
            Arg::new("sc")
                .short('S')
                .long("smart-case")
                .action(ArgAction::Append)
                .num_args(0..=0)
                .value_parser(value_parser!(bool))
                .default_missing_value("true"),
        )
        .arg(
            Arg::new("fixed_strings")
                .short('F')
                .long("fixed-strings")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("count")
                .short('c')
                .long("count")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("count_matches")
                .long("count-matches")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("files_with_matches")
                .short('l')
                .long("files-with-matches")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("files_without_match")
                .long("files-without-match")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("only_matching")
                .short('o')
                .long("only-matching")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("quiet")
                .short('q')
                .long("quiet")
                .action(ArgAction::SetTrue),
        )
    }

    fn augment_args_for_update(cmd: Command) -> Command {
        Self::augment_args(cmd)
    }
}

impl FromArgMatches for SearchFlags {
    fn from_arg_matches(matches: &clap::ArgMatches) -> Result<Self, clap::Error> {
        let args: Vec<String> = std::env::args().collect();
        let case_mode = resolve_case_mode_from_args(&args);
        let fixed_strings = matches.get_flag("fixed_strings");

        Ok(Self {
            case_mode,
            fixed_strings,
        })
    }

    fn update_from_arg_matches(&mut self, matches: &clap::ArgMatches) -> Result<(), clap::Error> {
        *self = Self::from_arg_matches(matches)?;
        Ok(())
    }
}

impl SearchFlags {
    fn to_options(&self) -> SearchOptions {
        let mut flags = SearchMatchFlags::empty();
        if self.fixed_strings {
            flags |= SearchMatchFlags::FIXED_STRINGS;
        }
        SearchOptions {
            flags,
            case_mode: self.case_mode,
            max_results: None,
            ..SearchOptions::default()
        }
    }
}

fn resolve_patterns(
    regexp: &[String],
    pattern_file: Option<&Path>,
    pattern: Option<&str>,
) -> anyhow::Result<Vec<String>> {
    let mut patterns = Vec::new();
    for p in regexp {
        patterns.push(p.clone());
    }
    if let Some(file) = pattern_file {
        let content = std::fs::read_to_string(file)?;
        for line in content.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                patterns.push(trimmed.to_string());
            }
        }
    }
    if let Some(p) = pattern {
        patterns.push(p.to_string());
    }
    if patterns.is_empty() {
        anyhow::bail!("no pattern given");
    }
    Ok(patterns)
}

fn corpus_path_prefixes(
    index_root: &Path,
    cwd: &Path,
    requested: &[PathBuf],
) -> anyhow::Result<Vec<PathBuf>> {
    if requested.is_empty() {
        return Ok(vec![PathBuf::from("")]);
    }
    let mut out = Vec::new();
    for rel in requested {
        let abs = if rel.is_absolute() {
            rel.clone()
        } else {
            cwd.join(rel)
        };
        let abs = abs.canonicalize().unwrap_or(abs);
        let index_root = index_root
            .canonicalize()
            .unwrap_or_else(|_| index_root.to_path_buf());
        if !abs.starts_with(&index_root) {
            anyhow::bail!(
                "path {} is not under indexed corpus root {}",
                abs.display(),
                index_root.display()
            );
        }
        out.push(
            abs.strip_prefix(&index_root)
                .expect("prefix checked")
                .to_path_buf(),
        );
    }
    Ok(out)
}

/// Path scopes relative to `cwd` when searching without an on-disk index (walk mode).
fn walk_path_prefixes(cwd: &Path, requested: &[PathBuf]) -> anyhow::Result<Vec<PathBuf>> {
    if requested.is_empty() {
        return Ok(vec![PathBuf::from("")]);
    }
    let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let mut out = Vec::new();
    for rel in requested {
        let abs = if rel.is_absolute() {
            rel.clone()
        } else {
            cwd.join(rel)
        };
        let abs = abs.canonicalize().unwrap_or(abs);
        if !abs.starts_with(&cwd) {
            anyhow::bail!("path {} is not under {}", abs.display(), cwd.display());
        }
        out.push(
            abs.strip_prefix(&cwd)
                .expect("prefix checked")
                .to_path_buf(),
        );
    }
    Ok(out)
}

fn excluded_search_paths(search_root: &Path, sift_dir: &Path) -> Vec<PathBuf> {
    let abs = if sift_dir.is_absolute() {
        sift_dir.to_path_buf()
    } else {
        std::env::current_dir().map_or_else(|_| sift_dir.to_path_buf(), |cwd| cwd.join(sift_dir))
    };
    let abs = abs.canonicalize().unwrap_or(abs);
    let root = search_root
        .canonicalize()
        .unwrap_or_else(|_| search_root.to_path_buf());
    if abs.starts_with(&root) {
        vec![
            abs.strip_prefix(&root)
                .expect("prefix checked")
                .to_path_buf(),
        ]
    } else {
        Vec::new()
    }
}

const fn search_output(
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

fn build_line_style_flags(out: &SearchOutputCtx, line_number: bool) -> LineStyleFlags {
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

const fn effective_filename_mode(
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

fn effective_path_display(scopes: &[PathBuf]) -> PathDisplay {
    for scope in scopes {
        if scope.is_absolute() {
            return PathDisplay::Absolute;
        }
    }
    PathDisplay::Relative
}

#[derive(Clone, Copy)]
struct SearchModeCtx {
    effective_mode: SearchMode,
    quiet: bool,
}

/// Heading and filename resolution (`-H` / `--no-filename`, path-only modes).
#[derive(Clone, Copy)]
struct SearchLineResolveCtx {
    heading: bool,
    with_filename: Option<bool>,
    is_path_mode: bool,
    column: bool,
    line_number: Option<bool>,
}

#[derive(Clone, Copy)]
struct SearchFormatCtx {
    null_data: bool,
    color: ColorChoice,
}

/// Resolved output mode and line/format flags (from argv + clap) shared by index and walk search.
#[derive(Clone)]
#[allow(clippy::struct_excessive_bools)]
struct SearchOutputCtx {
    mode: SearchModeCtx,
    lines: SearchLineResolveCtx,
    format: SearchFormatCtx,
    output_format: SearchOutputFormat,
    separators: SearchSeparators,
    print_stats: bool,
    byte_offset: bool,
    trim: bool,
    include_zero: bool,
    path_separator: Option<u8>,
}

/// Resolved visibility, ignore sources, and glob case (from argv order + clap) for [`SearchFilterConfig`].
#[derive(Clone, Copy)]
struct SearchFilterCtx {
    hidden: bool,
    ignore_sources: IgnoreSources,
    require_git: bool,
    glob_case_insensitive: bool,
    msg_flags: MessageFlags,
}

impl SearchFilterCtx {
    #[inline]
    const fn hidden_mode(self) -> HiddenMode {
        if self.hidden {
            HiddenMode::Include
        } else {
            HiddenMode::Respect
        }
    }
}

fn parse_size_suffix(s: &str) -> anyhow::Result<u64> {
    let s = s.trim();
    let (num_part, suffix) = s.find(|c: char| c.is_ascii_alphabetic()).map_or_else(
        || (s, String::new()),
        |i| (&s[..i], s[i..].to_ascii_uppercase()),
    );
    let base: u64 = num_part
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid size: '{s}'"))?;
    let multiplier: u64 = match suffix.as_str() {
        "" | "B" => 1,
        "K" | "KB" => 1024,
        "M" | "MB" => 1024 * 1024,
        "G" | "GB" => 1024 * 1024 * 1024,
        _ => anyhow::bail!("unknown size suffix: '{suffix}'"),
    };
    Ok(base * multiplier)
}

fn builtin_type_defs() -> Vec<TypeDef> {
    [
        ("rust", &["*.rs"][..]),
        ("py", &["*.py", "*.pyi"]),
        ("js", &["*.js", "*.mjs", "*.cjs"]),
        ("ts", &["*.ts", "*.tsx", "*.mts", "*.cts"]),
        ("c", &["*.c", "*.h"]),
        ("cpp", &["*.cpp", "*.cc", "*.cxx", "*.hpp", "*.hxx", "*.h"]),
        ("java", &["*.java"]),
        ("go", &["*.go"]),
        ("html", &["*.html", "*.htm", "*.xhtml"]),
        ("css", &["*.css", "*.scss", "*.less"]),
        ("json", &["*.json", "*.jsonl"]),
        ("yaml", &["*.yaml", "*.yml"]),
        ("toml", &["*.toml"]),
        ("xml", &["*.xml", "*.xsl", "*.xslt", "*.svg"]),
        ("md", &["*.md", "*.markdown", "*.mdown"]),
        ("txt", &["*.txt"]),
        ("sh", &["*.sh", "*.bash", "*.zsh", "*.fish"]),
        ("ruby", &["*.rb", "*.erb", "*.gemspec", "Gemfile"]),
        ("php", &["*.php"]),
        ("swift", &["*.swift"]),
        ("kotlin", &["*.kt", "*.kts"]),
        ("scala", &["*.scala", "*.sbt"]),
        ("lua", &["*.lua"]),
        ("perl", &["*.pl", "*.pm"]),
        ("r", &["*.r", "*.R", "*.Rmd"]),
        ("sql", &["*.sql"]),
        ("protobuf", &["*.proto"]),
        ("make", &["Makefile", "*.mk"]),
        ("cmake", &["CMakeLists.txt", "*.cmake"]),
        ("docker", &["Dockerfile", "*.dockerfile"]),
        ("tf", &["*.tf", "*.tfvars"]),
        ("hcl", &["*.hcl"]),
        ("nix", &["*.nix"]),
        ("zig", &["*.zig"]),
        ("elixir", &["*.ex", "*.exs"]),
        ("erlang", &["*.erl", "*.hrl"]),
        ("haskell", &["*.hs", "*.lhs"]),
        ("ocaml", &["*.ml", "*.mli"]),
        ("clojure", &["*.clj", "*.cljs", "*.cljc", "*.edn"]),
        ("csv", &["*.csv", "*.tsv"]),
        ("log", &["*.log"]),
        ("config", &["*.cfg", "*.conf", "*.ini"]),
        ("lock", &["*.lock"]),
        ("graphql", &["*.graphql", "*.gql"]),
        ("wasm", &["*.wasm", "*.wat"]),
        ("csharp", &["*.cs"]),
        ("fsharp", &["*.fs", "*.fsi", "*.fsx"]),
        ("dart", &["*.dart"]),
        ("vim", &["*.vim"]),
        ("tex", &["*.tex", "*.sty", "*.cls"]),
        ("rst", &["*.rst"]),
        ("org", &["*.org"]),
        ("asm", &["*.asm", "*.s", "*.S"]),
        ("bazel", &["*.bzl", "BUILD", "WORKSPACE"]),
        ("readme", &["README*"]),
        ("license", &["LICENSE*", "LICENCE*"]),
    ]
    .into_iter()
    .map(|(name, globs)| TypeDef {
        name: name.to_string(),
        globs: globs.iter().map(|s| (*s).to_string()).collect(),
    })
    .collect()
}

fn build_search_filter_config(
    cli: &Cli,
    filter: SearchFilterCtx,
    scopes: Vec<PathBuf>,
    exclude_paths: Vec<PathBuf>,
) -> anyhow::Result<SearchFilterConfig> {
    let max_filesize = cli
        .filter_decl
        .max_filesize
        .as_ref()
        .map(|s| parse_size_suffix(s))
        .transpose()?;

    let mut glob_patterns = cli.glob_flags.glob.clone();
    for ig in &cli.filter_decl.iglob {
        glob_patterns.push(ig.clone());
    }

    let glob_ci = filter.glob_case_insensitive || !cli.filter_decl.iglob.is_empty();

    let type_definitions = resolve_type_defs(&cli.filter_decl);

    let custom_files = if filter.msg_flags.contains(MessageFlags::NO_IGNORE_FILES) {
        Vec::new()
    } else {
        cli.filter_decl.ignore_file.clone()
    };

    Ok(SearchFilterConfig {
        scopes,
        exclude_paths,
        glob: GlobConfig {
            patterns: glob_patterns,
            case_insensitive: glob_ci,
        },
        visibility: VisibilityConfig {
            hidden: filter.hidden_mode(),
            ignore: IgnoreConfig {
                sources: filter.ignore_sources,
                custom_files,
                require_git: filter.require_git,
            },
        },
        follow_links: cli.paths.follow,
        max_depth: cli.filter_decl.max_depth,
        max_filesize,
        type_definitions,
        type_include: cli.filter_decl.type_include.clone(),
        type_exclude: cli.filter_decl.type_exclude.clone(),
        one_file_system: cli.walker_decl.one_file_system,
    })
}

fn resolve_type_defs(decl: &FilterDecl) -> Vec<TypeDef> {
    let mut defs = builtin_type_defs();

    for spec in &decl.type_clear {
        defs.retain(|d| d.name != *spec);
    }

    for spec in &decl.type_add {
        if let Some((name, globs_str)) = spec.split_once(':') {
            if let Some(rest) = globs_str.strip_prefix("include:") {
                let includes: Vec<&str> = rest.split(',').collect();
                let mut new_globs = Vec::new();
                for inc_name in &includes {
                    for d in &defs {
                        if d.name == *inc_name {
                            new_globs.extend(d.globs.clone());
                        }
                    }
                }
                if let Some(existing) = defs.iter_mut().find(|d| d.name == name) {
                    existing.globs.extend(new_globs);
                } else {
                    defs.push(TypeDef {
                        name: name.to_string(),
                        globs: new_globs,
                    });
                }
            } else {
                let globs: Vec<String> =
                    globs_str.split(',').map(|s| s.trim().to_string()).collect();
                if let Some(existing) = defs.iter_mut().find(|d| d.name == name) {
                    existing.globs.extend(globs);
                } else {
                    defs.push(TypeDef {
                        name: name.to_string(),
                        globs,
                    });
                }
            }
        }
    }

    defs
}
fn write_search_stats(stats: &SearchStats) {
    eprintln!("{} matches", stats.matches);
    eprintln!("{} files contained matches", stats.files_with_matches);
    eprintln!("{} files searched", stats.files_searched);
    eprintln!("{} bytes printed", stats.bytes_printed);
    eprintln!("{} bytes searched", stats.bytes_searched);
    eprintln!("{:.6}s elapsed", stats.elapsed.as_secs_f64());
}

const fn resolve_effective_line_number(
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

impl Cli {
    fn resolve_separators(&self) -> SearchSeparators {
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

    fn build_filter_config(
        &self,
        filter: SearchFilterCtx,
        scopes: Vec<PathBuf>,
        exclude_paths: Vec<PathBuf>,
    ) -> anyhow::Result<SearchFilterConfig> {
        let max_filesize = self
            .filter_decl
            .max_filesize
            .as_ref()
            .map(|s| parse_size_suffix(s))
            .transpose()?;

        let mut glob_patterns = self.glob_flags.glob.clone();
        for ig in &self.filter_decl.iglob {
            glob_patterns.push(ig.clone());
        }

        let glob_ci = filter.glob_case_insensitive || !self.filter_decl.iglob.is_empty();

        let type_definitions = resolve_type_defs(&self.filter_decl);

        Ok(SearchFilterConfig {
            scopes,
            exclude_paths,
            glob: GlobConfig {
                patterns: glob_patterns,
                case_insensitive: glob_ci,
            },
            visibility: VisibilityConfig {
                hidden: filter.hidden_mode(),
                ignore: IgnoreConfig {
                    sources: filter.ignore_sources,
                    custom_files: if filter.msg_flags.contains(MessageFlags::NO_IGNORE_FILES) {
                        Vec::new()
                    } else {
                        self.filter_decl.ignore_file.clone()
                    },
                    require_git: filter.require_git,
                },
            },
            follow_links: self.paths.follow,
            max_depth: self.filter_decl.max_depth,
            max_filesize,
            type_definitions,
            type_include: self.filter_decl.type_include.clone(),
            type_exclude: self.filter_decl.type_exclude.clone(),
            one_file_system: self.walker_decl.one_file_system,
        })
    }

    const fn resolve_binary_mode(&self) -> BinaryMode {
        if self.binary_decl.text {
            BinaryMode::AsText
        } else if self.binary_decl.binary {
            BinaryMode::SearchBinary
        } else {
            BinaryMode::Quit
        }
    }

    fn build_search_opts(&self, args: &[String], only_matching: bool) -> SearchOptions {
        let (before_context, after_context) = resolve_context_from_args(args);
        let mut opts = self.search_flags.to_options();
        opts.max_results = self.paths.max_count;
        if self.regex1.invert_match {
            opts.flags |= SearchMatchFlags::INVERT_MATCH;
        }
        if self.regex1.word_regexp {
            opts.flags |= SearchMatchFlags::WORD_REGEXP;
        }
        if self.regex2.line_regexp {
            opts.flags |= SearchMatchFlags::LINE_REGEXP;
        }
        if only_matching {
            opts.flags |= SearchMatchFlags::ONLY_MATCHING;
        }
        if self.multiline_decl.multiline {
            opts.flags |= SearchMatchFlags::MULTILINE;
        }
        if self.multiline_decl.multiline_dotall {
            opts.flags |= SearchMatchFlags::MULTILINE_DOTALL;
        }
        if self.multiline_decl.crlf {
            opts.flags |= SearchMatchFlags::CRLF;
        }
        opts.replace.clone_from(&self.replace_decl.replace);
        opts.before_context = before_context;
        opts.after_context = after_context;
        if self.replace_decl.passthru {
            opts.before_context = usize::MAX;
            opts.after_context = usize::MAX;
        }
        if only_matching {
            opts.before_context = 0;
            opts.after_context = 0;
        }
        opts.binary_mode = self.resolve_binary_mode();
        let _ = args;
        opts
    }

    fn build_output_and_filter(
        &self,
        args: &[String],
        effective_mode: SearchMode,
        quiet: bool,
        line_number_override: Option<bool>,
    ) -> (SearchOutputCtx, SearchFilterCtx) {
        let glob_case_insensitive = resolve_glob_case_insensitive_from_args(args);
        let ignore_res = resolve_visibility_and_ignore(args);
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
        };
        let filter = SearchFilterCtx {
            hidden: ignore_res.hidden,
            ignore_sources: ignore_res.sources,
            require_git: ignore_res.require_git,
            glob_case_insensitive,
            msg_flags: ignore_res.msg_flags,
        };
        (out, filter)
    }

    fn run_search_with_index(
        &self,
        query: &CompiledSearch,
        index: &Index,
        cwd: &Path,
        out: &SearchOutputCtx,
        filter: SearchFilterCtx,
    ) -> anyhow::Result<bool> {
        let prefixes = corpus_path_prefixes(&index.root, cwd, &self.search_scope.paths)?;
        let exclude_paths = excluded_search_paths(&index.root, &self.paths.sift_dir);
        let corpus_is_single_file = matches!(index.corpus_kind, sift_core::CorpusKind::File { .. });
        let filename_mode = effective_filename_mode(
            out.lines.with_filename,
            out.lines.is_path_mode,
            corpus_is_single_file,
        );
        let path_display = effective_path_display(&self.search_scope.paths);
        let line_number = resolve_effective_line_number(
            self.line_number_decl.line_number,
            out.lines.line_number,
            out.output_format,
        );
        let line_flags = build_line_style_flags(out, line_number);
        let output = search_output(
            out.output_format,
            out.mode.effective_mode,
            out.mode.quiet,
            SearchLineStyle {
                filename_mode,
                flags: line_flags,
                path_display,
            },
            SearchRecordStyle {
                null_data: out.format.null_data,
                color: out.format.color,
                path_separator: out.path_separator,
            },
            out.include_zero,
        );
        let filter_config = self.build_filter_config(filter, prefixes, exclude_paths)?;
        let search_filter = SearchFilter::new(&filter_config, &index.root)?;
        if out.print_stats {
            let mut stats = SearchStats::default();
            let ok = query.run_index_with_stats(
                index,
                &search_filter,
                output,
                &out.separators,
                &mut stats,
            )?;
            write_search_stats(&stats);
            return Ok(ok);
        }
        query
            .run_index(index, &search_filter, output, &out.separators)
            .map_err(Into::into)
    }

    fn run_search_walk(
        &self,
        query: &CompiledSearch,
        filter_root: &Path,
        out: &SearchOutputCtx,
        filter: SearchFilterCtx,
    ) -> anyhow::Result<bool> {
        let prefixes = walk_path_prefixes(filter_root, &self.search_scope.paths)?;
        let exclude_paths = excluded_search_paths(filter_root, &self.paths.sift_dir);
        let filename_mode =
            effective_filename_mode(out.lines.with_filename, out.lines.is_path_mode, false);
        let path_display = effective_path_display(&self.search_scope.paths);
        let line_number = resolve_effective_line_number(
            self.line_number_decl.line_number,
            out.lines.line_number,
            out.output_format,
        );
        let line_flags = build_line_style_flags(out, line_number);
        let output = search_output(
            out.output_format,
            out.mode.effective_mode,
            out.mode.quiet,
            SearchLineStyle {
                filename_mode,
                flags: line_flags,
                path_display,
            },
            SearchRecordStyle {
                null_data: out.format.null_data,
                color: out.format.color,
                path_separator: out.path_separator,
            },
            out.include_zero,
        );
        let filter_config = self.build_filter_config(filter, prefixes, exclude_paths)?;
        let search_filter = SearchFilter::new(&filter_config, filter_root)?;
        if out.print_stats {
            let mut stats = SearchStats::default();
            let ok = query.run_walk_with_stats(
                filter_root,
                &search_filter,
                output,
                &out.separators,
                &mut stats,
            )?;
            write_search_stats(&stats);
            return Ok(ok);
        }
        query
            .run_walk(filter_root, &search_filter, output, &out.separators)
            .map_err(Into::into)
    }

    fn run_search(&self) -> anyhow::Result<bool> {
        let patterns = resolve_patterns(
            &self.patterns.regexp,
            self.patterns.pattern_file.as_deref(),
            self.patterns.pattern.as_deref(),
        )?;

        let args: Vec<String> = std::env::args().collect();
        let invert_match = resolve_invert_match_from_args(&args);
        let (mode, only_matching, quiet) = resolve_output_mode(&args, invert_match);
        let use_json = resolve_json_from_args(&args);

        let pretty = self.column_decl.pretty;
        let vimgrep = self.column_decl.vimgrep;

        let line_number_override = if pretty || vimgrep {
            Some(true)
        } else {
            resolve_line_number_from_args(&args)
        };

        let effective_mode = if only_matching {
            SearchMode::OnlyMatching
        } else {
            mode
        };

        if use_json {
            match effective_mode {
                SearchMode::Count
                | SearchMode::CountMatches
                | SearchMode::FilesWithMatches
                | SearchMode::FilesWithoutMatch => {
                    anyhow::bail!(
                        "sift: --json cannot be used with --count, --count-matches, --files-with-matches, or --files-without-match"
                    );
                }
                SearchMode::Standard | SearchMode::OnlyMatching => {}
            }
        }

        if let Some(threads) = self.threading.threads {
            rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build_global()
                .ok();
        }

        let opts = self.build_search_opts(&args, only_matching);
        let query = CompiledSearch::new(&patterns, opts).map_err(|e| anyhow::anyhow!("{e}"))?;
        let cwd = std::env::current_dir()?;

        let (out, filter) =
            self.build_output_and_filter(&args, effective_mode, quiet, line_number_override);

        match Index::open(&self.paths.sift_dir) {
            Ok(index) => self.run_search_with_index(&query, &index, &cwd, &out, filter),
            Err(
                SiftError::MissingMeta(_)
                | SiftError::MissingComponent(_)
                | SiftError::InvalidMeta(_),
            ) => {
                let filter_root = cwd.canonicalize().map_err(|e| anyhow::anyhow!("{e}"))?;
                self.run_search_walk(&query, &filter_root, &out, filter)
            }
            Err(e) => Err(anyhow::anyhow!("{e}")),
        }
    }
}

fn run_type_list(cli: &Cli) {
    let defs = resolve_type_defs(&cli.filter_decl);
    let mut sorted = defs;
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    for def in &sorted {
        println!("{}: {}", def.name, def.globs.join(", "));
    }
}

fn run_files_mode(cli: &Cli) -> anyhow::Result<bool> {
    let args: Vec<String> = std::env::args().collect();
    let glob_case_insensitive = resolve_glob_case_insensitive_from_args(&args);
    let ignore_res = resolve_visibility_and_ignore(&args);
    let null_data = resolve_null_from_args(&args);

    let filter_ctx = SearchFilterCtx {
        hidden: ignore_res.hidden,
        ignore_sources: ignore_res.sources,
        require_git: ignore_res.require_git,
        glob_case_insensitive,
        msg_flags: ignore_res.msg_flags,
    };

    if let Some(threads) = cli.threading.threads {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build_global()
            .ok();
    }

    let cwd = std::env::current_dir()?;

    let (filter_root, scopes, exclude_paths) = if let Ok(index) = Index::open(&cli.paths.sift_dir) {
        let prefixes = corpus_path_prefixes(&index.root, &cwd, &cli.search_scope.paths)?;
        let excludes = excluded_search_paths(&index.root, &cli.paths.sift_dir);
        (index.root, prefixes, excludes)
    } else {
        let root = cwd.canonicalize()?;
        let prefixes = walk_path_prefixes(&root, &cli.search_scope.paths)?;
        let excludes = excluded_search_paths(&root, &cli.paths.sift_dir);
        (root, prefixes, excludes)
    };

    let filter_config = build_search_filter_config(cli, filter_ctx, scopes, exclude_paths)?;
    let search_filter = SearchFilter::new(&filter_config, &filter_root)?;

    let paths = sift_core::walk_file_paths(&filter_root, search_filter.follow_links())?;
    let mut sorted_paths: Vec<_> = paths
        .into_iter()
        .filter(|p| search_filter.is_candidate(p))
        .collect();
    sorted_paths.sort();
    let sep = if null_data { '\0' } else { '\n' };
    let mut any = false;
    for p in &sorted_paths {
        any = true;
        let display = filter_root.join(p);
        print!("{}{sep}", display.display());
    }
    Ok(any)
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    if cli.filter_decl.type_list {
        run_type_list(&cli);
        return ExitCode::SUCCESS;
    }

    if let Some(Commands::Build { path }) = cli.command {
        return match IndexBuilder::new(&path)
            .with_dir(&cli.paths.sift_dir)
            .with_follow_links(cli.paths.follow)
            .build()
        {
            Ok(_) => {
                eprintln!(
                    "indexed corpus {} → {}",
                    path.display(),
                    cli.paths.sift_dir.display()
                );
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("sift: {e}");
                ExitCode::from(2)
            }
        };
    }

    if cli.filter_decl.files {
        return match run_files_mode(&cli) {
            Ok(true) => ExitCode::SUCCESS,
            Ok(false) => ExitCode::from(1),
            Err(e) => {
                eprintln!("sift: {e}");
                ExitCode::from(2)
            }
        };
    }

    let args: Vec<String> = std::env::args().collect();
    let no_messages = resolve_visibility_and_ignore(&args)
        .msg_flags
        .contains(MessageFlags::NO_MESSAGES);

    match cli.run_search() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            if let Some(ioe) = e.downcast_ref::<std::io::Error>()
                && ioe.kind() == std::io::ErrorKind::BrokenPipe
            {
                return ExitCode::SUCCESS;
            }
            if !no_messages {
                eprintln!("sift: {e}");
            }
            ExitCode::from(2)
        }
    }
}

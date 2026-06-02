use std::path::PathBuf;

use clap::{Arg, ArgAction, ArgMatches, Args, Command, FromArgMatches, value_parser};
use sift_core::{BinaryMode, CaseMode, SearchMatchFlags, SearchMode, SearchOptions};

use super::argv::Argv;
use super::engine::{EngineDecl, MultilineDecl};
use super::filter::parse_size_suffix;
use super::output::ReplaceDecl;

#[derive(Args, Clone)]
pub struct PatternArgs {
    #[arg(short = 'e', long = "regexp", value_name = "PATTERN")]
    pub regexp: Vec<String>,
    #[arg(short = 'f', long = "file", value_name = "FILE")]
    pub pattern_file: Option<PathBuf>,
    #[arg(value_name = "PATTERN")]
    pub pattern: Option<String>,
}

#[derive(Args)]
pub struct SearchScope {
    #[arg(value_name = "PATH", num_args = 0..)]
    pub paths: Vec<PathBuf>,
}

#[derive(Args, Clone)]
pub struct RegexFlagsA {
    #[arg(short = 'v', long)]
    pub invert_match: bool,
    #[arg(short = 'w', long)]
    pub word_regexp: bool,
}

#[derive(Args, Clone)]
pub struct RegexFlagsB {
    #[arg(short = 'x', long)]
    pub line_regexp: bool,
}

#[derive(Args, Clone)]
pub struct BinaryDecl {
    #[arg(short = 'a', long = "text")]
    pub text: bool,
    #[arg(long = "binary")]
    pub binary: bool,
}

/// Pattern and engine flags resolved from clap declarations.
#[derive(Clone)]
pub struct PatternConfig {
    pub patterns: PatternArgs,
    pub search_flags: SearchFlags,
    pub regex1: RegexFlagsA,
    pub regex2: RegexFlagsB,
    pub multiline: MultilineDecl,
    pub engine: EngineDecl,
    pub binary: BinaryDecl,
    pub replace: ReplaceDecl,
    pub max_count: Option<usize>,
}

#[derive(Clone)]
pub struct SearchFlags {
    pub case_mode: CaseMode,
    pub fixed_strings: bool,
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
    fn from_arg_matches(matches: &ArgMatches) -> Result<Self, clap::Error> {
        Ok(Self {
            // Case mode is resolved from argv at run time via `PatternArgv`.
            case_mode: CaseMode::Sensitive,
            fixed_strings: matches.get_flag("fixed_strings"),
        })
    }

    fn update_from_arg_matches(&mut self, matches: &ArgMatches) -> Result<(), clap::Error> {
        *self = Self::from_arg_matches(matches)?;
        Ok(())
    }
}

// ── Argv-order resolution ──

/// Pattern and search-mode flags resolved from raw argv (ripgrep last-wins).
pub struct PatternArgv {
    pub case_mode: CaseMode,
    pub invert_match: bool,
    pub mode: SearchMode,
    pub only_matching: bool,
    pub quiet: bool,
    pub before_context: usize,
    pub after_context: usize,
}

impl PatternArgv {
    #[must_use]
    pub fn resolve(argv: &Argv<'_>) -> Self {
        let invert_match = Self::invert_match(argv);
        let (mode, only_matching, quiet) = Self::output_mode(argv, invert_match);
        let (before_context, after_context) = Self::context(argv);
        Self {
            case_mode: Self::case_mode(argv),
            invert_match,
            mode,
            only_matching,
            quiet,
            before_context,
            after_context,
        }
    }

    fn case_mode(argv: &Argv<'_>) -> CaseMode {
        let mut last_idx = 0usize;
        let mut result = CaseMode::Sensitive;
        let case_flags = [
            ("ci", CaseMode::Insensitive),
            ("cs", CaseMode::Sensitive),
            ("sc", CaseMode::Smart),
        ];
        for (i, arg) in argv.as_slice().iter().enumerate() {
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

    fn invert_match(argv: &Argv<'_>) -> bool {
        for arg in argv.as_slice() {
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

    /// Effective search mode tuple for a given invert-match override.
    #[must_use]
    pub fn output_mode(argv: &Argv<'_>, invert_match: bool) -> (SearchMode, bool, bool) {
        let mut last_idx = 0usize;
        let mut mode = SearchMode::Standard;
        let mut quiet = false;
        let mut saw_only_matching = false;

        for (i, arg) in argv.as_slice().iter().enumerate() {
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

    /// Context line counts from `-A`/`-B`/`-C` flags.
    #[must_use]
    pub fn context(argv: &Argv<'_>) -> (usize, usize) {
        let mut before = 0usize;
        let mut after = 0usize;
        let mut i = 0usize;
        let tokens = argv.as_slice();
        while i < tokens.len() {
            let arg = tokens[i].as_str();
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
                    if let Some(n) = tokens.get(i + 1).and_then(|s| parse_usize_token(s)) {
                        after = n;
                        i += 2;
                        continue;
                    }
                }
                "-B" | "--before-context" => {
                    if let Some(n) = tokens.get(i + 1).and_then(|s| parse_usize_token(s)) {
                        before = n;
                        i += 2;
                        continue;
                    }
                }
                "-C" | "--context" => {
                    if let Some(n) = tokens.get(i + 1).and_then(|s| parse_usize_token(s)) {
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
}

fn parse_usize_token(s: &str) -> Option<usize> {
    s.parse().ok()
}

/// Patterns resolved from `-e`/`-f`/positional clap declarations.
#[derive(Debug)]
pub struct ResolvedPatterns(pub Vec<String>);

impl ResolvedPatterns {
    /// # Errors
    ///
    /// Returns an error if no pattern is provided or if a pattern file cannot be read.
    pub fn resolve(config: &PatternConfig) -> anyhow::Result<Self> {
        let mut patterns = Vec::new();
        for p in &config.patterns.regexp {
            patterns.push(p.clone());
        }
        if let Some(file) = config.patterns.pattern_file.as_deref() {
            let content = std::fs::read_to_string(file)?;
            for line in content.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() && !trimmed.starts_with('#') {
                    patterns.push(trimmed.to_string());
                }
            }
        }
        if let Some(p) = config.patterns.pattern.as_deref() {
            patterns.push(p.to_string());
        }
        if patterns.is_empty() {
            anyhow::bail!("no pattern given");
        }
        Ok(Self(patterns))
    }

    #[must_use]
    pub fn as_slice(&self) -> &[String] {
        &self.0
    }
}

// ── Search option builders ──

#[must_use]
pub const fn binary_mode(config: &PatternConfig) -> BinaryMode {
    if config.binary.text {
        BinaryMode::AsText
    } else if config.binary.binary {
        BinaryMode::SearchBinary
    } else {
        BinaryMode::Quit
    }
}

#[must_use]
pub fn search_options(
    config: &PatternConfig,
    pattern_argv: &PatternArgv,
    only_matching: bool,
) -> SearchOptions {
    let mut opts = SearchOptions {
        case_mode: pattern_argv.case_mode,
        max_results: config.max_count,
        ..SearchOptions::default()
    };
    if config.search_flags.fixed_strings {
        opts.flags |= SearchMatchFlags::FIXED_STRINGS;
    }
    if pattern_argv.invert_match {
        opts.flags |= SearchMatchFlags::INVERT_MATCH;
    }
    if config.regex1.word_regexp {
        opts.flags |= SearchMatchFlags::WORD_REGEXP;
    }
    if config.regex2.line_regexp {
        opts.flags |= SearchMatchFlags::LINE_REGEXP;
    }
    if only_matching {
        opts.flags |= SearchMatchFlags::ONLY_MATCHING;
    }
    if config.multiline.multiline {
        opts.flags |= SearchMatchFlags::MULTILINE;
    }
    if config.multiline.multiline_dotall {
        opts.flags |= SearchMatchFlags::MULTILINE_DOTALL;
    }
    if config.multiline.crlf {
        opts.flags |= SearchMatchFlags::CRLF;
    }
    if config.engine.no_unicode {
        opts.unicode = false;
    } else if config.engine.unicode {
        opts.unicode = true;
    }
    if let Some(ref limit) = config.engine.regex_size_limit
        && let Ok(bytes) = parse_size_suffix(limit)
    {
        opts.regex_size_limit = usize::try_from(bytes).unwrap_or(usize::MAX);
    }
    if let Some(ref limit) = config.engine.dfa_size_limit
        && let Ok(bytes) = parse_size_suffix(limit)
    {
        opts.dfa_size_limit = usize::try_from(bytes).unwrap_or(usize::MAX);
    }
    opts.replace.clone_from(&config.replace.replace);
    opts.before_context = pattern_argv.before_context;
    opts.after_context = pattern_argv.after_context;
    if config.replace.passthru {
        opts.before_context = usize::MAX;
        opts.after_context = usize::MAX;
    }
    if only_matching {
        opts.before_context = 0;
        opts.after_context = 0;
    }
    opts.binary_mode = binary_mode(config);
    opts
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(ToString::to_string).collect()
    }

    fn pat(items: &[&str]) -> PatternArgv {
        PatternArgv::resolve(&Argv::new(&args(items)))
    }

    fn pattern_config(args: &[&str]) -> PatternConfig {
        crate::cli::Cli::try_parse_from(args)
            .unwrap()
            .pattern_config()
    }

    // ── PatternArgv case mode ──

    #[test]
    fn case_mode_default_sensitive() {
        assert_eq!(
            PatternArgv::resolve(&Argv::new(&args(&["sift", "pat"]))).case_mode,
            CaseMode::Sensitive
        );
    }

    #[test]
    fn case_mode_ignore_case_short() {
        assert_eq!(
            PatternArgv::resolve(&Argv::new(&args(&["sift", "-i", "pat"]))).case_mode,
            CaseMode::Insensitive
        );
    }

    #[test]
    fn case_mode_case_sensitive_short() {
        // -s is --case-sensitive, but resolves via short check
        assert_eq!(
            PatternArgv::resolve(&Argv::new(&args(&["sift", "-s", "pat"]))).case_mode,
            CaseMode::Sensitive
        );
    }

    #[test]
    fn case_mode_smart_case_short() {
        assert_eq!(
            PatternArgv::resolve(&Argv::new(&args(&["sift", "-S", "pat"]))).case_mode,
            CaseMode::Smart
        );
    }

    #[test]
    fn case_mode_ignore_case_long() {
        assert_eq!(
            PatternArgv::resolve(&Argv::new(&args(&["sift", "--ignore-case", "pat"]))).case_mode,
            CaseMode::Insensitive
        );
    }

    #[test]
    fn case_mode_case_sensitive_long() {
        assert_eq!(
            PatternArgv::resolve(&Argv::new(&args(&["sift", "--case-sensitive", "pat"]))).case_mode,
            CaseMode::Sensitive
        );
    }

    #[test]
    fn case_mode_smart_case_long() {
        assert_eq!(
            PatternArgv::resolve(&Argv::new(&args(&["sift", "--smart-case", "pat"]))).case_mode,
            CaseMode::Smart
        );
    }

    #[test]
    fn case_mode_last_wins_ignore_then_sensitive() {
        assert_eq!(
            PatternArgv::resolve(&Argv::new(&args(&["sift", "-i", "-s", "pat"]))).case_mode,
            CaseMode::Sensitive
        );
    }

    #[test]
    fn case_mode_last_wins_smart_then_ignore() {
        assert_eq!(
            PatternArgv::resolve(&Argv::new(&args(&["sift", "-S", "-i", "pat"]))).case_mode,
            CaseMode::Insensitive
        );
    }

    // ── resolve_invert_match_from_args ──

    #[test]
    fn invert_match_no_flag() {
        assert!(!pat(&["sift", "pat"]).invert_match);
    }

    #[test]
    fn invert_match_short() {
        assert!(pat(&["sift", "-v", "pat"]).invert_match);
    }

    #[test]
    fn invert_match_long() {
        assert!(pat(&["sift", "--invert-match", "pat"]).invert_match);
    }

    #[test]
    fn invert_match_dash_dash_terminates() {
        assert!(!pat(&["sift", "--", "-v", "pat"]).invert_match);
    }

    #[test]
    fn invert_match_flag_before_dash_dash() {
        assert!(pat(&["sift", "-v", "--", "pat"]).invert_match);
    }

    #[test]
    fn output_mode_default() {
        let (mode, only_matching, quiet) =
            PatternArgv::output_mode(&Argv::new(&args(&["sift", "pat"])), false);
        assert_eq!(mode, SearchMode::Standard);
        assert!(!only_matching);
        assert!(!quiet);
    }

    #[test]
    fn output_mode_count() {
        let (mode, only_matching, quiet) =
            PatternArgv::output_mode(&Argv::new(&args(&["sift", "-c", "pat"])), false);
        assert_eq!(mode, SearchMode::Count);
        assert!(!only_matching);
        assert!(!quiet);
    }

    #[test]
    fn output_mode_count_matches() {
        let (mode, only_matching, quiet) = PatternArgv::output_mode(
            &Argv::new(&args(&["sift", "--count-matches", "pat"])),
            false,
        );
        assert_eq!(mode, SearchMode::CountMatches);
        assert!(!only_matching);
        assert!(!quiet);
    }

    #[test]
    fn output_mode_files_with_matches() {
        let (mode, only_matching, quiet) =
            PatternArgv::output_mode(&Argv::new(&args(&["sift", "-l", "pat"])), false);
        assert_eq!(mode, SearchMode::FilesWithMatches);
        assert!(!only_matching);
        assert!(!quiet);
    }

    #[test]
    fn output_mode_files_without_match() {
        let (mode, only_matching, quiet) = PatternArgv::output_mode(
            &Argv::new(&args(&["sift", "--files-without-match", "pat"])),
            false,
        );
        assert_eq!(mode, SearchMode::FilesWithoutMatch);
        assert!(!only_matching);
        assert!(!quiet);
    }

    #[test]
    fn output_mode_only_matching() {
        let (mode, only_matching, quiet) =
            PatternArgv::output_mode(&Argv::new(&args(&["sift", "-o", "pat"])), false);
        assert_eq!(mode, SearchMode::Standard);
        assert!(only_matching);
        assert!(!quiet);
    }

    #[test]
    fn output_mode_quiet() {
        let (mode, only_matching, quiet) =
            PatternArgv::output_mode(&Argv::new(&args(&["sift", "-q", "pat"])), false);
        assert_eq!(mode, SearchMode::Standard);
        assert!(!only_matching);
        assert!(quiet);
    }

    #[test]
    fn output_mode_count_and_only_matching_becomes_count_matches() {
        let (mode, only_matching, _quiet) =
            PatternArgv::output_mode(&Argv::new(&args(&["sift", "-c", "-o", "pat"])), false);
        assert_eq!(mode, SearchMode::CountMatches);
        assert!(!only_matching);
    }

    #[test]
    fn output_mode_last_wins() {
        let (mode, _, _) =
            PatternArgv::output_mode(&Argv::new(&args(&["sift", "-c", "-l", "pat"])), false);
        assert_eq!(mode, SearchMode::FilesWithMatches);
    }

    #[test]
    fn output_mode_invert_match_downgrades_only_matching_to_count_matches() {
        let (mode, only_matching, _) =
            PatternArgv::output_mode(&Argv::new(&args(&["sift", "-o", "pat"])), true);
        // invert_match + only_matching = CountMatches (saw_only_matching persists)
        assert_eq!(mode, SearchMode::CountMatches);
        assert!(!only_matching);
    }

    #[test]
    fn output_mode_invert_match_with_only_matching_and_count() {
        let (mode, only_matching, _) =
            PatternArgv::output_mode(&Argv::new(&args(&["sift", "-c", "-o", "pat"])), true);
        assert_eq!(mode, SearchMode::CountMatches);
        assert!(!only_matching);
    }

    #[test]
    fn output_mode_only_matching_flag_after_count() {
        // -c -o: only_matching after count → CountMatches (saw_only_matching true)
        let (mode, only_matching, _) =
            PatternArgv::output_mode(&Argv::new(&args(&["sift", "-c", "-o", "pat"])), false);
        assert_eq!(mode, SearchMode::CountMatches);
        assert!(!only_matching);
    }

    // ── resolve_patterns ──

    #[test]
    fn resolve_patterns_empty_error() {
        let err = ResolvedPatterns::resolve(&pattern_config(&["sift"])).unwrap_err();
        assert!(err.to_string().contains("no pattern"));
    }

    #[test]
    fn resolve_patterns_regexp() {
        let patterns =
            ResolvedPatterns::resolve(&pattern_config(&["sift", "-e", "foo", "-e", "bar"]))
                .unwrap()
                .0;
        assert_eq!(patterns, vec!["foo", "bar"]);
    }

    #[test]
    fn resolve_patterns_positional() {
        let patterns = ResolvedPatterns::resolve(&pattern_config(&["sift", "baz"]))
            .unwrap()
            .0;
        assert_eq!(patterns, vec!["baz"]);
    }

    #[test]
    fn resolve_patterns_regexp_and_positional() {
        let patterns = ResolvedPatterns::resolve(&pattern_config(&["sift", "-e", "foo", "bar"]))
            .unwrap()
            .0;
        assert_eq!(patterns, vec!["foo", "bar"]);
    }

    // ── SearchFlags / search_options ──

    #[test]
    fn search_options_case_mode_from_argv() {
        let config = pattern_config(&["sift", "pat"]);
        let opts = search_options(&config, &pat(&["sift", "-i", "pat"]), false);
        assert!(matches!(opts.case_mode, CaseMode::Insensitive));
    }

    #[test]
    fn search_options_applies_fixed_strings() {
        let config = pattern_config(&["sift", "-F", "pat"]);
        let opts = search_options(&config, &pat(&["sift", "pat"]), false);
        assert!(opts.flags.contains(SearchMatchFlags::FIXED_STRINGS));
    }

    // ── binary_mode ──

    #[test]
    fn binary_mode_default_quit() {
        assert!(matches!(
            binary_mode(&pattern_config(&["sift", "pat"])),
            BinaryMode::Quit
        ));
    }

    #[test]
    fn binary_mode_text() {
        assert!(matches!(
            binary_mode(&pattern_config(&["sift", "-a", "pat"])),
            BinaryMode::AsText
        ));
    }

    #[test]
    fn binary_mode_binary() {
        assert!(matches!(
            binary_mode(&pattern_config(&["sift", "--binary", "pat"])),
            BinaryMode::SearchBinary
        ));
    }
}

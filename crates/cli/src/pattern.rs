use std::path::{Path, PathBuf};

use clap::{Arg, ArgAction, ArgMatches, Args, Command, FromArgMatches, value_parser};
use sift_core::{BinaryMode, CaseMode, SearchMatchFlags, SearchMode, SearchOptions};

use crate::cli::Cli;
use crate::filter::parse_size_suffix;
use crate::output::resolve_context_from_args;

#[derive(Args)]
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

#[derive(Args)]
pub struct RegexFlagsA {
    #[arg(short = 'v', long)]
    pub invert_match: bool,
    #[arg(short = 'w', long)]
    pub word_regexp: bool,
}

#[derive(Args)]
pub struct RegexFlagsB {
    #[arg(short = 'x', long)]
    pub line_regexp: bool,
}

#[derive(Args)]
pub struct BinaryDecl {
    #[arg(short = 'a', long = "text")]
    pub text: bool,
    #[arg(long = "binary")]
    pub binary: bool,
}

#[derive(Clone)]
pub struct SearchFlags {
    pub case_mode: CaseMode,
    pub fixed_strings: bool,
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
        let args: Vec<String> = std::env::args().collect();
        let case_mode = resolve_case_mode_from_args(&args);
        let fixed_strings = matches.get_flag("fixed_strings");

        Ok(Self {
            case_mode,
            fixed_strings,
        })
    }

    fn update_from_arg_matches(&mut self, matches: &ArgMatches) -> Result<(), clap::Error> {
        *self = Self::from_arg_matches(matches)?;
        Ok(())
    }
}

// ── Argv-order resolvers ──

pub fn resolve_case_mode_from_args(args: &[String]) -> CaseMode {
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

pub fn resolve_invert_match_from_args(args: &[String]) -> bool {
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

pub fn resolve_output_mode(args: &[String], invert_match: bool) -> (SearchMode, bool, bool) {
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

pub fn resolve_patterns(
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

// ── Cli method implementations ──

impl Cli {
    pub const fn resolve_binary_mode(&self) -> BinaryMode {
        if self.binary_decl.text {
            BinaryMode::AsText
        } else if self.binary_decl.binary {
            BinaryMode::SearchBinary
        } else {
            BinaryMode::Quit
        }
    }

    pub fn build_search_opts(&self, args: &[String], only_matching: bool) -> SearchOptions {
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
        if self.engine_decl.no_unicode {
            opts.unicode = false;
        } else if self.engine_decl.unicode {
            opts.unicode = true;
        }
        if let Some(ref limit) = self.engine_decl.regex_size_limit
            && let Ok(bytes) = parse_size_suffix(limit)
        {
            opts.regex_size_limit = usize::try_from(bytes).unwrap_or(usize::MAX);
        }
        if let Some(ref limit) = self.engine_decl.dfa_size_limit
            && let Ok(bytes) = parse_size_suffix(limit)
        {
            opts.dfa_size_limit = usize::try_from(bytes).unwrap_or(usize::MAX);
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
        opts
    }
}

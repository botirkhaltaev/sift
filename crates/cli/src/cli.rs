use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::engine::{EngineDecl, MultilineDecl, ThreadingDecl, WalkerDecl};
use crate::filter::{FilterDecl, GlobFlags};
use crate::ignore::{
    ContextDecl, IgnoreDotDecl, IgnoreExcludeDecl, IgnoreFilesDecl, IgnoreGitDecl,
    IgnoreGlobalDecl, IgnoreMessagesDecl, IgnoreNoDecl, IgnoreParentDecl, IgnoreVcsDecl,
    MessagesDecl, UnrestrictedDecl,
};
use crate::output::{
    ColumnDecl, ColumnsDecl, ExtraOutputDecl, FilenameDecl, HeadingDecl, JsonDecl, LineNumberDecl,
    NullColorDecl, ReplaceDecl, SeparatorDecl, StatsDecl,
};
use crate::paths::PathArgs;
use crate::pattern::{BinaryDecl, PatternArgs, RegexFlagsA, RegexFlagsB, SearchFlags, SearchScope};

#[derive(Parser)]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    #[command(flatten)]
    pub patterns: PatternArgs,
    #[command(flatten)]
    pub search_scope: SearchScope,
    #[command(flatten)]
    pub regex1: RegexFlagsA,
    #[command(flatten)]
    pub regex2: RegexFlagsB,
    #[command(flatten)]
    pub line_number_decl: LineNumberDecl,
    #[command(flatten)]
    pub search_flags: SearchFlags,
    #[command(flatten)]
    pub filename_decl: FilenameDecl,
    #[command(flatten)]
    pub heading_decl: HeadingDecl,
    #[command(flatten)]
    pub column_decl: ColumnDecl,
    #[command(flatten)]
    pub columns_decl: ColumnsDecl,
    #[command(flatten)]
    pub glob_flags: GlobFlags,
    #[command(flatten)]
    pub ignore_no: IgnoreNoDecl,
    #[command(flatten)]
    pub ignore_vcs: IgnoreVcsDecl,
    #[command(flatten)]
    pub ignore_dot: IgnoreDotDecl,
    #[command(flatten)]
    pub ignore_git: IgnoreGitDecl,
    #[command(flatten)]
    pub ignore_global: IgnoreGlobalDecl,
    #[command(flatten)]
    pub ignore_exclude: IgnoreExcludeDecl,
    #[command(flatten)]
    pub ignore_parent: IgnoreParentDecl,
    #[command(flatten)]
    pub ignore_files_decl: IgnoreFilesDecl,
    #[command(flatten)]
    pub messages_decl: MessagesDecl,
    #[command(flatten)]
    pub ignore_messages_decl: IgnoreMessagesDecl,
    #[command(flatten)]
    pub unrestricted: UnrestrictedDecl,
    #[command(flatten)]
    pub context_decl: ContextDecl,
    #[command(flatten)]
    pub null_color: NullColorDecl,
    #[command(flatten)]
    pub paths: PathArgs,
    #[command(flatten)]
    pub stats_decl: StatsDecl,
    #[command(flatten)]
    pub json_decl: JsonDecl,
    #[command(flatten)]
    pub separator_decl: SeparatorDecl,
    #[command(flatten)]
    pub filter_decl: FilterDecl,
    #[command(flatten)]
    pub binary_decl: BinaryDecl,
    #[command(flatten)]
    pub replace_decl: ReplaceDecl,
    #[command(flatten)]
    pub extra_output: ExtraOutputDecl,
    #[command(flatten)]
    pub threading: ThreadingDecl,
    #[command(flatten)]
    pub multiline_decl: MultilineDecl,
    #[command(flatten)]
    pub walker_decl: WalkerDecl,
    #[command(flatten)]
    pub engine_decl: EngineDecl,
}

#[derive(Subcommand)]
pub enum Commands {
    Build {
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Comma-separated index kinds to build (default: all).
        /// Available: trigram
        #[arg(short, long, value_delimiter = ',')]
        indexes: Option<Vec<sift_core::IndexKind>>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::path::Path;

    #[test]
    fn cli_parses_positional_pattern() {
        let cli = Cli::try_parse_from(["sift", "pattern"]).unwrap();
        assert_eq!(cli.patterns.pattern.as_deref(), Some("pattern"));
    }

    #[test]
    fn cli_parses_regexp_flag() {
        let cli = Cli::try_parse_from(["sift", "-e", "pattern"]).unwrap();
        assert_eq!(cli.patterns.regexp, vec!["pattern"]);
    }

    #[test]
    fn cli_parses_multiple_regexp_flags() {
        let cli = Cli::try_parse_from(["sift", "-e", "foo", "-e", "bar"]).unwrap();
        assert_eq!(cli.patterns.regexp, vec!["foo", "bar"]);
    }

    #[test]
    fn cli_parses_build_subcommand() {
        let cli = Cli::try_parse_from(["sift", "build"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Build { .. })));
    }

    #[test]
    fn cli_parses_build_subcommand_with_path() {
        let cli = Cli::try_parse_from(["sift", "build", "/tmp"]).unwrap();
        match cli.command {
            Some(Commands::Build { path, .. }) => {
                assert_eq!(path, PathBuf::from("/tmp"));
            }
            _ => panic!("expected Build subcommand"),
        }
    }

    #[test]
    fn cli_parses_no_search_scope() {
        let cli = Cli::try_parse_from(["sift", "pattern"]).unwrap();
        assert!(cli.search_scope.paths.is_empty());
    }

    #[test]
    fn cli_parses_search_scope() {
        let cli = Cli::try_parse_from(["sift", "pattern", "src/"]).unwrap();
        assert_eq!(cli.search_scope.paths, vec![PathBuf::from("src/")]);
    }

    #[test]
    fn cli_parses_multiple_paths() {
        let cli = Cli::try_parse_from(["sift", "pattern", "src/", "tests/"]).unwrap();
        assert_eq!(
            cli.search_scope.paths,
            vec![PathBuf::from("src/"), PathBuf::from("tests/")]
        );
    }

    #[test]
    fn cli_parses_version_flag() {
        let result = Cli::try_parse_from(["sift", "--version"]);
        assert!(result.is_err());
    }

    #[test]
    fn cli_rejects_unknown_flags() {
        let result = Cli::try_parse_from(["sift", "--nonexistent-flag"]);
        assert!(result.is_err());
    }

    #[test]
    fn cli_pattern_and_search_scope() {
        let cli = Cli::try_parse_from(["sift", "pat", "dir1", "dir2"]).unwrap();
        assert_eq!(cli.patterns.pattern.as_deref(), Some("pat"));
        assert_eq!(
            cli.search_scope.paths,
            vec![PathBuf::from("dir1"), PathBuf::from("dir2")]
        );
    }

    #[test]
    fn cli_parses_short_flags_before_pattern() {
        let cli = Cli::try_parse_from(["sift", "-n", "-H", "pattern"]).unwrap();
        assert!(cli.line_number_decl.line_number);
        assert!(cli.filename_decl.with_filename);
    }

    #[test]
    fn cli_parses_long_flags_before_pattern() {
        let cli =
            Cli::try_parse_from(["sift", "--line-number", "--with-filename", "pattern"]).unwrap();
        assert!(cli.line_number_decl.line_number);
        assert!(cli.filename_decl.with_filename);
    }

    #[test]
    fn cli_parses_empty_pattern_file() {
        let cli = Cli::try_parse_from(["sift", "-f", "ignore.txt"]).unwrap();
        assert_eq!(
            cli.patterns.pattern_file.as_deref(),
            Some(Path::new("ignore.txt"))
        );
    }
}

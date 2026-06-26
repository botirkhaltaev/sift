use std::process::ExitCode;

use crate::grep::Argv;
use crate::grep::engine::{EngineDecl, MultilineDecl, ThreadingDecl, WalkerDecl};
use crate::grep::filter::{FilterConfig, SearchFilterCtx};
use crate::grep::filter::{FilterDecl, GlobFlags, TypeCatalog};
use crate::grep::ignore::MessageFlags;
use crate::grep::ignore::{
    ContextDecl, IgnoreDotDecl, IgnoreExcludeDecl, IgnoreFilesDecl, IgnoreGitDecl,
    IgnoreGlobalDecl, IgnoreMessagesDecl, IgnoreNoDecl, IgnoreParentDecl, IgnoreVcsDecl,
    MessagesDecl, UnrestrictedDecl,
};
use crate::grep::output::OutputConfig;
use crate::grep::output::{
    ColumnDecl, ColumnsDecl, ExtraOutputDecl, FilenameDecl, HeadingDecl, JsonDecl, LineNumberDecl,
    NullColorDecl, ReplaceDecl, SeparatorDecl, StatsDecl,
};
use crate::grep::paths::PathArgs;
use crate::grep::pattern::PatternConfig;
use crate::grep::pattern::{
    BinaryDecl, PatternArgs, RegexFlagsA, RegexFlagsB, SearchFlags, SearchScope,
};
use crate::grep::run::{Grep, GrepConfig, GrepMode, GrepOutcome};
use crate::index::{IndexExecution, IndexJob, IndexOperation, IndexRequest};
use crate::update;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

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

impl Cli {
    #[must_use]
    pub fn pattern_config(&self) -> PatternConfig {
        PatternConfig {
            patterns: self.patterns.clone(),
            search_flags: self.search_flags.clone(),
            regex1: self.regex1.clone(),
            regex2: self.regex2.clone(),
            multiline: self.multiline_decl.clone(),
            engine: self.engine_decl.clone(),
            binary: self.binary_decl.clone(),
            replace: self.replace_decl.clone(),
            max_count: self.paths.max_count,
        }
    }

    #[must_use]
    pub fn filter_config(&self) -> FilterConfig {
        FilterConfig {
            decl: self.filter_decl.clone(),
            glob_patterns: self.glob_flags.glob.clone(),
            follow_links: self.paths.follow,
            one_file_system: self.walker_decl.one_file_system,
        }
    }

    #[must_use]
    fn output_config(&self, search_paths: &[PathBuf]) -> OutputConfig {
        OutputConfig {
            column: self.column_decl.clone(),
            columns: self.columns_decl.clone(),
            extra: self.extra_output.clone(),
            replace_trim: self.replace_decl.trim,
            path_separator: self.threading.path_separator.clone(),
            line_number: self.line_number_decl.line_number,
            separators: self.separator_decl.clone(),
            search_paths: search_paths.to_vec(),
        }
    }

    #[must_use]
    pub fn grep_config(&self) -> GrepConfig {
        let search_paths = self.search_scope.paths.clone();
        GrepConfig {
            pattern: self.pattern_config(),
            filter: self.filter_config(),
            output: self.output_config(&search_paths),
            sift_dir: self.paths.sift_dir.clone(),
            search_paths,
            threads: self.threading.threads,
            mode: if self.filter_decl.files {
                GrepMode::ListFiles
            } else {
                GrepMode::Search
            },
            candidate_sort: sift_core::grep::CandidateSort::default(),
        }
    }

    #[must_use]
    pub fn dispatch(self, argv: &Argv<'_>) -> ExitCode {
        if self.filter_decl.type_list {
            TypeCatalog::from_decl(&self.filter_decl).print_list();
            return ExitCode::SUCCESS;
        }

        match self.command {
            Some(Commands::Update) => match update::run_binary_update() {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("sift: {e}");
                    ExitCode::from(2)
                }
            },
            Some(Commands::Index { command }) => {
                let daemon = self.paths.daemon();
                let (path, indexes, operation, execution, build_coverage) =
                    command.into_request_parts();
                let req = IndexRequest {
                    operation,
                    execution,
                    build_coverage,
                    path,
                    indexes,
                    sift_dir: self.paths.sift_dir,
                    follow_links: self.paths.follow,
                    one_file_system: self.walker_decl.one_file_system,
                    max_depth: self.filter_decl.max_depth,
                    max_filesize: self.filter_decl.max_filesize,
                };
                match IndexJob::resolve(req) {
                    Ok(index) => index.run(daemon.as_ref(), argv),
                    Err(e) => {
                        eprintln!("sift: {e}");
                        ExitCode::from(2)
                    }
                }
            }
            None => {
                let daemon = self.paths.daemon();
                let search_paths = self.search_scope.paths;
                let replace_trim = self.replace_decl.trim;
                let max_count = self.paths.max_count;
                let line_number = self.line_number_decl.line_number;
                let follow_links = self.paths.follow;
                let one_file_system = self.walker_decl.one_file_system;
                let threads = self.threading.threads;
                let path_separator = self.threading.path_separator;
                let mode = if self.filter_decl.files {
                    GrepMode::ListFiles
                } else {
                    GrepMode::Search
                };
                let candidate_sort = match self.filter_decl.sort(argv) {
                    Ok(sort) => sort,
                    Err(e) => {
                        eprintln!("sift: {e}");
                        return ExitCode::from(2);
                    }
                };

                let grep = Grep::new(GrepConfig {
                    pattern: PatternConfig {
                        patterns: self.patterns,
                        search_flags: self.search_flags,
                        regex1: self.regex1,
                        regex2: self.regex2,
                        multiline: self.multiline_decl,
                        engine: self.engine_decl,
                        binary: self.binary_decl,
                        replace: self.replace_decl,
                        max_count,
                    },
                    filter: FilterConfig {
                        decl: self.filter_decl,
                        glob_patterns: self.glob_flags.glob,
                        follow_links,
                        one_file_system,
                    },
                    output: OutputConfig {
                        column: self.column_decl,
                        columns: self.columns_decl,
                        extra: self.extra_output,
                        replace_trim,
                        path_separator,
                        line_number,
                        separators: self.separator_decl,
                        search_paths: search_paths.clone(),
                    },
                    sift_dir: self.paths.sift_dir,
                    search_paths,
                    threads,
                    mode,
                    candidate_sort,
                });

                let suppress_errors = SearchFilterCtx::resolve(argv)
                    .ignore
                    .msg_flags
                    .contains(MessageFlags::NO_MESSAGES);
                Self::exit_from_grep(grep.run(argv, daemon.as_ref()), suppress_errors)
            }
        }
    }

    fn exit_from_grep(
        result: Result<GrepOutcome, anyhow::Error>,
        suppress_error_messages: bool,
    ) -> ExitCode {
        match result {
            Ok(outcome) if outcome.succeeded() => ExitCode::SUCCESS,
            Ok(GrepOutcome::Files { .. } | GrepOutcome::Search { .. }) => ExitCode::from(1),
            Err(e) => {
                if let Some(ioe) = e.downcast_ref::<std::io::Error>()
                    && ioe.kind() == std::io::ErrorKind::BrokenPipe
                {
                    return ExitCode::SUCCESS;
                }
                if !suppress_error_messages {
                    eprintln!("sift: {e}");
                }
                ExitCode::from(2)
            }
        }
    }
}

#[derive(Subcommand)]
pub enum Commands {
    /// Download and install the latest release over the current binary.
    Update,
    Index {
        #[command(subcommand)]
        command: IndexCommands,
    },
}

#[derive(Subcommand)]
pub enum IndexCommands {
    /// Create an index at `--sift-dir` (fails if an index already exists).
    Build {
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Comma-separated index configurations to build (default: all).
        /// Available: trigram, ngram-3, ngram:3
        #[arg(short, long, value_delimiter = ',')]
        indexes: Option<Vec<sift_core::IndexConfig>>,

        /// Block until the index build completes (default: async via daemon).
        #[arg(long)]
        wait: bool,

        /// Build a lazy index that may be completed incrementally by searches.
        #[arg(long)]
        lazy: bool,
    },
    /// Incrementally refresh an existing index (fails if no index exists).
    Update {
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Comma-separated index configurations to update (default: all).
        /// Available: trigram, ngram-3, ngram:3
        #[arg(short, long, value_delimiter = ',')]
        indexes: Option<Vec<sift_core::IndexConfig>>,

        /// Block until the index update completes.
        #[arg(long)]
        wait: bool,
    },
}

impl IndexCommands {
    fn into_request_parts(
        self,
    ) -> (
        PathBuf,
        Option<Vec<sift_core::IndexConfig>>,
        IndexOperation,
        IndexExecution,
        sift_core::IndexCoverage,
    ) {
        match self {
            Self::Build {
                path,
                indexes,
                wait,
                lazy,
            } => {
                let execution = if wait {
                    IndexExecution::Blocking
                } else {
                    IndexExecution::Background
                };
                let coverage = if lazy {
                    sift_core::IndexCoverage::Lazy
                } else {
                    sift_core::IndexCoverage::Complete
                };
                (path, indexes, IndexOperation::Build, execution, coverage)
            }
            Self::Update {
                path,
                indexes,
                wait,
            } => {
                let execution = if wait {
                    IndexExecution::Blocking
                } else {
                    IndexExecution::Background
                };
                (
                    path,
                    indexes,
                    IndexOperation::Update,
                    execution,
                    sift_core::IndexCoverage::Complete,
                )
            }
        }
    }
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
    fn cli_parses_update_subcommand() {
        let cli = Cli::try_parse_from(["sift", "update"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Update)));
    }

    #[test]
    fn cli_parses_index_build_subcommand() {
        let cli = Cli::try_parse_from(["sift", "index", "build"]).unwrap();
        match cli.command {
            Some(Commands::Index {
                command: IndexCommands::Build { path, .. },
            }) => assert_eq!(path, PathBuf::from(".")),
            _ => panic!("expected index build subcommand"),
        }
    }

    #[test]
    fn cli_parses_index_build_subcommand_with_path() {
        let cli = Cli::try_parse_from(["sift", "index", "build", "/tmp"]).unwrap();
        match cli.command {
            Some(Commands::Index {
                command: IndexCommands::Build { path, .. },
            }) => assert_eq!(path, PathBuf::from("/tmp")),
            _ => panic!("expected index build subcommand"),
        }
    }

    #[test]
    fn cli_parses_index_update_subcommand() {
        let cli = Cli::try_parse_from(["sift", "index", "update"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Index {
                command: IndexCommands::Update { .. }
            })
        ));
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

    #[test]
    fn cli_parses_index_build_defaults_to_background() {
        let cli = Cli::try_parse_from(["sift", "index", "build"]).unwrap();
        match cli.command {
            Some(Commands::Index {
                command: IndexCommands::Build { wait, .. },
            }) => assert!(!wait),
            _ => panic!("expected index build subcommand"),
        }
    }

    #[test]
    fn cli_parses_index_build_wait_flag() {
        let cli = Cli::try_parse_from(["sift", "index", "build", "--wait"]).unwrap();
        match cli.command {
            Some(Commands::Index {
                command: IndexCommands::Build { wait, .. },
            }) => assert!(wait),
            _ => panic!("expected index build subcommand"),
        }
    }

    #[test]
    fn cli_parses_index_build_lazy_flag() {
        let cli = Cli::try_parse_from(["sift", "index", "build", "--lazy"]).unwrap();
        match cli.command {
            Some(Commands::Index {
                command: IndexCommands::Build { lazy, .. },
            }) => assert!(lazy),
            _ => panic!("expected index build subcommand"),
        }
    }

    #[test]
    fn cli_rejects_index_update_lazy_flag() {
        let result = Cli::try_parse_from(["sift", "index", "update", "--lazy"]);
        assert!(result.is_err());
    }
}

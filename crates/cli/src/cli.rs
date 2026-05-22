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
    },
}

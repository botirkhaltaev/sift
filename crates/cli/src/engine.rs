use clap::Args;

/// Regex engine and configuration flags.
#[derive(Args)]
pub struct EngineDecl {
    #[arg(long = "no-config")]
    pub no_config: bool,
    #[arg(long = "unicode")]
    pub unicode: bool,
    #[arg(long = "no-unicode")]
    pub no_unicode: bool,
    #[arg(long = "colors", value_name = "COLOR_SPEC")]
    pub colors: Vec<String>,
    #[arg(long = "regex-size-limit", value_name = "NUM+SUFFIX?")]
    pub regex_size_limit: Option<String>,
    #[arg(long = "dfa-size-limit", value_name = "NUM+SUFFIX?")]
    pub dfa_size_limit: Option<String>,
}

/// Threading and output-buffering flags.
#[derive(Args)]
pub struct ThreadingDecl {
    #[arg(short = 'j', long = "threads", value_name = "NUM")]
    pub threads: Option<usize>,
    #[arg(long = "line-buffered")]
    pub line_buffered: bool,
    #[arg(long = "block-buffered")]
    pub block_buffered: bool,
    #[arg(long = "path-separator", value_name = "SEPARATOR")]
    pub path_separator: Option<String>,
}

/// Filesystem-level flags for the walker.
#[derive(Args)]
pub struct WalkerDecl {
    #[arg(long = "one-file-system")]
    pub one_file_system: bool,
    #[arg(long = "mmap")]
    pub mmap: bool,
    #[arg(long = "no-mmap")]
    pub no_mmap: bool,
}

/// Multiline and CRLF flags.
#[derive(Args)]
pub struct MultilineDecl {
    #[arg(short = 'U', long = "multiline")]
    pub multiline: bool,
    #[arg(long = "multiline-dotall")]
    pub multiline_dotall: bool,
    #[arg(long = "crlf")]
    pub crlf: bool,
}

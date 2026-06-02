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

#[cfg(test)]
mod tests {
    use crate::cli::Cli;
    use clap::Parser;

    #[test]
    fn engine_no_config_flag() {
        let cli = Cli::try_parse_from(["sift", "--no-config", "pat"]).unwrap();
        assert!(cli.engine_decl.no_config);
    }

    #[test]
    fn engine_unicode_flag() {
        let cli = Cli::try_parse_from(["sift", "--unicode", "pat"]).unwrap();
        assert!(cli.engine_decl.unicode);
    }

    #[test]
    fn engine_no_unicode_flag() {
        let cli = Cli::try_parse_from(["sift", "--no-unicode", "pat"]).unwrap();
        assert!(cli.engine_decl.no_unicode);
    }

    #[test]
    fn engine_colors_flag() {
        let cli = Cli::try_parse_from(["sift", "--colors", "path:fg:red", "pat"]).unwrap();
        assert!(!cli.engine_decl.colors.is_empty());
    }

    #[test]
    fn engine_regex_size_limit() {
        let cli = Cli::try_parse_from(["sift", "--regex-size-limit", "10M", "pat"]).unwrap();
        assert_eq!(cli.engine_decl.regex_size_limit.as_deref(), Some("10M"));
    }

    #[test]
    fn engine_dfa_size_limit() {
        let cli = Cli::try_parse_from(["sift", "--dfa-size-limit", "50M", "pat"]).unwrap();
        assert_eq!(cli.engine_decl.dfa_size_limit.as_deref(), Some("50M"));
    }

    #[test]
    fn threading_threads_flag() {
        let cli = Cli::try_parse_from(["sift", "-j", "4", "pat"]).unwrap();
        assert_eq!(cli.threading.threads, Some(4));
    }

    #[test]
    fn threading_line_buffered() {
        let cli = Cli::try_parse_from(["sift", "--line-buffered", "pat"]).unwrap();
        assert!(cli.threading.line_buffered);
    }

    #[test]
    fn threading_block_buffered() {
        let cli = Cli::try_parse_from(["sift", "--block-buffered", "pat"]).unwrap();
        assert!(cli.threading.block_buffered);
    }

    #[test]
    fn threading_path_separator() {
        let cli = Cli::try_parse_from(["sift", "--path-separator", "/", "pat"]).unwrap();
        assert_eq!(cli.threading.path_separator.as_deref(), Some("/"));
    }

    #[test]
    fn walker_one_file_system() {
        let cli = Cli::try_parse_from(["sift", "--one-file-system", "pat"]).unwrap();
        assert!(cli.walker_decl.one_file_system);
    }

    #[test]
    fn walker_mmap() {
        let cli = Cli::try_parse_from(["sift", "--mmap", "pat"]).unwrap();
        assert!(cli.walker_decl.mmap);
    }

    #[test]
    fn walker_no_mmap() {
        let cli = Cli::try_parse_from(["sift", "--no-mmap", "pat"]).unwrap();
        assert!(cli.walker_decl.no_mmap);
    }

    #[test]
    fn multiline_flag() {
        let cli = Cli::try_parse_from(["sift", "-U", "pat"]).unwrap();
        assert!(cli.multiline_decl.multiline);
    }

    #[test]
    fn multiline_long_flag() {
        let cli = Cli::try_parse_from(["sift", "--multiline", "pat"]).unwrap();
        assert!(cli.multiline_decl.multiline);
    }

    #[test]
    fn multiline_dotall_flag() {
        let cli = Cli::try_parse_from(["sift", "--multiline-dotall", "pat"]).unwrap();
        assert!(cli.multiline_decl.multiline_dotall);
    }

    #[test]
    fn crlf_flag() {
        let cli = Cli::try_parse_from(["sift", "--crlf", "pat"]).unwrap();
        assert!(cli.multiline_decl.crlf);
    }

    #[test]
    fn combined_engine_flags() {
        let cli = Cli::try_parse_from([
            "sift",
            "--no-config",
            "--unicode",
            "-j",
            "8",
            "--one-file-system",
            "-U",
            "--crlf",
            "pat",
        ])
        .unwrap();
        assert!(cli.engine_decl.no_config);
        assert!(cli.engine_decl.unicode);
        assert_eq!(cli.threading.threads, Some(8));
        assert!(cli.walker_decl.one_file_system);
        assert!(cli.multiline_decl.multiline);
        assert!(cli.multiline_decl.crlf);
    }
}

use std::path::PathBuf;

use clap::{Arg, ArgAction, ArgMatches, Args, Command, FromArgMatches};
use sift_core::{
    CandidateFilterConfig, GlobConfig, HiddenMode, IgnoreConfig, TypeDef, VisibilityConfig,
};

use super::ignore::MessageFlags;
use crate::cli::Cli;

#[derive(Args)]
pub struct FilterDecl {
    #[arg(long = "max-depth", value_name = "NUM")]
    pub max_depth: Option<usize>,
    #[arg(long = "max-filesize", value_name = "NUM+SUFFIX?")]
    pub max_filesize: Option<String>,
    #[arg(long = "iglob", action = ArgAction::Append, value_name = "GLOB")]
    pub iglob: Vec<String>,
    #[arg(long = "ignore-file", action = ArgAction::Append, value_name = "PATH")]
    pub ignore_file: Vec<PathBuf>,
    #[arg(long = "files")]
    pub files: bool,
    #[arg(short = 't', long = "type", action = ArgAction::Append, value_name = "TYPE")]
    pub type_include: Vec<String>,
    #[arg(short = 'T', long = "type-not", action = ArgAction::Append, value_name = "TYPE")]
    pub type_exclude: Vec<String>,
    #[arg(long = "type-list")]
    pub type_list: bool,
    #[arg(long = "type-add", action = ArgAction::Append, value_name = "TYPE_SPEC")]
    pub type_add: Vec<String>,
    #[arg(long = "type-clear", action = ArgAction::Append, value_name = "TYPE")]
    pub type_clear: Vec<String>,
    #[arg(long = "sort", value_name = "SORTBY")]
    pub sort: Option<String>,
    #[arg(long = "sortr", value_name = "SORTBY")]
    pub sortr: Option<String>,
}

/// Resolved visibility, ignore sources, and glob case for [`CandidateFilterConfig`].
#[derive(Clone, Copy)]
pub struct SearchFilterCtx {
    pub hidden: bool,
    pub ignore_sources: sift_core::IgnoreSources,
    pub require_git: bool,
    pub glob_case_insensitive: bool,
    pub msg_flags: MessageFlags,
}

impl SearchFilterCtx {
    #[must_use]
    pub const fn hidden_mode(self) -> HiddenMode {
        if self.hidden {
            HiddenMode::Include
        } else {
            HiddenMode::Respect
        }
    }
}

#[derive(Clone)]
pub struct GlobFlags {
    pub glob: Vec<String>,
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
    fn from_arg_matches(matches: &ArgMatches) -> Result<Self, clap::Error> {
        let glob = matches
            .get_many::<String>("glob")
            .map(|v| v.cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        Ok(Self { glob })
    }

    fn update_from_arg_matches(&mut self, matches: &ArgMatches) -> Result<(), clap::Error> {
        *self = Self::from_arg_matches(matches)?;
        Ok(())
    }
}

impl Default for GlobFlags {
    fn default() -> Self {
        Self::new()
    }
}

/// Parses a size suffix like "10K" or "2MB" into bytes.
///
/// # Errors
///
/// Returns an error if the input is not a valid size string.
pub fn parse_size_suffix(s: &str) -> anyhow::Result<u64> {
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

#[must_use]
pub fn builtin_type_defs() -> Vec<TypeDef> {
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

#[must_use]
pub fn resolve_type_defs(decl: &FilterDecl) -> Vec<TypeDef> {
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

// ── Config builders ──

impl Cli {
    /// # Errors
    ///
    /// Returns an error if `max_filesize` parsing fails.
    pub fn build_filter_config(
        &self,
        filter: SearchFilterCtx,
        scopes: Vec<PathBuf>,
        exclude_paths: Vec<PathBuf>,
    ) -> anyhow::Result<CandidateFilterConfig> {
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

        let needs_type_defs = !self.filter_decl.type_include.is_empty()
            || !self.filter_decl.type_exclude.is_empty()
            || !self.filter_decl.type_add.is_empty()
            || !self.filter_decl.type_clear.is_empty();
        let type_definitions = if needs_type_defs {
            resolve_type_defs(&self.filter_decl)
        } else {
            Vec::new()
        };

        Ok(CandidateFilterConfig {
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
}

/// Free-function version used by `run_files_mode` (no self).
///
/// # Errors
///
/// Returns an error if `max_filesize` parsing fails.
pub fn build_search_filter_config(
    cli: &Cli,
    filter: SearchFilterCtx,
    scopes: Vec<PathBuf>,
    exclude_paths: Vec<PathBuf>,
) -> anyhow::Result<CandidateFilterConfig> {
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

    let needs_type_defs = !cli.filter_decl.type_include.is_empty()
        || !cli.filter_decl.type_exclude.is_empty()
        || !cli.filter_decl.type_add.is_empty()
        || !cli.filter_decl.type_clear.is_empty();
    let type_definitions = if needs_type_defs {
        resolve_type_defs(&cli.filter_decl)
    } else {
        Vec::new()
    };

    let custom_files = if filter.msg_flags.contains(MessageFlags::NO_IGNORE_FILES) {
        Vec::new()
    } else {
        cli.filter_decl.ignore_file.clone()
    };

    Ok(CandidateFilterConfig {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_suffix_plain_number() {
        assert_eq!(parse_size_suffix("42").unwrap(), 42);
    }

    #[test]
    fn size_suffix_bytes() {
        assert_eq!(parse_size_suffix("100B").unwrap(), 100);
    }

    #[test]
    fn size_suffix_kilobytes() {
        assert_eq!(parse_size_suffix("2K").unwrap(), 2048);
        assert_eq!(parse_size_suffix("2KB").unwrap(), 2048);
    }

    #[test]
    fn size_suffix_megabytes() {
        assert_eq!(parse_size_suffix("3M").unwrap(), 3 * 1024 * 1024);
        assert_eq!(parse_size_suffix("3MB").unwrap(), 3 * 1024 * 1024);
    }

    #[test]
    fn size_suffix_gigabytes() {
        assert_eq!(parse_size_suffix("1G").unwrap(), 1024 * 1024 * 1024);
    }

    #[test]
    fn size_suffix_unknown_unit() {
        assert!(parse_size_suffix("5X").is_err());
    }

    #[test]
    fn size_suffix_invalid_number() {
        assert!(parse_size_suffix("abc").is_err());
    }

    #[test]
    fn builtin_type_defs_contains_rust() {
        let defs = builtin_type_defs();
        assert!(defs.iter().any(|d| d.name == "rust"));
    }

    #[test]
    fn builtin_type_defs_contains_python() {
        let defs = builtin_type_defs();
        assert!(defs.iter().any(|d| d.name == "py"));
    }

    #[test]
    fn builtin_type_defs_non_empty() {
        let defs = builtin_type_defs();
        assert!(!defs.is_empty());
    }

    #[test]
    fn resolve_type_defs_clear_removes_type() {
        let decl = FilterDecl {
            max_depth: None,
            max_filesize: None,
            iglob: vec![],
            ignore_file: vec![],
            files: false,
            type_include: vec![],
            type_exclude: vec![],
            type_list: false,
            type_add: vec![],
            type_clear: vec!["rust".into(), "py".into()],
            sort: None,
            sortr: None,
        };
        let defs = resolve_type_defs(&decl);
        assert!(!defs.iter().any(|d| d.name == "rust"));
        assert!(!defs.iter().any(|d| d.name == "py"));
    }

    #[test]
    fn resolve_type_defs_add_custom_type() {
        let decl = FilterDecl {
            max_depth: None,
            max_filesize: None,
            iglob: vec![],
            ignore_file: vec![],
            files: false,
            type_include: vec![],
            type_exclude: vec![],
            type_list: false,
            type_add: vec!["mytype:*.my".into()],
            type_clear: vec![],
            sort: None,
            sortr: None,
        };
        let defs = resolve_type_defs(&decl);
        assert!(defs.iter().any(|d| d.name == "mytype"));
    }

    #[test]
    fn resolve_type_defs_add_extends_existing() {
        let decl = FilterDecl {
            max_depth: None,
            max_filesize: None,
            iglob: vec![],
            ignore_file: vec![],
            files: false,
            type_include: vec![],
            type_exclude: vec![],
            type_list: false,
            type_add: vec!["rust:*.rsx".into()],
            type_clear: vec![],
            sort: None,
            sortr: None,
        };
        let defs = resolve_type_defs(&decl);
        let rust = defs.iter().find(|d| d.name == "rust").unwrap();
        assert!(rust.globs.contains(&"*.rsx".to_string()));
        assert!(rust.globs.contains(&"*.rs".to_string()));
    }

    #[test]
    fn resolve_type_defs_add_include() {
        let decl = FilterDecl {
            max_depth: None,
            max_filesize: None,
            iglob: vec![],
            ignore_file: vec![],
            files: false,
            type_include: vec![],
            type_exclude: vec![],
            type_list: false,
            type_add: vec!["combined:include:rust,py".into()],
            type_clear: vec![],
            sort: None,
            sortr: None,
        };
        let defs = resolve_type_defs(&decl);
        let combined = defs.iter().find(|d| d.name == "combined").unwrap();
        assert!(combined.globs.contains(&"*.rs".to_string()));
        assert!(combined.globs.contains(&"*.py".to_string()));
    }

    #[test]
    fn search_filter_ctx_hidden_mode_include() {
        let ctx = SearchFilterCtx {
            hidden: true,
            ignore_sources: sift_core::IgnoreSources::empty(),
            require_git: false,
            glob_case_insensitive: false,
            msg_flags: MessageFlags::empty(),
        };
        assert!(matches!(ctx.hidden_mode(), sift_core::HiddenMode::Include));
    }

    #[test]
    fn search_filter_ctx_hidden_mode_respect() {
        let ctx = SearchFilterCtx {
            hidden: false,
            ignore_sources: sift_core::IgnoreSources::empty(),
            require_git: false,
            glob_case_insensitive: false,
            msg_flags: MessageFlags::empty(),
        };
        assert!(matches!(ctx.hidden_mode(), sift_core::HiddenMode::Respect));
    }

    #[test]
    fn glob_flags_default_empty() {
        let g = GlobFlags { glob: vec![] };
        assert!(g.glob.is_empty());
    }
}

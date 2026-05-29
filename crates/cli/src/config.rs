//! Normalized CLI configuration.
//!
//! Builds domain configs from clap [`Cli`] and process environment once, at the
//! top level, so downstream modules never read environment variables directly.

use std::path::PathBuf;

use sift_core::{CorpusKind, IndexKind};

use crate::cli::{Cli, Commands};
use crate::paths::excluded_search_paths;

/// Fully resolved runtime configuration for the CLI.
pub struct CliConfig {
    pub build: Option<BuildConfig>,
    pub search: SearchConfig,
    pub daemon: DaemonConfig,
    pub files_mode: bool,
}

/// Resolved build sub-command configuration.
pub struct BuildConfig {
    pub root: PathBuf,
    pub include_paths: Vec<PathBuf>,
    pub corpus_kind: CorpusKind,
    pub indexes: Vec<IndexKind>,
    pub follow_links: bool,
    pub exclude_paths: Vec<PathBuf>,
    pub sift_dir: PathBuf,
}

/// Resolved search configuration.
pub struct SearchConfig {
    pub sift_dir: PathBuf,
    pub args: Vec<String>,
}

/// Daemon configuration containing both spawn policy and run config.
pub struct DaemonConfig {
    pub spawn: DaemonSpawnConfig,
}

/// Spawn policy for the daemon background process.
pub struct DaemonSpawnConfig {
    pub enabled: bool,
    pub sift_dir: PathBuf,
    pub init_root: Option<PathBuf>,
}

/// Runner configuration for the long-running daemon process.
pub struct DaemonRunConfig {
    pub sift_dir: PathBuf,
    pub init_root: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// Construction helpers
// ---------------------------------------------------------------------------

impl CliConfig {
    /// Build a `CliConfig` from parsed CLI args and process environment.
    ///
    /// # Errors
    ///
    /// Returns an error if the build path cannot be canonicalised.
    pub fn from_cli(cli: &Cli) -> Result<Self, std::io::Error> {
        let build = match &cli.command {
            Some(Commands::Build { path, indexes }) => {
                let canonical = path.canonicalize()?;
                let (root, include_paths, corpus_kind) = if canonical.is_file() {
                    let parent = canonical.parent().unwrap_or(&canonical).to_path_buf();
                    let filename = PathBuf::from(canonical.file_name().unwrap_or_default());
                    (parent, vec![filename], CorpusKind::SingleFile)
                } else {
                    (canonical, Vec::new(), CorpusKind::Directory)
                };
                let indexes: Vec<IndexKind> = indexes.as_deref().unwrap_or(IndexKind::ALL).to_vec();
                let sift_dir = cli.paths.sift_dir.clone();
                let exclude_paths = excluded_search_paths(&root, &sift_dir);
                Some(BuildConfig {
                    root,
                    include_paths,
                    corpus_kind,
                    indexes,
                    follow_links: cli.paths.follow,
                    exclude_paths,
                    sift_dir,
                })
            }
            None => None,
        };

        let search = SearchConfig {
            sift_dir: cli.paths.sift_dir.clone(),
            args: std::env::args().collect(),
        };

        let daemon_spawn = DaemonSpawnConfig {
            enabled: std::env::var_os("SIFT_NO_DAEMON").is_none(),
            sift_dir: cli.paths.sift_dir.clone(),
            init_root: None,
        };

        let files_mode = cli.filter_decl.files;

        Ok(Self {
            build,
            search,
            daemon: DaemonConfig {
                spawn: daemon_spawn,
            },
            files_mode,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_config_respects_enabled_flag() {
        let config = DaemonSpawnConfig {
            enabled: false,
            sift_dir: PathBuf::from("/tmp"),
            init_root: None,
        };
        assert!(!config.enabled);
    }
}

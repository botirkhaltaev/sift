use std::path::{Path, PathBuf};

use ignore::gitignore::{Gitignore, GitignoreBuilder};

use super::config::IgnoreConfig;
use super::error::FilterError;

impl IgnoreConfig {
    /// Build a gitignore matcher for `root` from this configuration.
    ///
    /// # Errors
    ///
    /// Returns [`FilterError::Ignore`] if ignore rules cannot be compiled.
    pub fn matcher(&self, root: &Path) -> Result<Option<Gitignore>, FilterError> {
        if self.sources.is_empty() && self.custom_files.is_empty() {
            return Ok(None);
        }

        let mut builder = GitignoreBuilder::new(root);

        if self.sources.contains(super::config::IgnoreSources::GLOBAL)
            && let Some(global_path) = global_gitignore_path()
        {
            let _ = builder.add(&global_path);
        }

        if self.sources.contains(super::config::IgnoreSources::PARENT) {
            let mut dir = root.parent();
            while let Some(parent) = dir {
                if self.sources.contains(super::config::IgnoreSources::VCS) {
                    let gitignore = parent.join(".gitignore");
                    if gitignore.is_file() {
                        let _ = builder.add(&gitignore);
                    }
                }
                if self.sources.contains(super::config::IgnoreSources::DOT) {
                    let ignore_file = parent.join(".ignore");
                    if ignore_file.is_file() {
                        let _ = builder.add(&ignore_file);
                    }
                }
                dir = parent.parent();
            }
        }

        if self.sources.contains(super::config::IgnoreSources::DOT) {
            let _ = builder.add(root.join(".ignore"));
            let _ = builder.add(root.join(".rgignore"));
        }

        if self.sources.contains(super::config::IgnoreSources::VCS) {
            let gitignore_path = root.join(".gitignore");
            if gitignore_path.is_file() && (!self.require_git || root.join(".git").is_dir()) {
                let _ = builder.add(&gitignore_path);
            }
        }

        if self.sources.contains(super::config::IgnoreSources::EXCLUDE) {
            let exclude_path = root.join(".git/info/exclude");
            if exclude_path.is_file() {
                let _ = builder.add(&exclude_path);
            }
        }

        for custom in &self.custom_files {
            let path = root.join(custom);
            let _ = builder.add(&path);
        }

        let matcher = builder.build().map_err(FilterError::Ignore)?;
        if matcher.is_empty() {
            return Ok(None);
        }
        Ok(Some(matcher))
    }
}

fn global_gitignore_path() -> Option<PathBuf> {
    if let Ok(out) = std::process::Command::new("git")
        .args(["config", "--global", "core.excludesFile"])
        .output()
    {
        let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !path.is_empty() {
            let expanded = if path.starts_with('~') {
                std::env::var("HOME").map_or_else(
                    |_| PathBuf::from(&path),
                    |home| PathBuf::from(path.replacen('~', &home, 1)),
                )
            } else {
                PathBuf::from(&path)
            };
            if expanded.is_file() {
                return Some(expanded);
            }
        }
    }
    let config_dir = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| PathBuf::from(h).join(".config"))
        });
    if let Some(dir) = config_dir {
        let path = dir.join("git/ignore");
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

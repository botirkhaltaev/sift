//! Search-time filtering over indexed file paths.
//!
//! All user-visible inclusion/exclusion rules are applied at search time,
//! not at index build time. The index stores a structural list of files;
//! this module applies visibility, scope, and glob rules to produce the
//! candidate set for a given search.

use std::path::{Path, PathBuf};

use ignore::gitignore::Gitignore;
use ignore::overrides::{Override, OverrideBuilder};

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct IgnoreSources: u8 {
        const DOT     = 1 << 0;
        const VCS     = 1 << 1;
        const GLOBAL  = 1 << 2;
        const EXCLUDE = 1 << 3;
        const PARENT  = 1 << 4;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HiddenMode {
    #[default]
    Respect,
    Include,
}

#[derive(Debug, Clone, Default)]
pub struct IgnoreConfig {
    pub sources: IgnoreSources,
    pub custom_files: Vec<PathBuf>,
    pub require_git: bool,
}

#[derive(Debug, Clone, Default)]
pub struct VisibilityConfig {
    pub hidden: HiddenMode,
    pub ignore: IgnoreConfig,
}

#[derive(Debug, Clone, Default)]
pub struct GlobConfig {
    pub patterns: Vec<String>,
    pub case_insensitive: bool,
}

/// A named file-type definition mapping a type name to a set of glob patterns.
#[derive(Debug, Clone)]
pub struct TypeDef {
    pub name: String,
    pub globs: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SearchFilterConfig {
    pub scopes: Vec<PathBuf>,
    pub exclude_paths: Vec<PathBuf>,
    pub glob: GlobConfig,
    pub visibility: VisibilityConfig,
    /// Follow symbolic links when walking the tree (search and index build).
    pub follow_links: bool,
    /// Maximum directory depth for walk-based search (`--max-depth`).
    /// `None` means no limit.
    pub max_depth: Option<usize>,
    /// Maximum file size in bytes (`--max-filesize`).
    /// Files larger than this are skipped. `None` means no limit.
    pub max_filesize: Option<u64>,
    /// Type definitions for `--type`/`--type-not` filtering.
    /// Each entry maps a type name to its glob patterns.
    pub type_definitions: Vec<TypeDef>,
    /// Type names to include (`-t`/`--type`).
    pub type_include: Vec<String>,
    /// Type names to exclude (`-T`/`--type-not`).
    pub type_exclude: Vec<String>,
    /// `--one-file-system`: do not cross filesystem boundaries.
    pub one_file_system: bool,
}

/// Pre-computed candidate for efficient batch filtering and search.
#[derive(Debug, Clone)]
pub struct CandidateInfo {
    /// Relative path as stored in the index.
    pub rel_path: PathBuf,
    /// Normalized relative path string (forward slashes, for gitignore/glob). Empty when neither applies.
    pub rel_str: String,
    /// Absolute path on disk.
    pub abs_path: PathBuf,
}

#[derive(Debug)]
pub struct SearchFilter {
    scopes: Vec<PathBuf>,
    exclude_paths: Vec<PathBuf>,
    hidden: HiddenMode,
    gitignore: Option<Gitignore>,
    glob: Option<Override>,
    glob_case_insensitive: bool,
    follow_links: bool,
    max_depth: Option<usize>,
    max_filesize: Option<u64>,
    type_glob: Option<Override>,
    one_file_system: bool,
}

impl SearchFilter {
    fn path_is_hidden(rel_path: &Path) -> bool {
        rel_path.components().any(|component| {
            let bytes = component.as_os_str().as_encoded_bytes();
            bytes.starts_with(b".") && bytes.len() > 1
        })
    }

    /// Path scopes (relative to the search root) limiting which files are considered.
    #[must_use]
    pub fn scopes(&self) -> &[PathBuf] {
        &self.scopes
    }

    #[must_use]
    pub(crate) const fn needs_rel_str_for_matching(&self) -> bool {
        self.gitignore.is_some() || self.glob.is_some() || self.type_glob.is_some()
    }

    /// Whether symlinked files and directories should be followed when walking.
    #[must_use]
    pub const fn follow_links(&self) -> bool {
        self.follow_links
    }

    /// Maximum directory depth for walk-based search.
    #[must_use]
    pub const fn max_depth(&self) -> Option<usize> {
        self.max_depth
    }

    /// Maximum file size in bytes; files above this are skipped.
    #[must_use]
    pub const fn max_filesize(&self) -> Option<u64> {
        self.max_filesize
    }

    /// Whether to stay on the same filesystem when walking.
    #[must_use]
    pub const fn one_file_system(&self) -> bool {
        self.one_file_system
    }

    /// Build a search filter from configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if glob patterns are invalid.
    pub fn new(config: &SearchFilterConfig, index_root: &Path) -> crate::Result<Self> {
        let scopes = if config.scopes.is_empty() {
            vec![PathBuf::from("")]
        } else {
            config.scopes.clone()
        };

        let gitignore = Self::build_gitignore_matcher(index_root, &config.visibility.ignore)?;

        let glob_case_insensitive = config.glob.case_insensitive;
        let glob = if config.glob.patterns.is_empty() {
            None
        } else {
            let mut builder = OverrideBuilder::new(index_root);
            if config.glob.case_insensitive {
                let _ = builder.case_insensitive(true);
            }
            for g in &config.glob.patterns {
                builder.add(g).map_err(|e| {
                    crate::Error::RegexBuild(format!("invalid glob pattern '{g}': {e}"))
                })?;
            }
            Some(
                builder
                    .build()
                    .map_err(|e| crate::Error::RegexBuild(e.to_string()))?,
            )
        };

        let type_glob = Self::build_type_glob(
            index_root,
            &config.type_definitions,
            &config.type_include,
            &config.type_exclude,
        )?;

        Ok(Self {
            scopes,
            exclude_paths: config.exclude_paths.clone(),
            hidden: config.visibility.hidden,
            gitignore,
            glob,
            glob_case_insensitive,
            follow_links: config.follow_links,
            max_depth: config.max_depth,
            max_filesize: config.max_filesize,
            type_glob,
            one_file_system: config.one_file_system,
        })
    }

    /// Resolve the global git ignore file path (like ripgrep's `core.excludesFile`).
    fn global_gitignore_path() -> Option<PathBuf> {
        // Try `core.excludesFile` via `git config`; fall back to default location.
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
        // Default: $XDG_CONFIG_HOME/git/ignore or $HOME/.config/git/ignore
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

    fn build_gitignore_matcher(
        root: &Path,
        ignore_config: &IgnoreConfig,
    ) -> crate::Result<Option<Gitignore>> {
        if ignore_config.sources.is_empty() && ignore_config.custom_files.is_empty() {
            return Ok(None);
        }

        let mut builder = ignore::gitignore::GitignoreBuilder::new(root);

        // Global git ignore (core.excludesFile / ~/.config/git/ignore).
        if ignore_config.sources.contains(IgnoreSources::GLOBAL)
            && let Some(global_path) = Self::global_gitignore_path()
        {
            let _ = builder.add(&global_path);
        }

        // Parent directory ignore files (walk up from root).
        if ignore_config.sources.contains(IgnoreSources::PARENT) {
            let mut dir = root.parent();
            while let Some(parent) = dir {
                if ignore_config.sources.contains(IgnoreSources::VCS) {
                    let gitignore = parent.join(".gitignore");
                    if gitignore.is_file() {
                        let _ = builder.add(&gitignore);
                    }
                }
                if ignore_config.sources.contains(IgnoreSources::DOT) {
                    let ignore = parent.join(".ignore");
                    if ignore.is_file() {
                        let _ = builder.add(&ignore);
                    }
                }
                dir = parent.parent();
            }
        }

        if ignore_config.sources.contains(IgnoreSources::DOT) {
            let _ = builder.add(root.join(".ignore"));
            let _ = builder.add(root.join(".rgignore"));
        }

        if ignore_config.sources.contains(IgnoreSources::VCS) {
            let gitignore_path = root.join(".gitignore");
            if gitignore_path.is_file()
                && (!ignore_config.require_git || root.join(".git").is_dir())
            {
                let _ = builder.add(&gitignore_path);
            }
        }

        if ignore_config.sources.contains(IgnoreSources::EXCLUDE) {
            let exclude_path = root.join(".git/info/exclude");
            if exclude_path.is_file() {
                let _ = builder.add(&exclude_path);
            }
        }

        for custom in &ignore_config.custom_files {
            let path = root.join(custom);
            let _ = builder.add(&path);
        }

        let matcher = builder.build().map_err(crate::Error::Ignore)?;
        Ok(Some(matcher))
    }

    fn build_type_glob(
        root: &Path,
        defs: &[TypeDef],
        include: &[String],
        exclude: &[String],
    ) -> crate::Result<Option<Override>> {
        if include.is_empty() && exclude.is_empty() {
            return Ok(None);
        }
        let mut builder = OverrideBuilder::new(root);
        for name in include {
            let patterns = Self::globs_for_type(defs, name)?;
            for g in patterns {
                builder.add(&g).map_err(|e| {
                    crate::Error::RegexBuild(format!("type glob for '{name}': {e}"))
                })?;
            }
        }
        for name in exclude {
            let patterns = Self::globs_for_type(defs, name)?;
            for g in patterns {
                let negated = format!("!{g}");
                builder.add(&negated).map_err(|e| {
                    crate::Error::RegexBuild(format!("type glob for '{name}': {e}"))
                })?;
            }
        }
        Ok(Some(
            builder
                .build()
                .map_err(|e| crate::Error::RegexBuild(e.to_string()))?,
        ))
    }

    fn globs_for_type(defs: &[TypeDef], name: &str) -> crate::Result<Vec<String>> {
        for def in defs {
            if def.name == name {
                return Ok(def.globs.clone());
            }
        }
        Err(crate::Error::RegexBuild(format!(
            "unknown file type: '{name}'"
        )))
    }

    /// Check if a relative path passes all filters.
    #[must_use]
    pub fn is_candidate(&self, rel_path: &Path) -> bool {
        if !self.in_scope(rel_path) {
            return false;
        }
        if self.is_excluded(rel_path) {
            return false;
        }
        self.matches_file(rel_path)
    }

    /// Check if a pre-computed `CandidateInfo` passes all filters.
    /// More efficient than `is_candidate` when the candidate has already been prepared.
    #[must_use]
    pub fn is_candidate_info(&self, info: &CandidateInfo) -> bool {
        if !self.in_scope_info(info) {
            return false;
        }
        if self.is_excluded(&info.rel_path) {
            return false;
        }
        self.matches_file_info(info)
    }

    fn is_excluded(&self, rel_path: &Path) -> bool {
        self.exclude_paths
            .iter()
            .any(|excluded| !excluded.as_os_str().is_empty() && rel_path.starts_with(excluded))
    }

    fn in_scope(&self, rel_path: &Path) -> bool {
        for scope in &self.scopes {
            if scope.as_os_str().is_empty() {
                return true;
            }
            if rel_path.starts_with(scope) {
                return true;
            }
        }
        false
    }

    fn in_scope_info(&self, info: &CandidateInfo) -> bool {
        for scope in &self.scopes {
            if scope.as_os_str().is_empty() {
                return true;
            }
            if info.rel_path.starts_with(scope) {
                return true;
            }
        }
        false
    }

    fn matches_file(&self, rel_path: &Path) -> bool {
        if self.hidden == HiddenMode::Respect && Self::path_is_hidden(rel_path) {
            return false;
        }

        if self.needs_rel_str_for_matching() {
            let rel_str = rel_path.to_string_lossy().replace('\\', "/");
            return self.matches_file_str(Path::new(&rel_str));
        }

        true
    }

    fn matches_file_info(&self, info: &CandidateInfo) -> bool {
        if self.hidden == HiddenMode::Respect && Self::path_is_hidden(&info.rel_path) {
            return false;
        }

        if !self.needs_rel_str_for_matching() {
            return true;
        }
        self.matches_file_str(Path::new(&info.rel_str))
    }

    fn matches_file_str(&self, rel_path: &Path) -> bool {
        if self
            .gitignore
            .as_ref()
            .is_some_and(|m| m.matched(rel_path, false).is_ignore())
        {
            return false;
        }

        if let Some(ref glob) = self.glob {
            // ASCII fast-path for case-insensitive glob
            if self.glob_case_insensitive {
                let rel_str = rel_path.to_string_lossy();
                if rel_str.is_ascii() {
                    let rel_lower = rel_str.to_ascii_lowercase();
                    if glob.matched(Path::new(&rel_lower), false).is_ignore() {
                        return false;
                    }
                    return self.matches_type_glob(rel_path);
                }
            }
            if glob.matched(rel_path, false).is_ignore() {
                return false;
            }
        }

        self.matches_type_glob(rel_path)
    }

    fn matches_type_glob(&self, rel_path: &Path) -> bool {
        if let Some(ref tg) = self.type_glob {
            return !tg.matched(rel_path, false).is_ignore();
        }
        true
    }
}

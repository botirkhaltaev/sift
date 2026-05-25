pub mod candidate;
pub mod config;
pub mod error;
pub mod ignore;
pub mod type_filter;

use std::path::{Path, PathBuf};

use error::FilterError;
use ignore::build_gitignore_matcher;
use type_filter::build_type_glob;

use crate::grep::SearchError;

use ::ignore::gitignore::Gitignore;
use ::ignore::overrides::{Override, OverrideBuilder};

pub use candidate::CandidateInfo;
pub use config::{
    GlobConfig, HiddenMode, IgnoreConfig, IgnoreSources, SearchFilterConfig, TypeDef,
    VisibilityConfig,
};

#[derive(Debug)]
pub struct SearchFilter {
    root: PathBuf,
    scopes: Vec<PathBuf>,
    exclude_paths: Vec<PathBuf>,
    hidden: crate::grep::filter::config::HiddenMode,
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

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn scopes(&self) -> &[PathBuf] {
        &self.scopes
    }

    #[must_use]
    pub(crate) const fn needs_rel_str_for_matching(&self) -> bool {
        self.gitignore.is_some() || self.glob.is_some() || self.type_glob.is_some()
    }

    #[must_use]
    pub const fn follow_links(&self) -> bool {
        self.follow_links
    }

    #[must_use]
    pub const fn max_depth(&self) -> Option<usize> {
        self.max_depth
    }

    #[must_use]
    pub const fn max_filesize(&self) -> Option<u64> {
        self.max_filesize
    }

    #[must_use]
    pub const fn one_file_system(&self) -> bool {
        self.one_file_system
    }

    /// Creates a new search filter from configuration.
    ///
    /// # Errors
    ///
    /// Returns `SearchError` if glob patterns are invalid or type definitions are unknown.
    pub fn new(config: &SearchFilterConfig, index_root: &Path) -> Result<Self, SearchError> {
        let scopes = if config.scopes.is_empty() {
            vec![PathBuf::from("")]
        } else {
            config.scopes.clone()
        };

        let gitignore = build_gitignore_matcher(index_root, &config.visibility.ignore)?;

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
                    SearchError::RegexBuild(format!("invalid glob pattern '{g}': {e}"))
                })?;
            }
            Some(
                builder
                    .build()
                    .map_err(|e| FilterError::RegexBuild(e.to_string()))?,
            )
        };

        let type_glob = build_type_glob(
            index_root,
            &config.type_definitions,
            &config.type_include,
            &config.type_exclude,
        )?;

        Ok(Self {
            root: index_root.to_path_buf(),
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
        if self.hidden == crate::grep::filter::config::HiddenMode::Respect
            && Self::path_is_hidden(rel_path)
        {
            return false;
        }

        if self.needs_rel_str_for_matching() {
            let rel_str = rel_path.to_string_lossy().replace('\\', "/");
            return self.matches_file_str(Path::new(&rel_str));
        }

        true
    }

    fn matches_file_info(&self, info: &CandidateInfo) -> bool {
        if self.hidden == crate::grep::filter::config::HiddenMode::Respect
            && Self::path_is_hidden(&info.rel_path)
        {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grep::filter::config::*;

    fn make_filter(config: &SearchFilterConfig) -> SearchFilter {
        SearchFilter::new(config, Path::new("/root")).expect("create filter")
    }

    #[test]
    fn empty_config_includes_normal_visible_files() {
        let config = SearchFilterConfig::default();
        let filter = make_filter(&config);
        assert!(filter.is_candidate(Path::new("src/lib.rs")));
    }

    #[test]
    fn hidden_paths_rejected_by_default() {
        let config = SearchFilterConfig::default();
        let filter = make_filter(&config);
        assert!(!filter.is_candidate(Path::new(".hidden/file.txt")));
        assert!(!filter.is_candidate(Path::new("dir/.hidden")));
    }

    #[test]
    fn hidden_paths_accepted_with_include_mode() {
        let config = SearchFilterConfig {
            visibility: VisibilityConfig {
                hidden: HiddenMode::Include,
                ignore: IgnoreConfig::default(),
            },
            ..SearchFilterConfig::default()
        };
        let filter = make_filter(&config);
        assert!(filter.is_candidate(Path::new(".hidden/file.txt")));
    }

    #[test]
    fn empty_scopes_include_all_files() {
        let config = SearchFilterConfig::default();
        let filter = make_filter(&config);
        assert!(filter.is_candidate(Path::new("any/path/file.txt")));
    }

    #[test]
    fn specific_scopes_include_matching_prefixes_only() {
        let config = SearchFilterConfig {
            scopes: vec![PathBuf::from("src")],
            ..SearchFilterConfig::default()
        };
        let filter = make_filter(&config);
        assert!(filter.is_candidate(Path::new("src/lib.rs")));
        assert!(!filter.is_candidate(Path::new("tests/test.rs")));
    }

    #[test]
    fn exclude_paths_reject_matching_prefixes() {
        let config = SearchFilterConfig {
            visibility: VisibilityConfig {
                hidden: HiddenMode::Include,
                ignore: IgnoreConfig::default(),
            },
            exclude_paths: vec![PathBuf::from("vendor")],
            ..SearchFilterConfig::default()
        };
        let filter = make_filter(&config);
        assert!(!filter.is_candidate(Path::new("vendor/pkg/file.rs")));
        assert!(filter.is_candidate(Path::new("src/lib.rs")));
    }

    #[test]
    fn candidate_info_and_path_candidate_agree() {
        let config = SearchFilterConfig {
            visibility: VisibilityConfig {
                hidden: HiddenMode::Include,
                ignore: IgnoreConfig::default(),
            },
            ..SearchFilterConfig::default()
        };
        let filter = make_filter(&config);
        let rel_path = Path::new("src/lib.rs");
        let info = CandidateInfo {
            rel_path: rel_path.to_path_buf(),
            rel_str: "src/lib.rs".to_string(),
            abs_path: PathBuf::from("/root/src/lib.rs"),
        };
        assert_eq!(
            filter.is_candidate(rel_path),
            filter.is_candidate_info(&info)
        );
    }

    #[test]
    fn glob_excludes_reject_excluded_paths() {
        let config = SearchFilterConfig {
            visibility: VisibilityConfig {
                hidden: HiddenMode::Include,
                ignore: IgnoreConfig::default(),
            },
            glob: GlobConfig {
                patterns: vec!["!*.log".to_string()],
                case_insensitive: false,
            },
            ..SearchFilterConfig::default()
        };
        let filter = make_filter(&config);
        assert!(!filter.is_candidate(Path::new("debug.log")));
        assert!(filter.is_candidate(Path::new("src/lib.rs")));
    }

    #[test]
    fn case_insensitive_glob_matching_works() {
        let config = SearchFilterConfig {
            visibility: VisibilityConfig {
                hidden: HiddenMode::Include,
                ignore: IgnoreConfig::default(),
            },
            glob: GlobConfig {
                patterns: vec!["!*.LOG".to_string()],
                case_insensitive: true,
            },
            ..SearchFilterConfig::default()
        };
        let filter = make_filter(&config);
        assert!(!filter.is_candidate(Path::new("debug.log")));
        assert!(!filter.is_candidate(Path::new("debug.LOG")));
    }

    #[test]
    fn type_include_accepts_matching_type_globs() {
        let config = SearchFilterConfig {
            visibility: VisibilityConfig {
                hidden: HiddenMode::Include,
                ignore: IgnoreConfig::default(),
            },
            type_definitions: vec![TypeDef {
                name: "rust".to_string(),
                globs: vec!["*.rs".to_string()],
            }],
            type_include: vec!["rust".to_string()],
            ..SearchFilterConfig::default()
        };
        let filter = make_filter(&config);
        assert!(filter.is_candidate(Path::new("src/lib.rs")));
        assert!(!filter.is_candidate(Path::new("src/lib.txt")));
    }

    #[test]
    fn type_exclude_rejects_matching_type_globs() {
        let config = SearchFilterConfig {
            visibility: VisibilityConfig {
                hidden: HiddenMode::Include,
                ignore: IgnoreConfig::default(),
            },
            type_definitions: vec![TypeDef {
                name: "rust".to_string(),
                globs: vec!["*.rs".to_string()],
            }],
            type_exclude: vec!["rust".to_string()],
            ..SearchFilterConfig::default()
        };
        let filter = make_filter(&config);
        assert!(!filter.is_candidate(Path::new("src/lib.rs")));
        assert!(filter.is_candidate(Path::new("src/lib.txt")));
    }

    #[test]
    fn unknown_type_returns_error() {
        let config = SearchFilterConfig {
            type_definitions: vec![],
            type_include: vec!["unknown".to_string()],
            ..SearchFilterConfig::default()
        };
        let result = SearchFilter::new(&config, Path::new("/root"));
        assert!(result.is_err());
    }

    #[test]
    fn invalid_glob_returns_error() {
        let config = SearchFilterConfig {
            glob: GlobConfig {
                patterns: vec!["[invalid".to_string()],
                case_insensitive: false,
            },
            ..SearchFilterConfig::default()
        };
        let result = SearchFilter::new(&config, Path::new("/root"));
        assert!(result.is_err());
    }

    #[test]
    fn filter_accessor_follow_links() {
        let config = SearchFilterConfig {
            follow_links: true,
            ..SearchFilterConfig::default()
        };
        let filter = make_filter(&config);
        assert!(filter.follow_links());
    }

    #[test]
    fn filter_accessor_max_depth() {
        let config = SearchFilterConfig {
            max_depth: Some(5),
            ..SearchFilterConfig::default()
        };
        let filter = make_filter(&config);
        assert_eq!(filter.max_depth(), Some(5));
    }

    #[test]
    fn filter_accessor_max_filesize() {
        let config = SearchFilterConfig {
            max_filesize: Some(1024),
            ..SearchFilterConfig::default()
        };
        let filter = make_filter(&config);
        assert_eq!(filter.max_filesize(), Some(1024));
    }

    #[test]
    fn filter_accessor_one_file_system() {
        let config = SearchFilterConfig {
            one_file_system: true,
            ..SearchFilterConfig::default()
        };
        let filter = make_filter(&config);
        assert!(filter.one_file_system());
    }

    #[test]
    fn filter_accessor_scopes() {
        let config = SearchFilterConfig {
            scopes: vec![PathBuf::from("src")],
            ..SearchFilterConfig::default()
        };
        let filter = make_filter(&config);
        assert_eq!(filter.scopes(), &[PathBuf::from("src")]);
    }
}

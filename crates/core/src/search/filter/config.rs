use std::path::PathBuf;

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
    pub follow_links: bool,
    pub max_depth: Option<usize>,
    pub max_filesize: Option<u64>,
    pub type_definitions: Vec<TypeDef>,
    pub type_include: Vec<String>,
    pub type_exclude: Vec<String>,
    pub one_file_system: bool,
}

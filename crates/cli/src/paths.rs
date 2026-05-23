use std::path::{Path, PathBuf};

use clap::Args;
use sift_core::PathDisplay;

#[derive(Args)]
pub struct PathArgs {
    #[arg(short = 'm', long = "max-count", value_name = "NUM")]
    pub max_count: Option<usize>,
    #[arg(long, default_value = ".sift")]
    pub sift_dir: PathBuf,
    #[arg(short = 'L', long = "follow")]
    pub follow: bool,
}

pub fn corpus_path_prefixes(
    index_root: &Path,
    cwd: &Path,
    requested: &[PathBuf],
) -> anyhow::Result<Vec<PathBuf>> {
    if requested.is_empty() {
        return Ok(vec![PathBuf::from("")]);
    }
    let index_root = index_root
        .canonicalize()
        .unwrap_or_else(|_| index_root.to_path_buf());
    let mut out = Vec::with_capacity(requested.len());
    for rel in requested {
        let abs = if rel.is_absolute() {
            rel.clone()
        } else {
            cwd.join(rel)
        };
        let abs = abs.canonicalize().unwrap_or(abs);
        if !abs.starts_with(&index_root) {
            anyhow::bail!(
                "path {} is not under indexed corpus root {}",
                abs.display(),
                index_root.display()
            );
        }
        out.push(
            abs.strip_prefix(&index_root)
                .expect("prefix checked")
                .to_path_buf(),
        );
    }
    Ok(out)
}

pub fn walk_path_prefixes(cwd: &Path, requested: &[PathBuf]) -> anyhow::Result<Vec<PathBuf>> {
    if requested.is_empty() {
        return Ok(vec![PathBuf::from("")]);
    }
    let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let mut out = Vec::with_capacity(requested.len());
    for rel in requested {
        let abs = if rel.is_absolute() {
            rel.clone()
        } else {
            cwd.join(rel)
        };
        let abs = abs.canonicalize().unwrap_or(abs);
        if !abs.starts_with(&cwd) {
            anyhow::bail!("path {} is not under {}", abs.display(), cwd.display());
        }
        out.push(
            abs.strip_prefix(&cwd)
                .expect("prefix checked")
                .to_path_buf(),
        );
    }
    Ok(out)
}

pub fn excluded_search_paths(search_root: &Path, sift_dir: &Path) -> Vec<PathBuf> {
    let abs = if sift_dir.is_absolute() {
        sift_dir.to_path_buf()
    } else {
        std::env::current_dir().map_or_else(|_| sift_dir.to_path_buf(), |cwd| cwd.join(sift_dir))
    };
    let abs = abs.canonicalize().unwrap_or(abs);
    let root = search_root
        .canonicalize()
        .unwrap_or_else(|_| search_root.to_path_buf());
    if abs.starts_with(&root) {
        vec![
            abs.strip_prefix(&root)
                .expect("prefix checked")
                .to_path_buf(),
        ]
    } else {
        Vec::new()
    }
}

pub fn effective_path_display(scopes: &[PathBuf]) -> PathDisplay {
    for scope in scopes {
        if scope.is_absolute() {
            return PathDisplay::Absolute;
        }
    }
    PathDisplay::Relative
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_display_relative_when_empty() {
        assert_eq!(effective_path_display(&[]), PathDisplay::Relative);
    }

    #[test]
    fn path_display_relative_when_relative() {
        assert_eq!(
            effective_path_display(&[PathBuf::from("src")]),
            PathDisplay::Relative
        );
    }

    #[cfg(unix)]
    #[test]
    fn path_display_absolute_when_absolute_unix() {
        assert_eq!(
            effective_path_display(&[PathBuf::from("/home/user")]),
            PathDisplay::Absolute
        );
    }

    #[cfg(windows)]
    #[test]
    fn path_display_absolute_when_absolute_windows() {
        assert_eq!(
            effective_path_display(&[PathBuf::from("C:\\Users")]),
            PathDisplay::Absolute
        );
    }

    #[cfg(unix)]
    #[test]
    fn path_display_absolute_when_first_is_absolute_unix() {
        assert_eq!(
            effective_path_display(&[PathBuf::from("/root"), PathBuf::from("sub")]),
            PathDisplay::Absolute
        );
    }

    #[cfg(windows)]
    #[test]
    fn path_display_absolute_when_first_is_absolute_windows() {
        assert_eq!(
            effective_path_display(&[PathBuf::from("D:\\projects"), PathBuf::from("sub")]),
            PathDisplay::Absolute
        );
    }

    #[test]
    fn path_display_relative_when_all_relative() {
        assert_eq!(
            effective_path_display(&[PathBuf::from("a"), PathBuf::from("b/c")]),
            PathDisplay::Relative
        );
    }
}

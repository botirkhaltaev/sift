use std::collections::HashSet;
use std::path::{Path, PathBuf};

use rayon::prelude::*;

use crate::grep::execution::config::{LinkTraversal, WalkOptions};
use crate::grep::filter::{CandidateInfo, SearchFilter};

pub fn walk_directory_files(root: &Path, filter: &SearchFilter) -> crate::Result<Vec<PathBuf>> {
    let root = root.canonicalize()?;
    let mut builder = ignore::WalkBuilder::new(&root);
    builder
        .follow_links(filter.follow_links())
        .same_file_system(filter.one_file_system())
        .hidden(false)
        .parents(false)
        .ignore(false)
        .git_global(false)
        .git_ignore(false)
        .git_exclude(false)
        .require_git(false);
    if let Some(d) = filter.max_depth() {
        builder.max_depth(Some(d + 1));
    }
    let mut out = Vec::new();
    for entry in builder.build() {
        let entry = entry.map_err(crate::Error::Ignore)?;
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        if let Some(limit) = filter.max_filesize() {
            let skip = entry.metadata().is_ok_and(|m| m.len() > limit);
            if skip {
                continue;
            }
        }
        out.push(entry.path().to_path_buf());
    }
    Ok(out)
}

pub fn collect_abs_paths_for_scopes(filter: &SearchFilter) -> crate::Result<Vec<PathBuf>> {
    let filter_root = filter.root().canonicalize()?;
    let mut out = Vec::new();
    for scope in filter.scopes() {
        let path = if scope.as_os_str().is_empty() {
            filter_root.clone()
        } else {
            filter_root.join(scope)
        };
        if !path.exists() {
            continue;
        }
        let path = path.canonicalize().unwrap_or(path);
        if path.is_file() {
            out.push(path);
        } else if path.is_dir() {
            out.extend(walk_directory_files(&path, filter)?);
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

pub fn prepare_walk_candidates(
    abs_paths: &[PathBuf],
    filter: &SearchFilter,
    threshold: usize,
) -> Vec<CandidateInfo> {
    let filter_root = filter
        .root()
        .canonicalize()
        .unwrap_or_else(|_| filter.root().to_path_buf());
    let cap = abs_paths.len();
    let need_rel = filter.needs_rel_str_for_matching();

    if abs_paths.len() >= threshold {
        abs_paths
            .par_iter()
            .filter_map(|abs_path| {
                let rel_path = abs_path
                    .strip_prefix(&filter_root)
                    .unwrap_or(abs_path.as_path())
                    .to_path_buf();
                let rel_str = if need_rel {
                    rel_path.to_string_lossy().replace('\\', "/")
                } else {
                    String::new()
                };
                let info = CandidateInfo {
                    rel_path,
                    rel_str,
                    abs_path: abs_path.clone(),
                };
                filter.is_candidate_info(&info).then_some(info)
            })
            .collect()
    } else {
        let mut out = Vec::with_capacity(cap);
        for abs_path in abs_paths {
            let rel_path = abs_path
                .strip_prefix(&filter_root)
                .unwrap_or(abs_path.as_path())
                .to_path_buf();
            let rel_str = if need_rel {
                rel_path.to_string_lossy().replace('\\', "/")
            } else {
                String::new()
            };
            let info = CandidateInfo {
                rel_path,
                rel_str,
                abs_path: abs_path.clone(),
            };
            if filter.is_candidate_info(&info) {
                out.push(info);
            }
        }
        out
    }
}

/// Discovers files under the given root matching the walk options.
///
/// # Errors
///
/// Returns an error if the root path cannot be canonicalized or
/// the walk encounters an inaccessible directory.
pub fn discover_files(root: &Path, options: WalkOptions) -> crate::Result<HashSet<PathBuf>> {
    let root = root.canonicalize()?;
    let mut set = HashSet::new();
    let follow = matches!(options.links, LinkTraversal::Follow);
    let mut builder = ignore::WalkBuilder::new(&root);
    builder.follow_links(follow);
    if let Some(depth) = options.max_depth {
        builder.max_depth(Some(depth + 1));
    }
    builder.same_file_system(options.one_file_system);
    let walker = builder.build();
    for entry in walker {
        let entry = entry.map_err(crate::Error::Ignore)?;
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        let path = entry.path();
        if options
            .max_filesize
            .is_some_and(|limit| std::fs::metadata(path).is_ok_and(|m| m.len() > limit))
        {
            continue;
        }
        let display = path.strip_prefix(&root).unwrap_or(path).to_path_buf();
        set.insert(display);
    }
    Ok(set)
}

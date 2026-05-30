use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::Candidate;
use crate::search::filter::{CandidateFilter, HiddenMode, IgnoreSources};
use crate::search::request::{LinkTraversal, WalkOptions};

fn walk_directory_files(root: &Path, filter: &CandidateFilter) -> crate::Result<Vec<Candidate>> {
    let root = root.canonicalize()?;
    let visibility = filter.visibility();
    let sources = visibility.ignore.sources;
    let mut builder = ignore::WalkBuilder::new(&root);
    builder
        .follow_links(filter.follow_links())
        .same_file_system(filter.one_file_system())
        .hidden(matches!(visibility.hidden, HiddenMode::Respect))
        .parents(sources.contains(IgnoreSources::PARENT))
        .ignore(sources.contains(IgnoreSources::DOT))
        .git_ignore(sources.contains(IgnoreSources::VCS))
        .git_exclude(sources.contains(IgnoreSources::EXCLUDE))
        .git_global(sources.contains(IgnoreSources::GLOBAL))
        .require_git(visibility.ignore.require_git);
    if let Some(d) = filter.max_depth() {
        builder.max_depth(Some(d + 1));
    }
    let filter_root = filter
        .root()
        .canonicalize()
        .unwrap_or_else(|_| filter.root().to_path_buf());
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
        let abs_path = entry.path().to_path_buf();
        let rel_path = abs_path
            .strip_prefix(&filter_root)
            .unwrap_or(&abs_path)
            .to_path_buf();
        out.push(Candidate::new(rel_path, abs_path));
    }
    Ok(out)
}

/// Collect candidate files across all scopes by walking the filesystem.
pub fn collect_candidates(filter: &CandidateFilter) -> crate::Result<Vec<Candidate>> {
    let filter_root = filter
        .root()
        .canonicalize()
        .unwrap_or_else(|_| filter.root().to_path_buf());
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
            let rel_path = path
                .strip_prefix(&filter_root)
                .unwrap_or(&path)
                .to_path_buf();
            out.push(Candidate::new(rel_path, path));
        } else if path.is_dir() {
            out.extend(walk_directory_files(&path, filter)?);
        }
    }
    out.sort_by(|a, b| a.rel_path().cmp(b.rel_path()));
    out.dedup();
    Ok(out)
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

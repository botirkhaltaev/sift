use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ignore::{DirEntry, Error as IgnoreError, WalkBuilder, WalkState};

use crate::Candidate;
use crate::search::filter::{CandidateFilter, HiddenMode, IgnoreSources};
use crate::search::request::{LinkTraversal, WalkOptions};

/// Per-scope parallel walk that produces candidates.
///
/// Also serves as [`ignore::ParallelVisitorBuilder`] — each worker thread
/// receives a thread-local [`CandidateCollector`].
struct CandidateWalk<'a> {
    filter: &'a CandidateFilter,
    root: PathBuf,
    filter_root: PathBuf,
    walk_error: Arc<Mutex<Option<crate::Error>>>,
    consolidated: Arc<Mutex<Vec<Candidate>>>,
}

impl<'a> CandidateWalk<'a> {
    fn new(root: &Path, filter: &'a CandidateFilter) -> crate::Result<Self> {
        let root = root.canonicalize()?;
        let filter_root = filter
            .root()
            .canonicalize()
            .unwrap_or_else(|_| filter.root().to_path_buf());
        Ok(Self {
            filter,
            root,
            filter_root,
            walk_error: Arc::new(Mutex::new(None)),
            consolidated: Arc::new(Mutex::new(Vec::new())),
        })
    }

    fn walk_builder(&self) -> WalkBuilder {
        let visibility = self.filter.visibility();
        let sources = visibility.ignore.sources;
        let mut builder = WalkBuilder::new(&self.root);
        builder
            .follow_links(self.filter.follow_links())
            .same_file_system(self.filter.one_file_system())
            .hidden(matches!(visibility.hidden, HiddenMode::Respect))
            .parents(sources.contains(IgnoreSources::PARENT))
            .ignore(sources.contains(IgnoreSources::DOT))
            .git_ignore(sources.contains(IgnoreSources::VCS))
            .git_exclude(sources.contains(IgnoreSources::EXCLUDE))
            .git_global(sources.contains(IgnoreSources::GLOBAL))
            .require_git(visibility.ignore.require_git);
        if let Some(d) = self.filter.max_depth() {
            builder.max_depth(Some(d + 1));
        }
        builder
    }

    fn collect(&mut self) -> crate::Result<Vec<Candidate>> {
        self.walk_builder().build_parallel().visit(self);

        {
            let mut guard = self.walk_error.lock().expect("walk error lock");
            if let Some(err) = guard.take() {
                return Err(err);
            }
        }

        Ok(std::mem::take(
            &mut *self.consolidated.lock().expect("candidate lock"),
        ))
    }
}

impl<'a> ignore::ParallelVisitorBuilder<'a> for CandidateWalk<'_> {
    fn build(&mut self) -> Box<dyn ignore::ParallelVisitor + 'a> {
        Box::new(CandidateCollector {
            filter_root: self.filter_root.clone(),
            max_filesize: self.filter.max_filesize(),
            thread_candidates: Vec::new(),
            walk_error: Arc::clone(&self.walk_error),
            consolidated: Arc::clone(&self.consolidated),
        })
    }
}

/// Per-thread collector for the parallel walk.
struct CandidateCollector {
    filter_root: PathBuf,
    max_filesize: Option<u64>,
    thread_candidates: Vec<Candidate>,
    walk_error: Arc<Mutex<Option<crate::Error>>>,
    consolidated: Arc<Mutex<Vec<Candidate>>>,
}

impl Drop for CandidateCollector {
    fn drop(&mut self) {
        if self.thread_candidates.is_empty() {
            return;
        }
        let mut guard = self.consolidated.lock().expect("candidate lock");
        guard.append(&mut self.thread_candidates);
    }
}

impl ignore::ParallelVisitor for CandidateCollector {
    fn visit(&mut self, entry: Result<DirEntry, IgnoreError>) -> WalkState {
        let entry = match entry {
            Ok(e) => e,
            Err(err) => {
                let mut guard = self.walk_error.lock().expect("walk error lock");
                if guard.is_none() {
                    *guard = Some(crate::Error::Ignore(err));
                }
                drop(guard);
                return WalkState::Quit;
            }
        };
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            return WalkState::Continue;
        }
        if let Some(limit) = self.max_filesize
            && entry.metadata().is_ok_and(|m| m.len() > limit)
        {
            return WalkState::Continue;
        }
        let abs_path = entry.path().to_path_buf();
        let rel_path = abs_path
            .strip_prefix(&self.filter_root)
            .unwrap_or(&abs_path)
            .to_path_buf();
        self.thread_candidates
            .push(Candidate::new(rel_path, abs_path));
        WalkState::Continue
    }
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
            let mut walk = CandidateWalk::new(&path, filter)?;
            out.extend(walk.collect()?);
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

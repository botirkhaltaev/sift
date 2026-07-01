use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ignore::{DirEntry, Error as IgnoreError, WalkBuilder, WalkState};

use crate::corpus::candidate::Candidate;
use crate::corpus::filter::{
    CandidateFilter, HiddenMode, IgnoreSources, VisibilityConfig,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LinkTraversal {
    #[default]
    DoNotFollow,
    Follow,
}

/// Reusable filesystem discovery over one corpus root.
pub struct FileWalk<'a> {
    root: &'a Path,
    scopes: &'a [PathBuf],
    excludes: &'a [PathBuf],
    visibility: VisibilityConfig,
    links: LinkTraversal,
    one_file_system: bool,
    max_depth: Option<usize>,
    max_filesize: Option<u64>,
}

impl<'a> FileWalk<'a> {
    #[must_use]
    pub fn new(root: &'a Path) -> Self {
        Self {
            root,
            scopes: &[],
            excludes: &[],
            visibility: VisibilityConfig::default(),
            links: LinkTraversal::DoNotFollow,
            one_file_system: false,
            max_depth: None,
            max_filesize: None,
        }
    }

    #[must_use]
    pub fn from_filter(filter: &'a CandidateFilter) -> Self {
        Self::new(filter.root())
            .scopes(filter.scopes())
            .excludes(filter.exclude_paths())
            .visibility(filter.visibility().clone())
            .links(if filter.follow_links() {
                LinkTraversal::Follow
            } else {
                LinkTraversal::DoNotFollow
            })
            .one_file_system(filter.one_file_system())
            .max_depth(filter.max_depth())
            .max_filesize(filter.max_filesize())
    }

    #[must_use]
    pub const fn scopes(mut self, scopes: &'a [PathBuf]) -> Self {
        self.scopes = scopes;
        self
    }

    #[must_use]
    pub const fn excludes(mut self, excludes: &'a [PathBuf]) -> Self {
        self.excludes = excludes;
        self
    }

    #[must_use]
    pub fn visibility(mut self, visibility: VisibilityConfig) -> Self {
        self.visibility = visibility;
        self
    }

    #[must_use]
    pub const fn links(mut self, links: LinkTraversal) -> Self {
        self.links = links;
        self
    }

    #[must_use]
    pub const fn one_file_system(mut self, enabled: bool) -> Self {
        self.one_file_system = enabled;
        self
    }

    #[must_use]
    pub const fn max_depth(mut self, depth: Option<usize>) -> Self {
        self.max_depth = depth;
        self
    }

    #[must_use]
    pub const fn max_filesize(mut self, size: Option<u64>) -> Self {
        self.max_filesize = size;
        self
    }

    /// Collect matching files as candidates with cached metadata.
    ///
    /// # Errors
    ///
    /// Returns an error if the root cannot be canonicalized or a walk fails.
    pub fn collect(self) -> crate::Result<Vec<Candidate>> {
        let filter_root = self.root.canonicalize()?;
        let mut out = Vec::new();
        if self.scopes.is_empty() {
            self.collect_scope(&filter_root, Path::new(""), &mut out)?;
        } else {
            for scope in self.scopes {
                self.collect_scope(&filter_root, scope, &mut out)?;
            }
        }
        out.sort_by(|a, b| a.rel_path().cmp(b.rel_path()));
        out.dedup();
        Ok(out)
    }

    /// Collect matching file paths relative to the walk root.
    ///
    /// # Errors
    ///
    /// Returns an error if filesystem discovery fails.
    pub fn collect_paths(self) -> crate::Result<Vec<PathBuf>> {
        Ok(self
            .collect()?
            .into_iter()
            .map(|candidate| candidate.rel_path().to_path_buf())
            .collect())
    }

    fn is_excluded(&self, rel: &Path) -> bool {
        self.excludes
            .iter()
            .any(|excluded| !excluded.as_os_str().is_empty() && rel.starts_with(excluded))
    }

    fn collect_scope(
        &self,
        filter_root: &Path,
        scope: &Path,
        out: &mut Vec<Candidate>,
    ) -> crate::Result<()> {
        let path = if scope.as_os_str().is_empty() {
            filter_root.to_path_buf()
        } else {
            filter_root.join(scope)
        };
        if !path.exists() {
            return Ok(());
        }
        let path = path.canonicalize().unwrap_or(path);
        if path.is_file() {
            let rel_path = path
                .strip_prefix(filter_root)
                .unwrap_or(&path)
                .to_path_buf();
            if !self.is_excluded(&rel_path) {
                out.push(Candidate::new(rel_path, path));
            }
        } else if path.is_dir() {
            let mut walk = FileWalkRun::new(&path, filter_root, self);
            out.extend(walk.collect()?);
        }
        Ok(())
    }
}

struct FileWalkRun<'a> {
    root: PathBuf,
    filter_root: &'a Path,
    config: &'a FileWalk<'a>,
    walk_error: Arc<Mutex<Option<crate::Error>>>,
    consolidated: Arc<Mutex<Vec<Candidate>>>,
}

impl<'a> FileWalkRun<'a> {
    fn new(root: &Path, filter_root: &'a Path, config: &'a FileWalk<'a>) -> Self {
        Self {
            root: root.to_path_buf(),
            filter_root,
            config,
            walk_error: Arc::new(Mutex::new(None)),
            consolidated: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn walk_builder(&self) -> WalkBuilder {
        let sources = self.config.visibility.ignore.sources;
        let mut builder = WalkBuilder::new(&self.root);
        builder
            .follow_links(matches!(self.config.links, LinkTraversal::Follow))
            .same_file_system(self.config.one_file_system)
            .hidden(matches!(self.config.visibility.hidden, HiddenMode::Respect))
            .parents(sources.contains(IgnoreSources::PARENT))
            .ignore(sources.contains(IgnoreSources::DOT))
            .git_ignore(sources.contains(IgnoreSources::VCS))
            .git_exclude(sources.contains(IgnoreSources::EXCLUDE))
            .git_global(sources.contains(IgnoreSources::GLOBAL))
            .require_git(self.config.visibility.ignore.require_git);
        if let Some(depth) = self.config.max_depth {
            builder.max_depth(Some(depth + 1));
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

impl<'a> ignore::ParallelVisitorBuilder<'a> for FileWalkRun<'_> {
    fn build(&mut self) -> Box<dyn ignore::ParallelVisitor + 'a> {
        Box::new(FileCollector {
            filter_root: self.filter_root.to_path_buf(),
            excludes: self.config.excludes.to_vec(),
            max_filesize: self.config.max_filesize,
            thread_candidates: Vec::new(),
            walk_error: Arc::clone(&self.walk_error),
            consolidated: Arc::clone(&self.consolidated),
        })
    }
}

struct FileCollector {
    filter_root: PathBuf,
    excludes: Vec<PathBuf>,
    max_filesize: Option<u64>,
    thread_candidates: Vec<Candidate>,
    walk_error: Arc<Mutex<Option<crate::Error>>>,
    consolidated: Arc<Mutex<Vec<Candidate>>>,
}

impl Drop for FileCollector {
    fn drop(&mut self) {
        if self.thread_candidates.is_empty() {
            return;
        }
        let mut guard = self.consolidated.lock().expect("candidate lock");
        guard.append(&mut self.thread_candidates);
    }
}

impl ignore::ParallelVisitor for FileCollector {
    fn visit(&mut self, entry: Result<DirEntry, IgnoreError>) -> WalkState {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                let mut guard = self.walk_error.lock().expect("walk error lock");
                if guard.is_none() {
                    *guard = Some(crate::Error::Ignore(err));
                }
                drop(guard);
                return WalkState::Quit;
            }
        };

        let abs_path = entry.path();
        let rel_path = abs_path
            .strip_prefix(&self.filter_root)
            .unwrap_or(abs_path)
            .to_path_buf();
        if self.is_excluded(&rel_path) {
            return if entry.file_type().is_some_and(|ft| ft.is_dir()) {
                WalkState::Skip
            } else {
                WalkState::Continue
            };
        }
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            return WalkState::Continue;
        }

        let metadata = entry.metadata().ok();
        if let Some(limit) = self.max_filesize
            && metadata.as_ref().is_some_and(|m| m.len() > limit)
        {
            return WalkState::Continue;
        }
        self.thread_candidates.push(Candidate::with_metadata(
            rel_path,
            abs_path.to_path_buf(),
            metadata.map(|m| m.len()),
            Some(entry.depth().saturating_sub(1)),
        ));
        WalkState::Continue
    }
}

impl FileCollector {
    fn is_excluded(&self, rel_path: &Path) -> bool {
        self.excludes
            .iter()
            .any(|excluded| !excluded.as_os_str().is_empty() && rel_path.starts_with(excluded))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use tempfile::TempDir;

    use super::*;
    use crate::corpus::filter::{HiddenMode, IgnoreConfig};

    fn raw_visibility() -> VisibilityConfig {
        VisibilityConfig {
            hidden: HiddenMode::Include,
            ignore: IgnoreConfig::disabled(),
        }
    }

    #[test]
    fn empty_scopes_walk_root() {
        let tmp = TempDir::new().expect("tempdir");
        std::fs::write(tmp.path().join("a.txt"), "a").expect("write file");

        let paths = FileWalk::new(tmp.path())
            .visibility(raw_visibility())
            .collect_paths()
            .expect("walk");

        assert_eq!(paths, vec![PathBuf::from("a.txt")]);
    }

    #[test]
    fn scopes_limit_walk() {
        let tmp = TempDir::new().expect("tempdir");
        std::fs::create_dir(tmp.path().join("src")).expect("mkdir");
        std::fs::write(tmp.path().join("src/lib.rs"), "lib").expect("write file");
        std::fs::write(tmp.path().join("README.md"), "readme").expect("write file");
        let scopes = [PathBuf::from("src")];

        let paths = FileWalk::new(tmp.path())
            .scopes(&scopes)
            .visibility(raw_visibility())
            .collect_paths()
            .expect("walk");

        assert_eq!(paths, vec![PathBuf::from("src/lib.rs")]);
    }

    #[test]
    fn excludes_prune_subtrees() {
        let tmp = TempDir::new().expect("tempdir");
        std::fs::create_dir(tmp.path().join("target")).expect("mkdir");
        std::fs::write(tmp.path().join("target/generated.rs"), "generated").expect("write file");
        std::fs::write(tmp.path().join("src.rs"), "src").expect("write file");
        let excludes = [PathBuf::from("target")];

        let paths = FileWalk::new(tmp.path())
            .excludes(&excludes)
            .visibility(raw_visibility())
            .collect_paths()
            .expect("walk");

        assert_eq!(paths, vec![PathBuf::from("src.rs")]);
    }
}

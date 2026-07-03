use std::fs::Metadata;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ignore::{DirEntry, Error as IgnoreError, WalkBuilder, WalkState};

use crate::corpus::candidate::Candidate;
use crate::corpus::filter::{CandidateFilter, HiddenMode, IgnoreSources, VisibilityConfig};

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

    /// Collect matching files as caller-selected records.
    ///
    /// # Errors
    ///
    /// Returns an error if the root cannot be canonicalized or a walk fails.
    pub fn collect<P: WalkProjection>(self, projection: &P) -> crate::Result<Vec<P::Record>> {
        let records = self.discover(projection)?;
        Ok(records.into_iter().map(WalkOutput::into_record).collect())
    }

    fn discover<P: WalkProjection>(
        self,
        projection: &P,
    ) -> crate::Result<Vec<WalkOutput<P::Record>>> {
        let filter_root = self.root.canonicalize()?;
        let mut out: Vec<WalkOutput<P::Record>> = Vec::new();
        if self.scopes.is_empty() {
            self.collect_scope(&filter_root, Path::new(""), projection, &mut out)?;
        } else {
            for scope in self.scopes {
                self.collect_scope(&filter_root, scope, projection, &mut out)?;
            }
        }
        out.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
        out.dedup_by(|a, b| a.rel_path == b.rel_path);
        Ok(out)
    }

    fn is_excluded(&self, rel: &Path) -> bool {
        self.excludes
            .iter()
            .any(|excluded| !excluded.as_os_str().is_empty() && rel.starts_with(excluded))
    }

    fn collect_scope<P: WalkProjection>(
        &self,
        filter_root: &Path,
        scope: &Path,
        projection: &P,
        out: &mut Vec<WalkOutput<P::Record>>,
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
            if !self.is_excluded(&rel_path)
                && let Some(file) = WalkFile::from_scope_file(rel_path, path, self.max_filesize)
            {
                out.push(WalkOutput::new(&file, projection.project(&file)));
            }
        } else if path.is_dir() {
            let mut walk = FileWalkRun::new(&path, filter_root, self, projection.clone());
            out.extend(walk.run()?);
        }
        Ok(())
    }
}

pub struct WalkFile {
    rel_path: PathBuf,
    abs_path: PathBuf,
    depth: usize,
    metadata: Option<Metadata>,
}

impl WalkFile {
    fn from_scope_file(
        rel_path: PathBuf,
        abs_path: PathBuf,
        max_filesize: Option<u64>,
    ) -> Option<Self> {
        let metadata = std::fs::metadata(&abs_path).ok();
        if let Some(limit) = max_filesize
            && metadata.as_ref().is_some_and(|m| m.len() > limit)
        {
            return None;
        }
        let depth = rel_path.components().count().saturating_sub(1);
        Some(Self {
            rel_path,
            abs_path,
            depth,
            metadata,
        })
    }

    #[must_use]
    pub const fn rel_path(&self) -> &PathBuf {
        &self.rel_path
    }

    #[must_use]
    pub fn abs_path(&self) -> &Path {
        self.abs_path.as_path()
    }

    #[must_use]
    pub const fn metadata(&self) -> Option<&Metadata> {
        self.metadata.as_ref()
    }

    #[must_use]
    pub const fn depth(&self) -> usize {
        self.depth
    }

    #[must_use]
    pub fn size(&self) -> Option<u64> {
        self.metadata.as_ref().map(Metadata::len)
    }
}

pub trait WalkProjection: Clone + Send + Sync + 'static {
    type Record: Send + 'static;

    const NEEDS_METADATA: bool = false;

    fn project(&self, file: &WalkFile) -> Self::Record;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CandidateRecords;

impl WalkProjection for CandidateRecords {
    type Record = Candidate;

    const NEEDS_METADATA: bool = true;

    fn project(&self, file: &WalkFile) -> Self::Record {
        Candidate::with_metadata(
            file.rel_path().clone(),
            file.abs_path().to_path_buf(),
            file.size(),
            Some(file.depth()),
        )
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RelativePaths;

impl WalkProjection for RelativePaths {
    type Record = PathBuf;

    fn project(&self, file: &WalkFile) -> Self::Record {
        file.rel_path().clone()
    }
}

struct WalkOutput<T> {
    rel_path: PathBuf,
    record: T,
}

impl<T> WalkOutput<T> {
    fn new(file: &WalkFile, record: T) -> Self {
        Self {
            rel_path: file.rel_path().clone(),
            record,
        }
    }

    fn into_record(self) -> T {
        self.record
    }
}

struct FileWalkRun<'a, P: WalkProjection> {
    root: PathBuf,
    filter_root: &'a Path,
    config: &'a FileWalk<'a>,
    projection: P,
    walk_error: Arc<Mutex<Option<crate::Error>>>,
    consolidated: Arc<Mutex<Vec<WalkOutput<P::Record>>>>,
}

impl<'a, P: WalkProjection> FileWalkRun<'a, P> {
    fn new(root: &Path, filter_root: &'a Path, config: &'a FileWalk<'a>, projection: P) -> Self {
        Self {
            root: root.to_path_buf(),
            filter_root,
            config,
            projection,
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

    fn run(&mut self) -> crate::Result<Vec<WalkOutput<P::Record>>> {
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

impl<'a, P: WalkProjection> ignore::ParallelVisitorBuilder<'a> for FileWalkRun<'_, P> {
    fn build(&mut self) -> Box<dyn ignore::ParallelVisitor + 'a> {
        Box::new(FileCollector::<P> {
            filter_root: self.filter_root.to_path_buf(),
            excludes: self.config.excludes.to_vec(),
            max_filesize: self.config.max_filesize,
            projection: self.projection.clone(),
            thread_items: Vec::new(),
            walk_error: Arc::clone(&self.walk_error),
            consolidated: Arc::clone(&self.consolidated),
        })
    }
}

struct FileCollector<P: WalkProjection> {
    filter_root: PathBuf,
    excludes: Vec<PathBuf>,
    max_filesize: Option<u64>,
    projection: P,
    thread_items: Vec<WalkOutput<P::Record>>,
    walk_error: Arc<Mutex<Option<crate::Error>>>,
    consolidated: Arc<Mutex<Vec<WalkOutput<P::Record>>>>,
}

impl<P: WalkProjection> Drop for FileCollector<P> {
    fn drop(&mut self) {
        if self.thread_items.is_empty() {
            return;
        }
        let mut guard = self.consolidated.lock().expect("candidate lock");
        guard.append(&mut self.thread_items);
    }
}

impl<P: WalkProjection> ignore::ParallelVisitor for FileCollector<P> {
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

        let metadata = if self.max_filesize.is_some() || P::NEEDS_METADATA {
            entry.metadata().ok()
        } else {
            None
        };
        if let Some(limit) = self.max_filesize
            && metadata.as_ref().is_some_and(|m| m.len() > limit)
        {
            return WalkState::Continue;
        }
        let file = WalkFile {
            rel_path,
            abs_path: abs_path.to_path_buf(),
            depth: entry.depth().saturating_sub(1),
            metadata,
        };
        self.thread_items
            .push(WalkOutput::new(&file, self.projection.project(&file)));
        WalkState::Continue
    }
}

impl<P: WalkProjection> FileCollector<P> {
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

    struct WalkSummary {
        path: PathBuf,
        size: Option<u64>,
    }

    #[derive(Clone, Copy)]
    struct WalkSummaries;

    impl WalkProjection for WalkSummaries {
        type Record = WalkSummary;

        const NEEDS_METADATA: bool = true;

        fn project(&self, file: &WalkFile) -> Self::Record {
            WalkSummary {
                path: file.rel_path().clone(),
                size: file.size(),
            }
        }
    }

    #[test]
    fn empty_scopes_walk_root() {
        let tmp = TempDir::new().expect("tempdir");
        std::fs::write(tmp.path().join("a.txt"), "a").expect("write file");

        let paths = FileWalk::new(tmp.path())
            .visibility(raw_visibility())
            .collect(&RelativePaths)
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
            .collect(&RelativePaths)
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
            .collect(&RelativePaths)
            .expect("walk");

        assert_eq!(paths, vec![PathBuf::from("src.rs")]);
    }

    #[test]
    fn collects_custom_walk_records() {
        let tmp = TempDir::new().expect("tempdir");
        std::fs::write(tmp.path().join("a.txt"), "alpha").expect("write file");

        let records = FileWalk::new(tmp.path())
            .visibility(raw_visibility())
            .collect(&WalkSummaries)
            .expect("walk");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].path, PathBuf::from("a.txt"));
        assert_eq!(records[0].size, Some(5));
    }
}

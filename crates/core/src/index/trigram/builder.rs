//! Walk corpus, extract trigrams, build in-memory index tables.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ignore::{
    DirEntry, Error as IgnoreError, ParallelVisitor, ParallelVisitorBuilder, WalkBuilder, WalkState,
};
use rayon::prelude::*;

use super::Trigram;
use super::file_table::FileFingerprint;
use super::storage::lexicon::LexiconEntry;
use super::storage::mmap::open_mmap;
use super::storage::trigram_sets::TrigramSet;

use crate::index::{CorpusKind, IndexBuildConfig};
use crate::search::filter::ignore::build_gitignore_matcher;
use crate::search::filter::{HiddenMode, IgnoreSources, VisibilityConfig};

/// Collected index data ready for persistence.
pub struct IndexTables {
    pub fingerprints: Vec<FileFingerprint>,
    pub lexicon: Vec<LexiconEntry>,
    pub postings: Vec<u8>,
}

/// Walks a corpus and builds index tables.
pub struct IndexTableBuilder<'a> {
    config: &'a IndexBuildConfig<'a>,
}

impl<'a> IndexTableBuilder<'a> {
    #[must_use]
    pub const fn new(config: &'a IndexBuildConfig<'a>) -> Self {
        Self { config }
    }

    /// Build the index tables.
    ///
    /// # Errors
    ///
    /// Returns an error if corpus walking, file I/O, or trigram extraction fails.
    pub fn build(self) -> crate::Result<IndexTables> {
        let paths = CorpusWalker::new(self.config).collect()?;
        let fingerprints = FingerprintCollector::new(self.config.root, &paths).collect()?;

        let extractor = TrigramExtractor::new(self.config.root, &fingerprints);
        let file_trigrams = extractor.extract()?;

        let (lexicon, postings) = PostingAssembler::new(&file_trigrams).assemble()?;

        Ok(IndexTables {
            fingerprints,
            lexicon,
            postings,
        })
    }
}

/// Per-thread collector for [`WalkParallel::visit`]. Each worker buffers relative paths
/// without synchronization; on drop the buffer is merged into a shared list.
struct CorpusPathCollector<'a> {
    root: PathBuf,
    exclude_paths: &'a [PathBuf],
    include_paths: &'a [PathBuf],
    gitignore: Option<Arc<ignore::gitignore::Gitignore>>,
    thread_paths: Vec<PathBuf>,
    walk_error: Arc<Mutex<Option<crate::Error>>>,
    consolidated_paths: Arc<Mutex<Vec<PathBuf>>>,
}

impl Drop for CorpusPathCollector<'_> {
    fn drop(&mut self) {
        if self.thread_paths.is_empty() {
            return;
        }
        let mut guard = self
            .consolidated_paths
            .lock()
            .expect("consolidate corpus paths lock");
        guard.append(&mut self.thread_paths);
    }
}

impl ParallelVisitor for CorpusPathCollector<'_> {
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
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            return WalkState::Continue;
        }
        let path = entry.path();
        let rel = path.strip_prefix(&self.root).unwrap_or(path).to_path_buf();
        if self.accepts_relative_path(&rel) {
            self.thread_paths.push(rel);
        }
        WalkState::Continue
    }
}

impl CorpusPathCollector<'_> {
    fn accepts_relative_path(&self, rel: &Path) -> bool {
        if self
            .exclude_paths
            .iter()
            .any(|excluded| rel.starts_with(excluded))
        {
            return false;
        }
        if !self.include_paths.is_empty()
            && !self
                .include_paths
                .iter()
                .any(|included| rel == *included || rel.starts_with(included))
        {
            return false;
        }
        if let Some(ref matcher) = self.gitignore {
            let rel_normalized = rel.to_string_lossy().replace('\\', "/");
            if matcher
                .matched(Path::new(&rel_normalized), false)
                .is_ignore()
            {
                return false;
            }
        }
        true
    }
}

struct CorpusPathCollectorBuilder<'a> {
    root: PathBuf,
    exclude_paths: &'a [PathBuf],
    include_paths: &'a [PathBuf],
    gitignore: Option<Arc<ignore::gitignore::Gitignore>>,
    walk_error: Arc<Mutex<Option<crate::Error>>>,
    consolidated_paths: Arc<Mutex<Vec<PathBuf>>>,
}

impl<'a> ParallelVisitorBuilder<'a> for CorpusPathCollectorBuilder<'a> {
    fn build(&mut self) -> Box<dyn ParallelVisitor + 'a> {
        Box::new(CorpusPathCollector {
            root: self.root.clone(),
            exclude_paths: self.exclude_paths,
            include_paths: self.include_paths,
            gitignore: self.gitignore.clone(),
            thread_paths: Vec::new(),
            walk_error: Arc::clone(&self.walk_error),
            consolidated_paths: Arc::clone(&self.consolidated_paths),
        })
    }
}

/// Walks the corpus directory and collects sorted relative file paths.
struct CorpusWalker<'a> {
    config: &'a IndexBuildConfig<'a>,
}

impl<'a> CorpusWalker<'a> {
    const fn new(config: &'a IndexBuildConfig<'a>) -> Self {
        Self { config }
    }

    /// Parallel directory walk with `WalkBuilder` ignore readers disabled.
    ///
    /// Pattern rules (`.gitignore`, `.ignore`, exclude, global) come only from
    /// [`Self::corpus_matcher`] and are applied in [`CorpusPathCollector::accepts_relative_path`],
    /// matching [`crate::CandidateFilter`]. `WalkBuilder`'s `ignore`/`git_*` flags must stay off
    /// or rules would be applied twice with different semantics.
    fn walk_builder(&self) -> WalkBuilder {
        let mut wb = WalkBuilder::new(self.config.root);
        wb.follow_links(self.config.follow_links)
            .hidden(matches!(self.config.visibility.hidden, HiddenMode::Respect))
            .parents(
                self.config
                    .visibility
                    .ignore
                    .sources
                    .contains(IgnoreSources::PARENT),
            )
            .ignore(false)
            .git_ignore(false)
            .git_exclude(false)
            .git_global(false)
            .require_git(false);
        wb
    }

    fn corpus_matcher(&self) -> crate::Result<Option<Arc<ignore::gitignore::Gitignore>>> {
        if self.config.visibility.ignore.sources.is_empty()
            && self.config.visibility.ignore.custom_files.is_empty()
        {
            return Ok(None);
        }
        build_gitignore_matcher(self.config.root, &self.config.visibility.ignore)
            .map_err(|e| crate::Error::Search(e.into()))
            .map(|matcher| matcher.map(Arc::new))
    }

    fn collect(&self) -> crate::Result<Vec<PathBuf>> {
        let walk_error = Arc::new(Mutex::new(None));
        let consolidated_paths = Arc::new(Mutex::new(Vec::new()));

        let gitignore = self.corpus_matcher()?;

        let mut builder = CorpusPathCollectorBuilder {
            root: self.config.root.to_path_buf(),
            exclude_paths: self.config.exclude_paths,
            include_paths: self.config.include_paths,
            gitignore,
            walk_error: Arc::clone(&walk_error),
            consolidated_paths: Arc::clone(&consolidated_paths),
        };

        self.walk_builder().build_parallel().visit(&mut builder);

        {
            let mut err_slot = walk_error.lock().expect("walk error lock");
            if let Some(err) = err_slot.take() {
                return Err(err);
            }
        }

        let merged_paths = {
            let mut guard = consolidated_paths.lock().expect("paths lock");
            guard.sort_unstable();
            std::mem::take(&mut *guard)
        };
        Ok(merged_paths)
    }
}

/// Stats each file to produce fingerprints with mtime and size.
struct FingerprintCollector<'a> {
    root: &'a Path,
    paths: &'a [PathBuf],
}

impl<'a> FingerprintCollector<'a> {
    const fn new(root: &'a Path, paths: &'a [PathBuf]) -> Self {
        Self { root, paths }
    }

    fn collect(&self) -> crate::Result<Vec<FileFingerprint>> {
        self.paths
            .par_iter()
            .map(|rel| {
                let abs = self.root.join(rel);
                let meta = std::fs::metadata(&abs)?;
                let mtime_secs = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));
                let size = meta.len();
                Ok(FileFingerprint {
                    path: rel.clone(),
                    mtime_secs,
                    size,
                })
            })
            .collect()
    }
}

/// Reusable bitset deduper for per-file unique trigram extraction.
struct TrigramDeduper {
    seen: Box<[u64]>,
    touched: Vec<Trigram>,
}

const SEEN_WORDS: usize = 262_144;

impl TrigramDeduper {
    fn new() -> Self {
        Self {
            seen: vec![0; SEEN_WORDS].into_boxed_slice(),
            touched: Vec::new(),
        }
    }

    fn reset(&mut self) {
        for tri in self.touched.drain(..) {
            let key = tri.as_u24() as usize;
            let word = key >> 6;
            let bit = 1u64 << (key & 63);
            self.seen[word] &= !bit;
        }
    }

    fn mark(&mut self, tri: Trigram) -> bool {
        let key = tri.as_u24() as usize;
        let word = key >> 6;
        let bit = 1u64 << (key & 63);
        let slot = &mut self.seen[word];
        if *slot & bit != 0 {
            return false;
        }
        *slot |= bit;
        self.touched.push(tri);
        true
    }

    /// Collect unique trigrams from `bytes`, returning a sorted deduplicated vec.
    fn collect_unique(&mut self, bytes: &[u8]) -> Vec<Trigram> {
        self.reset();
        if bytes.len() >= 3 {
            for i in 0..=bytes.len() - 3 {
                let tri = Trigram::from_bytes([bytes[i], bytes[i + 1], bytes[i + 2]]);
                let _ = self.mark(tri);
            }
        }
        self.touched.sort_unstable();
        let result = std::mem::take(&mut self.touched);
        for tri in &result {
            let key = tri.as_u24() as usize;
            let word = key >> 6;
            let bit = 1u64 << (key & 63);
            self.seen[word] &= !bit;
        }
        result
    }
}

/// Extracts unique trigrams per file.
struct TrigramExtractor<'a> {
    root: &'a Path,
    fingerprints: &'a [FileFingerprint],
}

impl<'a> TrigramExtractor<'a> {
    const fn new(root: &'a Path, fingerprints: &'a [FileFingerprint]) -> Self {
        Self { root, fingerprints }
    }

    fn extract(&self) -> crate::Result<Vec<TrigramSet>> {
        self.fingerprints
            .par_iter()
            .map_init(TrigramDeduper::new, |deduper, fp| {
                let abs = self.root.join(&fp.path);
                let mmap = open_mmap(&abs).map_err(crate::Error::Io)?;
                let unique = deduper.collect_unique(mmap.as_ref());
                TrigramSet::new(unique).map_err(|_| {
                    crate::Error::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "trigram set rejected unsorted/duplicate trigrams",
                    ))
                })
            })
            .collect()
    }
}

/// Assembles trigram → file ID posting lists from per-file trigram sets.
struct PostingAssembler<'a> {
    file_trigrams: &'a [TrigramSet],
}

impl<'a> PostingAssembler<'a> {
    const fn new(file_trigrams: &'a [TrigramSet]) -> Self {
        Self { file_trigrams }
    }

    fn assemble(&self) -> crate::Result<(Vec<LexiconEntry>, Vec<u8>)> {
        let mut pairs: Vec<(Trigram, u32)> = Vec::new();
        for (id, set) in self.file_trigrams.iter().enumerate() {
            let id_u32: u32 = id.try_into().map_err(|_| {
                crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "too many indexed files",
                ))
            })?;
            for tri in set.as_slice() {
                pairs.push((*tri, id_u32));
            }
        }
        pairs.sort_unstable_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));

        let mut posting_bytes: Vec<u8> = Vec::new();
        let mut lex_entries: Vec<LexiconEntry> = Vec::new();
        let mut i = 0;
        while i < pairs.len() {
            let tri = pairs[i].0;
            let offset: u64 = posting_bytes.len().try_into().map_err(|_| {
                crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "postings offset overflow",
                ))
            })?;
            let mut ids = Vec::new();
            while i < pairs.len() && pairs[i].0 == tri {
                ids.push(pairs[i].1);
                i += 1;
            }
            let len: u32 = ids.len().try_into().map_err(|_| {
                crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "posting list too long",
                ))
            })?;
            super::storage::postings::Postings::encode_sorted(&mut posting_bytes, &ids)
                .map_err(crate::Error::Io)?;
            lex_entries.push(LexiconEntry {
                trigram: tri.to_bytes(),
                offset,
                len,
            });
        }

        Ok((lex_entries, posting_bytes))
    }
}

/// Fluent builder for standalone trigram index construction.
///
/// Used by `sift build` and tests. For snapshot-managed builds, use
/// `Index::build` via `IndexStore`.
pub struct TrigramIndexBuilder<'a> {
    root: &'a Path,
    dir: Option<PathBuf>,
    follow_links: bool,
}

impl<'a> TrigramIndexBuilder<'a> {
    #[must_use]
    pub const fn new(root: &'a Path) -> Self {
        Self {
            root,
            dir: None,
            follow_links: false,
        }
    }

    #[must_use]
    pub fn with_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.dir = Some(dir.into());
        self
    }

    #[must_use]
    pub const fn with_follow_links(mut self, follow_links: bool) -> Self {
        self.follow_links = follow_links;
        self
    }

    fn excluded_build_paths(&self, root: &Path) -> crate::Result<Vec<PathBuf>> {
        let Some(dir) = self.dir.as_ref() else {
            return Ok(Vec::new());
        };
        let abs = if dir.is_absolute() {
            dir.clone()
        } else {
            std::env::current_dir()?.join(dir)
        };
        let abs = abs.canonicalize().unwrap_or(abs);
        if abs.starts_with(root) {
            Ok(vec![
                abs.strip_prefix(root)
                    .expect("prefix checked")
                    .to_path_buf(),
            ])
        } else {
            Ok(Vec::new())
        }
    }

    /// Walk `root`, extract trigrams, and write the index to the configured output directory.
    ///
    /// Returns an mmap-backed [`TrigramIndex`] over the written files.
    ///
    /// # Errors
    ///
    /// Propagates IO errors from walking, reading files, or writing persistence files.
    /// Returns an error if `with_dir` was not called before `build`.
    pub fn build(self) -> crate::Result<super::TrigramIndex> {
        let canonical = self.root.canonicalize()?;

        let (root, include_paths, kind) = if canonical.is_file() {
            let parent = canonical
                .parent()
                .ok_or_else(|| {
                    crate::Error::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "corpus root must have a parent directory",
                    ))
                })?
                .to_path_buf();
            let file_name = PathBuf::from(canonical.file_name().ok_or_else(|| {
                crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "single-file corpus must have a file name",
                ))
            })?);
            (parent, vec![file_name], CorpusKind::SingleFile)
        } else {
            (canonical, Vec::new(), CorpusKind::Directory)
        };

        let exclude_paths = self.excluded_build_paths(&root)?;
        let config = IndexBuildConfig {
            root: &root,
            follow_links: self.follow_links,
            exclude_paths: &exclude_paths,
            include_paths: &include_paths,
            corpus_kind: kind,
            visibility: VisibilityConfig::default(),
        };

        let dir = self.dir.ok_or_else::<crate::Error, _>(|| {
            crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "output directory is required; use .with_dir() before .build()",
            ))
        })?;
        let tables = IndexTableBuilder::new(&config).build()?;
        super::TrigramIndex::create_in_dir(&tables, &root, kind, &dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::filter::IgnoreConfig;
    use crate::{CandidateFilter, CandidateFilterConfig, VisibilityConfig};
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    struct TablesFixture;

    impl TablesFixture {
        fn no_ignore_config(root: &Path) -> IndexBuildConfig<'_> {
            IndexBuildConfig {
                root,
                follow_links: false,
                exclude_paths: &[],
                include_paths: &[],
                corpus_kind: CorpusKind::Directory,
                visibility: VisibilityConfig {
                    ignore: IgnoreConfig::disabled(),
                    ..Default::default()
                },
            }
        }

        fn build(root: &Path) -> IndexTables {
            IndexTableBuilder::new(&Self::no_ignore_config(root))
                .build()
                .expect("build tables")
        }
    }

    struct PostingCounts;

    impl PostingCounts {
        fn file_occurrences(postings: &[u8], lexicon: &[LexiconEntry], file_id: u32) -> usize {
            lexicon
                .iter()
                .map(|entry| {
                    let start = usize::try_from(entry.offset).unwrap_or(usize::MAX);
                    let end = lexicon
                        .iter()
                        .find(|e| e.offset > entry.offset)
                        .map_or(postings.len(), |e| {
                            usize::try_from(e.offset).unwrap_or(usize::MAX)
                        });
                    let slice = &postings[start..end];
                    crate::index::trigram::storage::postings::Postings::decode_sorted(slice)
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|&id| id == file_id)
                        .count()
                })
                .sum()
        }
    }

    struct FilterCorpus;

    impl FilterCorpus {
        fn write(root: &Path) {
            fs::create_dir_all(root.join("skip")).expect("mkdir skip");
            fs::create_dir_all(root.join("also_skip")).expect("mkdir also_skip");
            fs::write(root.join("keep.txt"), "beta\n").expect("write keep");
            fs::write(root.join("skip/ignored.txt"), "beta\n").expect("write skip");
            fs::write(root.join("also_skip/omit.txt"), "beta\n").expect("write omit");
            fs::write(root.join(".gitignore"), "skip/**\n").expect("write gitignore");
            fs::write(root.join(".ignore"), "also_skip/**\n").expect("write ignore");
        }
    }

    struct FilterParity;

    impl FilterParity {
        fn filter_config(build: &IndexBuildConfig<'_>) -> CandidateFilterConfig {
            CandidateFilterConfig {
                exclude_paths: build.exclude_paths.to_vec(),
                visibility: build.visibility.clone(),
                follow_links: build.follow_links,
                ..CandidateFilterConfig::default()
            }
        }

        fn all_corpus_files(root: &Path) -> Vec<PathBuf> {
            let mut files = Vec::new();
            Self::collect_files_recursive(root, root, &mut files);
            files.sort_unstable();
            files
        }

        fn collect_files_recursive(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
            let entries = fs::read_dir(dir).expect("read dir");
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    Self::collect_files_recursive(root, &path, out);
                } else if path.is_file() {
                    let rel = path.strip_prefix(root).expect("under root").to_path_buf();
                    out.push(rel);
                }
            }
        }
    }

    #[test]
    fn build_index_tables_sorts_file_paths_deterministically() {
        let tmp = TempDir::new().expect("create temp dir");
        fs::write(tmp.path().join("z.txt"), "hello\n").expect("write z");
        fs::write(tmp.path().join("a.txt"), "world\n").expect("write a");
        fs::write(tmp.path().join("m.txt"), "test\n").expect("write m");

        let tables = TablesFixture::build(tmp.path());
        let paths: Vec<PathBuf> = tables
            .fingerprints
            .iter()
            .map(|fp| fp.path.clone())
            .collect();
        let expected = vec![
            PathBuf::from("a.txt"),
            PathBuf::from("m.txt"),
            PathBuf::from("z.txt"),
        ];
        assert_eq!(paths, expected);
    }

    #[test]
    fn build_index_tables_exclude_paths_prevents_indexing() {
        let tmp = TempDir::new().expect("create temp dir");
        fs::write(tmp.path().join("keep.txt"), "hello\n").expect("write keep");
        let excluded_dir = tmp.path().join("excluded");
        fs::create_dir_all(&excluded_dir).expect("create excluded dir");
        fs::write(excluded_dir.join("skip.txt"), "world\n").expect("write skip");

        let config = IndexBuildConfig {
            root: tmp.path(),
            follow_links: false,
            exclude_paths: &[PathBuf::from("excluded")],
            include_paths: &[],
            corpus_kind: CorpusKind::Directory,
            visibility: VisibilityConfig {
                ignore: IgnoreConfig::disabled(),
                ..Default::default()
            },
        };
        let tables = IndexTableBuilder::new(&config)
            .build()
            .expect("build tables");
        assert_eq!(tables.fingerprints.len(), 1);
        assert_eq!(tables.fingerprints[0].path, PathBuf::from("keep.txt"));
    }

    #[test]
    fn build_index_tables_duplicate_trigrams_deduplicated_in_postings() {
        let tmp = TempDir::new().expect("create temp dir");
        fs::write(tmp.path().join("a.txt"), "ababab\n").expect("write file");
        fs::write(tmp.path().join("b.txt"), "ababab\n").expect("write file");

        let tables = TablesFixture::build(tmp.path());
        assert_eq!(tables.fingerprints.len(), 2);

        let unique_trigrams = 3;
        assert_eq!(
            PostingCounts::file_occurrences(&tables.postings, &tables.lexicon, 0),
            unique_trigrams
        );
        assert_eq!(
            PostingCounts::file_occurrences(&tables.postings, &tables.lexicon, 1),
            unique_trigrams
        );
    }

    #[test]
    fn corpus_walk_excludes_gitignored_paths_without_git_repo() {
        let tmp = TempDir::new().expect("create temp dir");
        FilterCorpus::write(tmp.path());

        let config = IndexBuildConfig {
            root: tmp.path(),
            follow_links: false,
            exclude_paths: &[],
            include_paths: &[],
            corpus_kind: CorpusKind::Directory,
            visibility: VisibilityConfig::default(),
        };
        let paths = CorpusWalker::new(&config).collect().expect("walk corpus");
        assert!(paths.iter().any(|p| p == Path::new("keep.txt")));
        assert!(!paths.iter().any(|p| p.starts_with("skip")));
        assert!(!paths.iter().any(|p| p.starts_with("also_skip")));
    }

    #[test]
    fn corpus_walk_with_empty_ignore_sources_includes_gitignored() {
        let tmp = TempDir::new().expect("create temp dir");
        FilterCorpus::write(tmp.path());

        let config = IndexBuildConfig {
            root: tmp.path(),
            follow_links: false,
            exclude_paths: &[],
            include_paths: &[],
            corpus_kind: CorpusKind::Directory,
            visibility: VisibilityConfig {
                ignore: IgnoreConfig::disabled(),
                ..Default::default()
            },
        };
        let paths = CorpusWalker::new(&config).collect().expect("walk corpus");
        assert!(paths.iter().any(|p| p.starts_with("skip")));
        assert!(paths.iter().any(|p| p.starts_with("also_skip")));
    }

    #[test]
    fn corpus_walk_agrees_with_candidate_filter() {
        let tmp = TempDir::new().expect("create temp dir");
        FilterCorpus::write(tmp.path());

        let config = IndexBuildConfig {
            root: tmp.path(),
            follow_links: false,
            exclude_paths: &[],
            include_paths: &[],
            corpus_kind: CorpusKind::Directory,
            visibility: VisibilityConfig::default(),
        };
        let indexed = CorpusWalker::new(&config).collect().expect("walk corpus");
        let filter = CandidateFilter::new(&FilterParity::filter_config(&config), tmp.path())
            .expect("filter");

        for rel in FilterParity::all_corpus_files(tmp.path()) {
            let should_index = filter.matches_path(&rel);
            let is_indexed = indexed.iter().any(|p| p == &rel);
            assert_eq!(
                is_indexed, should_index,
                "path {rel:?}: indexed={is_indexed} filter={should_index}"
            );
        }
    }

    #[test]
    fn build_index_tables_include_paths_filters_to_single_file() {
        let tmp = TempDir::new().expect("create temp dir");
        fs::write(tmp.path().join("keep.txt"), "hello\n").expect("write keep");
        fs::write(tmp.path().join("skip.txt"), "world\n").expect("write skip");

        let config = IndexBuildConfig {
            root: tmp.path(),
            follow_links: false,
            exclude_paths: &[],
            include_paths: &[PathBuf::from("keep.txt")],
            corpus_kind: CorpusKind::Directory,
            visibility: VisibilityConfig {
                ignore: IgnoreConfig::disabled(),
                ..Default::default()
            },
        };
        let tables = IndexTableBuilder::new(&config)
            .build()
            .expect("build tables");
        assert_eq!(tables.fingerprints.len(), 1);
        assert_eq!(tables.fingerprints[0].path, PathBuf::from("keep.txt"));
    }

    #[test]
    fn build_single_file_corpus_ignores_siblings() {
        let tmp = TempDir::new().expect("create temp dir");
        let file = tmp.path().join("only.txt");
        fs::write(&file, "needle\n").expect("write file");
        fs::write(tmp.path().join("other.txt"), "haystack\n").expect("write other");

        let index = TrigramIndexBuilder::new(&file)
            .with_dir(tmp.path().join(".sift"))
            .build()
            .expect("build index");
        assert_eq!(index.corpus_kind(), CorpusKind::SingleFile);
        assert_eq!(index.fingerprints.len(), 1);
        assert_eq!(
            index.fingerprints[0].path,
            PathBuf::from("only.txt"),
            "should only index the specified file, not siblings"
        );
    }

    #[test]
    fn fingerprints_have_nonzero_size() {
        let tmp = TempDir::new().expect("create temp dir");
        fs::write(tmp.path().join("a.txt"), "hello world\n").expect("write file");

        let tables = TablesFixture::build(tmp.path());
        assert_eq!(tables.fingerprints.len(), 1);
        assert!(
            tables.fingerprints[0].size > 0,
            "fingerprint should capture file size"
        );
    }

    #[test]
    fn build_respects_gitignore_by_default() {
        let tmp = TempDir::new().expect("create temp dir");
        fs::write(tmp.path().join(".gitignore"), "*.ignored\n").expect("write gitignore");
        fs::write(tmp.path().join("keep.txt"), "hello\n").expect("write keep");
        fs::write(tmp.path().join("skip.ignored"), "secret\n").expect("write ignored");

        let config = IndexBuildConfig {
            root: tmp.path(),
            follow_links: false,
            exclude_paths: &[],
            include_paths: &[],
            corpus_kind: CorpusKind::Directory,
            visibility: VisibilityConfig::default(),
        };
        let tables = IndexTableBuilder::new(&config)
            .build()
            .expect("build tables");
        let paths: Vec<_> = tables.fingerprints.iter().map(|f| f.path.clone()).collect();
        assert!(
            !paths.iter().any(|p| p == "skip.ignored"),
            "gitignored file must not be indexed, got {paths:?}"
        );
        assert!(
            paths.iter().any(|p| p == "keep.txt"),
            "keep.txt must be indexed, got {paths:?}"
        );
    }

    mod deduper {
        use super::*;

        #[test]
        fn sorts_and_deduplicates() {
            let tris = TrigramDeduper::new().collect_unique(b"ababa");
            assert_eq!(tris.len(), 2);
            assert!(tris.contains(&Trigram::from_bytes(*b"aba")));
            assert!(tris.contains(&Trigram::from_bytes(*b"bab")));
        }

        #[test]
        fn short_input_returns_empty() {
            let mut deduper = TrigramDeduper::new();
            assert!(deduper.collect_unique(b"").is_empty());
            assert!(deduper.collect_unique(b"ab").is_empty());
        }

        #[test]
        fn matches_raw_windows_valid_ascii() {
            let b = b"hello world";
            let unique: Vec<[u8; 3]> = TrigramDeduper::new()
                .collect_unique(b)
                .into_iter()
                .map(Trigram::to_bytes)
                .collect();
            let mut ref_set: Vec<[u8; 3]> = Trigram::windows(b).map(Trigram::to_bytes).collect();
            ref_set.sort_unstable();
            ref_set.dedup();
            assert_eq!(unique, ref_set);
        }

        #[test]
        fn matches_raw_windows_multibyte() {
            let b = "café résumé 日本語".as_bytes();
            let unique: Vec<[u8; 3]> = TrigramDeduper::new()
                .collect_unique(b)
                .into_iter()
                .map(Trigram::to_bytes)
                .collect();
            let mut ref_set: Vec<[u8; 3]> = Trigram::windows(b).map(Trigram::to_bytes).collect();
            ref_set.sort_unstable();
            ref_set.dedup();
            assert_eq!(unique, ref_set);
        }

        #[test]
        fn uses_raw_windows_for_invalid_utf8() {
            let b: Vec<u8> = [b"ok", &[0xff, 0xfe][..], b" trail"].concat();
            let unique: Vec<[u8; 3]> = TrigramDeduper::new()
                .collect_unique(&b)
                .into_iter()
                .map(Trigram::to_bytes)
                .collect();
            let mut ref_set: Vec<[u8; 3]> = Trigram::windows(&b).map(Trigram::to_bytes).collect();
            ref_set.sort_unstable();
            ref_set.dedup();
            assert_eq!(unique, ref_set);
        }

        #[test]
        fn does_not_allocate_lossy_replacement_trigrams() {
            let b = &[0xff, 0xfe, 0xfd];
            let unique = TrigramDeduper::new().collect_unique(b);
            assert_eq!(unique.len(), 1);
            assert_eq!(unique[0].to_bytes(), *b);
        }

        #[test]
        fn reused_deduper_does_not_drop_overlapping_trigrams() {
            let mut deduper = TrigramDeduper::new();
            let first = deduper.collect_unique(b"abcxxx");
            assert!(first.contains(&Trigram::from_bytes(*b"abc")));

            let second = deduper.collect_unique(b"abczzz");
            assert!(
                second.contains(&Trigram::from_bytes(*b"abc")),
                "abc should appear in second call; got {second:?}"
            );
        }

        #[test]
        fn reused_deduper_three_times_no_loss() {
            let mut deduper = TrigramDeduper::new();
            let _ = deduper.collect_unique(b"aaaaaa");
            let _ = deduper.collect_unique(b"bbbbbb");
            let third = deduper.collect_unique(b"abcabc");
            assert!(
                third.contains(&Trigram::from_bytes(*b"abc")),
                "abc must be present"
            );
        }

        #[test]
        fn reused_deduper_preserves_independence() {
            let mut deduper = TrigramDeduper::new();
            let a = deduper.collect_unique(b"aaa");
            let b = deduper.collect_unique(b"bbb");
            let c = deduper.collect_unique(b"ccc");
            assert_eq!(a.len(), 1);
            assert_eq!(b.len(), 1);
            assert_eq!(c.len(), 1);
        }
    }
}

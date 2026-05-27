//! Walk corpus, extract trigrams, build in-memory index tables.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ignore::{
    DirEntry, Error as IgnoreError, ParallelVisitor, ParallelVisitorBuilder, WalkBuilder, WalkState,
};
use rayon::prelude::*;

use super::file_table::FileFingerprint;
use super::storage::lexicon::LexiconEntry;
use super::storage::mmap::open_mmap;
use super::storage::varint;
use super::types::{Trigram, TrigramDeduper};

use crate::index::{CorpusKind, IndexBuildConfig};
use crate::search::filter::{HiddenMode, IgnoreSources, VisibilityConfig};
use crate::search::filter::ignore::build_gitignore_matcher;

/// Collected index data ready for persistence.
pub struct IndexTables {
    pub fingerprints: Vec<FileFingerprint>,
    pub file_trigrams: Vec<Vec<Trigram>>,
    pub lexicon: Vec<LexiconEntry>,
    pub postings: Vec<u8>,
}

/// Walks a corpus and builds index tables, optionally reusing cached data
/// from a previous index for files that have not changed.
pub struct IndexTableBuilder<'a> {
    config: &'a IndexBuildConfig<'a>,
    prev_fingerprints: Option<&'a [FileFingerprint]>,
    prev_trigrams: Option<&'a [Vec<Trigram>]>,
}

impl<'a> IndexTableBuilder<'a> {
    #[must_use]
    pub const fn new(config: &'a IndexBuildConfig<'a>) -> Self {
        Self {
            config,
            prev_fingerprints: None,
            prev_trigrams: None,
        }
    }

    #[must_use]
    pub const fn with_previous(
        mut self,
        fingerprints: &'a [FileFingerprint],
        trigrams: &'a [Vec<Trigram>],
    ) -> Self {
        self.prev_fingerprints = Some(fingerprints);
        self.prev_trigrams = Some(trigrams);
        self
    }

    /// Build the index tables.
    ///
    /// When previous data is provided via [`with_previous`](Self::with_previous),
    /// files whose fingerprint matches the previous index skip trigram
    /// extraction entirely.
    ///
    /// # Errors
    ///
    /// Returns an error if corpus walking, file I/O, or trigram extraction fails.
    pub fn build(self) -> crate::Result<IndexTables> {
        let paths = CorpusWalker::new(self.config).collect()?;
        let fingerprints = FingerprintCollector::new(self.config.root, &paths).collect()?;

        let extractor = TrigramExtractor::new(
            self.config.root,
            &fingerprints,
            self.prev_fingerprints,
            self.prev_trigrams,
        );
        let file_trigrams = extractor.extract()?;

        let (lexicon, postings) = PostingAssembler::new(&file_trigrams).assemble()?;

        Ok(IndexTables {
            fingerprints,
            file_trigrams,
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
            .hidden(matches!(
                self.config.visibility.hidden,
                HiddenMode::Respect
            ))
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

/// Extracts unique trigrams per file, reusing cached trigrams for unchanged files.
struct TrigramExtractor<'a> {
    root: &'a Path,
    fingerprints: &'a [FileFingerprint],
    prev_fingerprints: Option<&'a [FileFingerprint]>,
    prev_trigrams: Option<&'a [Vec<Trigram>]>,
}

impl<'a> TrigramExtractor<'a> {
    const fn new(
        root: &'a Path,
        fingerprints: &'a [FileFingerprint],
        prev_fingerprints: Option<&'a [FileFingerprint]>,
        prev_trigrams: Option<&'a [Vec<Trigram>]>,
    ) -> Self {
        Self {
            root,
            fingerprints,
            prev_fingerprints,
            prev_trigrams,
        }
    }

    fn extract(&self) -> crate::Result<Vec<Vec<Trigram>>> {
        let cache = self.build_lookup();
        self.fingerprints
            .par_iter()
            .map_init(TrigramDeduper::new, |deduper, fp| {
                if let Some(tris) = cache.get(&(fp.path.as_path(), fp.mtime_secs, fp.size)) {
                    return Ok(tris.to_vec());
                }
                let abs = self.root.join(&fp.path);
                let mmap = open_mmap(&abs).map_err(crate::Error::Io)?;
                Ok(deduper.collect_unique(mmap.as_ref()))
            })
            .collect()
    }

    fn build_lookup(&self) -> std::collections::HashMap<(&Path, i64, u64), &[Trigram]> {
        let (Some(prev_fps), Some(prev_tris)) = (self.prev_fingerprints, self.prev_trigrams) else {
            return std::collections::HashMap::new();
        };
        if prev_fps.len() != prev_tris.len() {
            return std::collections::HashMap::new();
        }
        prev_fps
            .iter()
            .zip(prev_tris.iter())
            .map(|(fp, tris)| ((fp.path.as_path(), fp.mtime_secs, fp.size), tris.as_slice()))
            .collect()
    }
}

/// Assembles trigram → file ID posting lists from per-file trigram sets.
struct PostingAssembler<'a> {
    file_trigrams: &'a [Vec<Trigram>],
}

impl<'a> PostingAssembler<'a> {
    const fn new(file_trigrams: &'a [Vec<Trigram>]) -> Self {
        Self { file_trigrams }
    }

    fn assemble(&self) -> crate::Result<(Vec<LexiconEntry>, Vec<u8>)> {
        let mut pairs: Vec<(Trigram, u32)> = Vec::new();
        for (id, tris) in self.file_trigrams.iter().enumerate() {
            let id_u32: u32 = id.try_into().map_err(|_| {
                crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "too many indexed files",
                ))
            })?;
            for tri in tris {
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
            varint::encode_sorted_deltas(&mut posting_bytes, &ids);
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

    /// Walk `root`, extract trigrams, and return an in-memory [`TrigramIndex`].
    ///
    /// # Errors
    ///
    /// Propagates IO errors from walking, reading files, or writing persistence files
    /// (if `with_dir` was called).
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
            visibility: VisibilityConfig::standard(),
        };

        let tables = IndexTableBuilder::new(&config).build()?;
        let index = super::TrigramIndex::from_tables(tables, root, kind);

        if let Some(dir) = self.dir {
            index.save_to_dir(&dir)?;
        }
        Ok(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
                visibility: VisibilityConfig::ignores_disabled(),
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
                    varint::decode_sorted_deltas::<u32>(slice)
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
            visibility: VisibilityConfig::ignores_disabled(),
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
            visibility: VisibilityConfig::standard(),
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
            visibility: VisibilityConfig::ignores_disabled(),
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
            visibility: VisibilityConfig::standard(),
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
    fn trigram_extraction_returns_sorted_deduped_keys() {
        let tmp = TempDir::new().expect("create temp dir");
        fs::write(tmp.path().join("test.txt"), "ababab\n").expect("write file");

        let tables = TablesFixture::build(tmp.path());
        assert_eq!(tables.file_trigrams.len(), 1);
        let tris = &tables.file_trigrams[0];
        assert_eq!(tris.len(), 3);
        for i in 0..tris.len() - 1 {
            assert!(tris[i] <= tris[i + 1]);
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
            visibility: VisibilityConfig::ignores_disabled(),
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
            visibility: VisibilityConfig::standard(),
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

    #[test]
    fn incremental_build_reuses_cached_trigrams() {
        let tmp = TempDir::new().expect("create temp dir");
        fs::write(tmp.path().join("a.txt"), "hello\n").expect("write a");
        fs::write(tmp.path().join("b.txt"), "world\n").expect("write b");

        let config = IndexBuildConfig {
            root: tmp.path(),
            follow_links: false,
            exclude_paths: &[],
            include_paths: &[],
            corpus_kind: CorpusKind::Directory,
            visibility: VisibilityConfig::ignores_disabled(),
        };

        let tables1 = IndexTableBuilder::new(&config)
            .build()
            .expect("first build");

        let tables2 = IndexTableBuilder::new(&config)
            .with_previous(&tables1.fingerprints, &tables1.file_trigrams)
            .build()
            .expect("incremental build");

        assert_eq!(tables1.file_trigrams, tables2.file_trigrams);
        assert_eq!(tables1.lexicon, tables2.lexicon);
        assert_eq!(tables1.postings, tables2.postings);
    }
}

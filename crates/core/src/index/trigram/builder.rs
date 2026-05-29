//! Corpus walk, file stat, and posting assembly helpers.
//!
//! These are private helpers used by [`TrigramIndex::build`] and
//! [`TrigramIndex::update`] — there is no separate builder type.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ignore::{
    DirEntry, Error as IgnoreError, ParallelVisitor, ParallelVisitorBuilder, WalkBuilder, WalkState,
};

use super::Trigram;
use super::file_table::FileFingerprint;
use super::storage::lexicon::LexiconEntry;
use super::storage::trigram_sets::TrigramSet;

use crate::index::{CorpusKind, IndexConfig};
use crate::search::filter::ignore::build_gitignore_matcher;
use crate::search::filter::{HiddenMode, IgnoreSources};

/// Collected index data ready for persistence.
pub struct IndexTables {
    pub fingerprints: Vec<FileFingerprint>,
    pub file_trigrams: Vec<TrigramSet>,
    pub lexicon: Vec<LexiconEntry>,
    pub postings: Vec<u8>,
}

// ---------------------------------------------------------------------------
// Corpus walk
// ---------------------------------------------------------------------------

/// Per-thread collector for [`WalkParallel::visit`].
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
pub struct CorpusWalker<'a> {
    config: &'a IndexConfig<'a>,
}

impl<'a> CorpusWalker<'a> {
    pub const fn new(config: &'a IndexConfig<'a>) -> Self {
        Self { config }
    }

    fn walk_builder(&self) -> WalkBuilder {
        let mut wb = WalkBuilder::new(self.config.corpus.root);
        wb.follow_links(self.config.corpus.follow_links)
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
        build_gitignore_matcher(self.config.corpus.root, &self.config.visibility.ignore)
            .map_err(|e| crate::Error::Search(e.into()))
            .map(|matcher| matcher.map(Arc::new))
    }

    pub fn collect(&self) -> crate::Result<Vec<PathBuf>> {
        let walk_error = Arc::new(Mutex::new(None));
        let consolidated_paths = Arc::new(Mutex::new(Vec::new()));

        let gitignore = self.corpus_matcher()?;

        let mut builder = CorpusPathCollectorBuilder {
            root: self.config.corpus.root.to_path_buf(),
            exclude_paths: self.config.corpus.exclude_paths,
            include_paths: self.config.corpus.include_paths,
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

// ---------------------------------------------------------------------------
// Fingerprint collection
// ---------------------------------------------------------------------------

/// Stats each file to produce fingerprints with mtime and size.
pub struct FingerprintCollector<'a> {
    root: &'a Path,
    paths: &'a [PathBuf],
}

impl<'a> FingerprintCollector<'a> {
    pub const fn new(root: &'a Path, paths: &'a [PathBuf]) -> Self {
        Self { root, paths }
    }

    pub fn collect(&self) -> crate::Result<Vec<FileFingerprint>> {
        use rayon::prelude::*;
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

// ---------------------------------------------------------------------------
// Posting assembly
// ---------------------------------------------------------------------------

/// A packed (`trigram_key` << 32) | `file_id`, enabling sort-by-trigram-then-file-id on raw u64 ordering.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct PackedPosting(u64);

impl PackedPosting {
    fn new(trigram: Trigram, file_id: u32) -> Self {
        Self((u64::from(trigram.as_u24()) << 32) | u64::from(file_id))
    }

    const fn trigram_key(self) -> u32 {
        (self.0 >> 32) as u32
    }

    const fn file_id(self) -> u32 {
        (self.0 & 0xFFFF_FFFF) as u32
    }
}

/// A single contiguous posting list for one trigram, yielded by [`PostingRuns`].
struct PostingRun<'a> {
    trigram_key: u32,
    pairs: &'a [PackedPosting],
}

impl PostingRun<'_> {
    const fn trigram_bytes(&self) -> [u8; 3] {
        Trigram::from_u24(self.trigram_key).to_bytes()
    }

    const fn len(&self) -> usize {
        self.pairs.len()
    }

    fn file_ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.pairs.iter().map(|p| p.file_id())
    }
}

/// Iterator over contiguous trigram runs in sorted packed-posting order.
struct PostingRuns<'a> {
    pairs: &'a [PackedPosting],
    pos: usize,
}

impl<'a> PostingRuns<'a> {
    const fn new(pairs: &'a [PackedPosting]) -> Self {
        Self { pairs, pos: 0 }
    }
}

impl<'a> Iterator for PostingRuns<'a> {
    type Item = PostingRun<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.pairs.len() {
            return None;
        }
        let trigram_key = self.pairs[self.pos].trigram_key();
        let start = self.pos;
        while self.pos < self.pairs.len() && self.pairs[self.pos].trigram_key() == trigram_key {
            self.pos += 1;
        }
        Some(PostingRun {
            trigram_key,
            pairs: &self.pairs[start..self.pos],
        })
    }
}

/// Assembles trigram → file ID posting lists from per-file trigram sets.
pub struct PostingAssembler<'a> {
    file_trigrams: &'a [TrigramSet],
}

impl<'a> PostingAssembler<'a> {
    pub const fn new(file_trigrams: &'a [TrigramSet]) -> Self {
        Self { file_trigrams }
    }

    pub fn assemble(&self) -> crate::Result<(Vec<LexiconEntry>, Vec<u8>)> {
        let mut pairs = self.collect_pairs()?;
        Self::sort_pairs(&mut pairs);
        Self::encode_runs(&pairs)
    }

    fn collect_pairs(&self) -> crate::Result<Vec<PackedPosting>> {
        let total: usize = self.file_trigrams.iter().map(|s| s.as_slice().len()).sum();
        let mut pairs = Vec::with_capacity(total);
        for (id, set) in self.file_trigrams.iter().enumerate() {
            let id_u32: u32 = id.try_into().map_err(|_| {
                crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "too many indexed files",
                ))
            })?;
            for tri in set.as_slice() {
                pairs.push(PackedPosting::new(*tri, id_u32));
            }
        }
        Ok(pairs)
    }

    fn sort_pairs(pairs: &mut Vec<PackedPosting>) {
        let len = pairs.len();
        if len < 2 {
            return;
        }

        let mut scratch = vec![PackedPosting(0); len];
        let mut count = vec![0usize; 65_536];

        for pass in 0..4 {
            let shift = pass * 16;

            for &p in pairs.iter() {
                count[((p.0 >> shift) & 0xFFFF) as usize] += 1;
            }

            let mut total = 0usize;
            for c in &mut count {
                let tmp = *c + total;
                *c = total;
                total = tmp;
            }

            for &p in pairs.iter() {
                let byte = ((p.0 >> shift) & 0xFFFF) as usize;
                let pos = &mut count[byte];
                scratch[*pos] = p;
                *pos += 1;
            }

            std::mem::swap(pairs, &mut scratch);

            count.fill(0);
        }
    }

    fn encode_runs(pairs: &[PackedPosting]) -> crate::Result<(Vec<LexiconEntry>, Vec<u8>)> {
        let mut posting_bytes = Vec::with_capacity(pairs.len() * 3);
        let mut lex_entries = Vec::new();

        for run in PostingRuns::new(pairs) {
            let offset: u64 = posting_bytes.len().try_into().map_err(|_| {
                crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "postings offset overflow",
                ))
            })?;

            let len = u32::try_from(run.len()).map_err(|_| {
                crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "posting list too long",
                ))
            })?;

            let tri_bytes = run.trigram_bytes();
            Self::encode_run(&run, &mut posting_bytes)?;

            lex_entries.push(LexiconEntry {
                trigram: tri_bytes,
                offset,
                len,
            });
        }

        Ok((lex_entries, posting_bytes))
    }

    fn encode_run(run: &PostingRun<'_>, out: &mut Vec<u8>) -> crate::Result<()> {
        let mut prev = 0u64;
        for (j, id) in run.file_ids().enumerate() {
            let raw = if j == 0 {
                u64::from(id)
            } else {
                u64::from(id).checked_sub(prev).ok_or_else(|| {
                    crate::Error::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "non-monotonic posting list",
                    ))
                })?
            };
            let mut buf = unsigned_varint::encode::u64_buffer();
            let encoded = unsigned_varint::encode::u64(raw, &mut buf);
            out.extend_from_slice(encoded);
            prev = u64::from(id);
        }
        Ok(())
    }
}

/// Build in-memory index tables from a corpus configuration.
///
/// Orchestrates: corpus walk → fingerprint → trigram extraction → posting assembly.
pub fn build_tables(config: &IndexConfig<'_>) -> crate::Result<IndexTables> {
    use rayon::prelude::*;

    let (paths, root) = match config.corpus.kind {
        CorpusKind::SingleFile => {
            if config.corpus.include_paths.is_empty() {
                return Err(crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "SingleFile corpus must specify the file in include_paths",
                )));
            }
            let paths = config.corpus.include_paths.to_vec();
            (paths, config.corpus.root)
        }
        CorpusKind::Directory => {
            let paths = CorpusWalker::new(config).collect()?;
            (paths, config.corpus.root)
        }
    };

    let fingerprints = FingerprintCollector::new(root, &paths).collect()?;

    let file_trigrams: Vec<TrigramSet> = fingerprints
        .par_iter()
        .map(|fp| {
            let abs = root.join(&fp.path);
            TrigramSet::from_file(&abs)
        })
        .collect::<std::io::Result<_>>()
        .map_err(crate::Error::Io)?;

    let (lexicon, postings) = PostingAssembler::new(&file_trigrams).assemble()?;

    Ok(IndexTables {
        fingerprints,
        file_trigrams,
        lexicon,
        postings,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::{CorpusKind, CorpusSpec, IndexConfig};
    use crate::search::filter::IgnoreConfig;
    use crate::{CandidateFilter, CandidateFilterConfig, VisibilityConfig};
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    struct TablesFixture;

    impl TablesFixture {
        fn no_ignore_config(root: &Path) -> IndexConfig<'_> {
            IndexConfig {
                corpus: CorpusSpec {
                    root,
                    kind: CorpusKind::Directory,
                    follow_links: false,
                    include_paths: &[],
                    exclude_paths: &[],
                },
                visibility: VisibilityConfig {
                    ignore: IgnoreConfig::disabled(),
                    ..Default::default()
                },
            }
        }

        fn build(root: &Path) -> IndexTables {
            let config = Self::no_ignore_config(root);
            build_tables(&config).expect("build tables")
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
        fn filter_config(build: &IndexConfig<'_>) -> CandidateFilterConfig {
            CandidateFilterConfig {
                exclude_paths: build.corpus.exclude_paths.to_vec(),
                visibility: build.visibility.clone(),
                follow_links: build.corpus.follow_links,
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

        let config = IndexConfig {
            corpus: CorpusSpec {
                root: tmp.path(),
                kind: CorpusKind::Directory,
                follow_links: false,
                include_paths: &[],
                exclude_paths: &[PathBuf::from("excluded")],
            },
            visibility: VisibilityConfig {
                ignore: IgnoreConfig::disabled(),
                ..Default::default()
            },
        };
        let tables = build_tables(&config).expect("build tables");
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

        let config = IndexConfig {
            corpus: CorpusSpec {
                root: tmp.path(),
                kind: CorpusKind::Directory,
                follow_links: false,
                include_paths: &[],
                exclude_paths: &[],
            },
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

        let config = IndexConfig {
            corpus: CorpusSpec {
                root: tmp.path(),
                kind: CorpusKind::Directory,
                follow_links: false,
                include_paths: &[],
                exclude_paths: &[],
            },
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

        let config = IndexConfig {
            corpus: CorpusSpec {
                root: tmp.path(),
                kind: CorpusKind::Directory,
                follow_links: false,
                include_paths: &[],
                exclude_paths: &[],
            },
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

        let config = IndexConfig {
            corpus: CorpusSpec {
                root: tmp.path(),
                kind: CorpusKind::Directory,
                follow_links: false,
                include_paths: &[PathBuf::from("keep.txt")],
                exclude_paths: &[],
            },
            visibility: VisibilityConfig {
                ignore: IgnoreConfig::disabled(),
                ..Default::default()
            },
        };
        let tables = build_tables(&config).expect("build tables");
        assert_eq!(tables.fingerprints.len(), 1);
        assert_eq!(tables.fingerprints[0].path, PathBuf::from("keep.txt"));
    }

    #[test]
    fn build_single_file_corpus_ignores_siblings() {
        let tmp = TempDir::new().expect("create temp dir");
        let file = tmp.path().join("only.txt");
        fs::write(&file, "needle\n").expect("write file");
        fs::write(tmp.path().join("other.txt"), "haystack\n").expect("write other");

        let only_txt = PathBuf::from("only.txt");
        let config = IndexConfig {
            corpus: CorpusSpec {
                root: tmp.path(),
                kind: CorpusKind::SingleFile,
                follow_links: false,
                include_paths: &[only_txt],
                exclude_paths: &[],
            },
            visibility: VisibilityConfig::default(),
        };
        let tables = build_tables(&config).expect("build tables");
        assert_eq!(tables.fingerprints.len(), 1);
        assert_eq!(
            tables.fingerprints[0].path,
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

        let config = IndexConfig {
            corpus: CorpusSpec {
                root: tmp.path(),
                kind: CorpusKind::Directory,
                follow_links: false,
                include_paths: &[],
                exclude_paths: &[],
            },
            visibility: VisibilityConfig::default(),
        };
        let tables = build_tables(&config).expect("build tables");
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
}

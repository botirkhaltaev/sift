//! Walk corpus, extract trigrams, build in-memory index tables.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use rayon::prelude::*;

use super::file_table::FileFingerprint;
use super::storage::lexicon::LexiconEntry;
use super::storage::mmap::open_mmap;
use super::types::Trigram;

use crate::index::{CorpusKind, IndexBuildConfig};

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

        let extractor = if let (Some(prev_fps), Some(prev_tris)) =
            (self.prev_fingerprints, self.prev_trigrams)
        {
            TrigramExtractor::incremental(self.config.root, &fingerprints, prev_fps, prev_tris)
        } else {
            TrigramExtractor::fresh(self.config.root, &fingerprints)
        };
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

/// Walks the corpus directory and collects sorted relative file paths.
struct CorpusWalker<'a> {
    config: &'a IndexBuildConfig<'a>,
}

impl<'a> CorpusWalker<'a> {
    const fn new(config: &'a IndexBuildConfig<'a>) -> Self {
        Self { config }
    }

    fn collect(&self) -> crate::Result<Vec<PathBuf>> {
        let mut paths: Vec<PathBuf> = Vec::new();
        let walker = WalkBuilder::new(self.config.root)
            .follow_links(self.config.follow_links)
            .hidden(false)
            .parents(false)
            .ignore(false)
            .git_global(false)
            .git_ignore(false)
            .git_exclude(false)
            .require_git(false)
            .build();
        for entry in walker {
            let entry = entry.map_err(crate::Error::Ignore)?;
            if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                continue;
            }
            let path = entry.path();
            let rel = path
                .strip_prefix(self.config.root)
                .unwrap_or(path)
                .to_path_buf();
            if self
                .config
                .exclude_paths
                .iter()
                .any(|excluded| rel.starts_with(excluded))
            {
                continue;
            }
            if !self.config.include_paths.is_empty()
                && !self
                    .config
                    .include_paths
                    .iter()
                    .any(|included| rel == *included || rel.starts_with(included))
            {
                continue;
            }
            paths.push(rel);
        }
        paths.sort_unstable();
        Ok(paths)
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
    const fn fresh(root: &'a Path, fingerprints: &'a [FileFingerprint]) -> Self {
        Self {
            root,
            fingerprints,
            prev_fingerprints: None,
            prev_trigrams: None,
        }
    }

    const fn incremental(
        root: &'a Path,
        fingerprints: &'a [FileFingerprint],
        prev_fingerprints: &'a [FileFingerprint],
        prev_trigrams: &'a [Vec<Trigram>],
    ) -> Self {
        Self {
            root,
            fingerprints,
            prev_fingerprints: Some(prev_fingerprints),
            prev_trigrams: Some(prev_trigrams),
        }
    }

    fn extract(&self) -> crate::Result<Vec<Vec<Trigram>>> {
        let cache = self.build_lookup();
        self.fingerprints
            .par_iter()
            .map(|fp| {
                if let Some(tris) = cache.get(&(fp.path.as_path(), fp.mtime_secs, fp.size)) {
                    return Ok(tris.to_vec());
                }
                let abs = self.root.join(&fp.path);
                let mmap = open_mmap(&abs).map_err(crate::Error::Io)?;
                Ok(Trigram::unique_from_lossy_utf8(mmap.as_ref()))
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
        let mut map: HashMap<Trigram, Vec<u32>> = HashMap::new();

        for (id, tris) in self.file_trigrams.iter().enumerate() {
            let id_u32: u32 = id.try_into().map_err(|_| {
                crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "too many indexed files",
                ))
            })?;
            for tri in tris {
                map.entry(*tri).or_default().push(id_u32);
            }
        }

        let mut entries: Vec<_> = map.into_iter().collect();
        entries.sort_unstable_by_key(|(tri, _)| tri.to_bytes());

        let total_u32s: usize = entries.iter().map(|(_, ids)| ids.len()).sum();
        let mut posting_bytes: Vec<u8> = Vec::with_capacity(total_u32s.saturating_mul(4));
        let mut lex_entries: Vec<LexiconEntry> = Vec::with_capacity(entries.len());
        for (tri, ids) in entries {
            let offset: u64 = posting_bytes.len().try_into().map_err(|_| {
                crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "postings offset overflow",
                ))
            })?;
            let len: u32 = ids.len().try_into().map_err(|_| {
                crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "posting list too long",
                ))
            })?;
            for fid in &ids {
                posting_bytes.extend_from_slice(&fid.to_le_bytes());
            }
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
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn build_tables(tmp: &TempDir) -> IndexTables {
        let config = IndexBuildConfig {
            root: tmp.path(),
            follow_links: false,
            exclude_paths: &[],
            include_paths: &[],
            corpus_kind: CorpusKind::Directory,
        };
        IndexTableBuilder::new(&config)
            .build()
            .expect("build tables")
    }

    #[test]
    fn build_index_tables_sorts_file_paths_deterministically() {
        let tmp = TempDir::new().expect("create temp dir");
        fs::write(tmp.path().join("z.txt"), "hello\n").expect("write z");
        fs::write(tmp.path().join("a.txt"), "world\n").expect("write a");
        fs::write(tmp.path().join("m.txt"), "test\n").expect("write m");

        let tables = build_tables(&tmp);
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

        let tables = build_tables(&tmp);
        assert_eq!(tables.fingerprints.len(), 2);
        let file_id_0: u32 = 0;
        let file_id_1: u32 = 1;
        let occurrences_0 = tables
            .postings
            .chunks_exact(4)
            .filter(|chunk| {
                let bytes: [u8; 4] = (*chunk).try_into().unwrap();
                u32::from_le_bytes(bytes) == file_id_0
            })
            .count();
        let occurrences_1 = tables
            .postings
            .chunks_exact(4)
            .filter(|chunk| {
                let bytes: [u8; 4] = (*chunk).try_into().unwrap();
                u32::from_le_bytes(bytes) == file_id_1
            })
            .count();
        let unique_trigrams = 3;
        assert_eq!(
            occurrences_0, unique_trigrams,
            "file 0 ID should appear once per unique trigram"
        );
        assert_eq!(
            occurrences_1, unique_trigrams,
            "file 1 ID should appear once per unique trigram"
        );
    }

    #[test]
    fn trigram_extraction_returns_sorted_deduped_keys() {
        let tmp = TempDir::new().expect("create temp dir");
        fs::write(tmp.path().join("test.txt"), "ababab\n").expect("write file");

        let tables = build_tables(&tmp);
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

        let tables = build_tables(&tmp);
        assert_eq!(tables.fingerprints.len(), 1);
        assert!(
            tables.fingerprints[0].size > 0,
            "fingerprint should capture file size"
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

//! Walk corpus, extract trigrams, build in-memory index tables.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use memmap2::Mmap;
use rayon::prelude::*;

use super::storage::lexicon::{LexiconEntry, MappedLexicon};
use super::storage::mmap::open_mmap;
use super::storage::postings::MappedPostings;
use super::types::Trigram;

use crate::index::CorpusKind;
use crate::parallel::{ParallelWorkload, parallel_threshold};

pub struct IndexTables {
    pub files: Vec<PathBuf>,
    pub lexicon: Vec<LexiconEntry>,
    pub postings: Vec<u8>,
}

/// Configuration for index building: walk policy and path filters.
pub struct IndexBuildConfig<'a> {
    pub root: &'a Path,
    pub follow_links: bool,
    pub exclude_paths: &'a [PathBuf],
    pub include_paths: &'a [PathBuf],
}

fn collect_paths(config: &IndexBuildConfig<'_>) -> crate::Result<Vec<PathBuf>> {
    let mut paths: Vec<PathBuf> = Vec::new();
    let walker = WalkBuilder::new(config.root)
        .follow_links(config.follow_links)
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
        let display = path.strip_prefix(config.root).unwrap_or(path).to_path_buf();
        if config
            .exclude_paths
            .iter()
            .any(|excluded| display.starts_with(excluded))
        {
            continue;
        }
        if !config.include_paths.is_empty()
            && !config
                .include_paths
                .iter()
                .any(|included| display == *included || display.starts_with(included))
        {
            continue;
        }
        paths.push(display);
    }
    Ok(paths)
}

fn open_corpus_bytes(path: &Path) -> crate::Result<Mmap> {
    open_mmap(path).map_err(crate::Error::Io)
}

fn unique_trigrams_for_file(path: &Path) -> crate::Result<Vec<Trigram>> {
    let mmap = open_corpus_bytes(path)?;
    Ok(Trigram::unique_from_lossy_utf8(mmap.as_ref()))
}

pub fn build_index_tables(config: &IndexBuildConfig<'_>) -> crate::Result<IndexTables> {
    let mut paths = collect_paths(config)?;
    paths.sort_unstable();

    let threshold = parallel_threshold(ParallelWorkload::IndexBuild);
    let per_file = extract_trigrams_per_file(config.root, &paths, threshold)?;
    let rel_paths: Vec<PathBuf> = per_file.iter().map(|(p, _)| p.clone()).collect();

    let mut map: BTreeMap<Trigram, Vec<u32>> = BTreeMap::new();

    for (id, (_rel, tris)) in per_file.iter().enumerate() {
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

    let total_u32s: usize = map.values().map(Vec::len).sum();
    let mut posting_bytes: Vec<u8> = Vec::with_capacity(total_u32s.saturating_mul(4));
    let mut lex_entries: Vec<LexiconEntry> = Vec::with_capacity(map.len());
    for (tri, ids) in map {
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

    Ok(IndexTables {
        files: rel_paths,
        lexicon: lex_entries,
        postings: posting_bytes,
    })
}

fn extract_trigrams_per_file(
    root: &Path,
    paths: &[PathBuf],
    threshold: usize,
) -> crate::Result<Vec<(PathBuf, Vec<Trigram>)>> {
    let op = |display: &PathBuf| {
        let path = root.join(display);
        unique_trigrams_for_file(&path).map(|tris| (display.clone(), tris))
    };
    if paths.len() >= threshold {
        paths.par_iter().map(op).collect()
    } else {
        paths.iter().map(op).collect()
    }
}

fn compute_abs_paths(root: &Path, file_paths: &[PathBuf]) -> Vec<PathBuf> {
    file_paths.iter().map(|p| root.join(p)).collect()
}

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
        let tables = build_index_tables(&IndexBuildConfig {
            root: &root,
            follow_links: self.follow_links,
            exclude_paths: &exclude_paths,
            include_paths: &include_paths,
        })?;

        let lexicon = MappedLexicon::from_entries(&tables.lexicon);
        let postings = MappedPostings::from_bytes(&tables.postings);

        let abs_paths = compute_abs_paths(&root, &tables.files);
        let mut index = super::TrigramIndex {
            root,
            file_paths: tables.files,
            abs_paths,
            lexicon,
            postings,
            index_dir: None,
            corpus_kind: kind,
        };

        if let Some(dir) = self.dir {
            index.index_dir = Some(dir.clone());
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

    #[test]
    fn build_index_tables_sorts_file_paths_deterministically() {
        let tmp = TempDir::new().expect("create temp dir");
        fs::write(tmp.path().join("z.txt"), "hello\n").expect("write z");
        fs::write(tmp.path().join("a.txt"), "world\n").expect("write a");
        fs::write(tmp.path().join("m.txt"), "test\n").expect("write m");

        let tables = build_index_tables(&IndexBuildConfig {
            root: tmp.path(),
            follow_links: false,
            exclude_paths: &[],
            include_paths: &[],
        })
        .expect("build tables");
        let expected = vec![
            PathBuf::from("a.txt"),
            PathBuf::from("m.txt"),
            PathBuf::from("z.txt"),
        ];
        assert_eq!(tables.files, expected);
    }

    #[test]
    fn build_index_tables_exclude_paths_prevents_indexing() {
        let tmp = TempDir::new().expect("create temp dir");
        fs::write(tmp.path().join("keep.txt"), "hello\n").expect("write keep");
        let excluded_dir = tmp.path().join("excluded");
        fs::create_dir_all(&excluded_dir).expect("create excluded dir");
        fs::write(excluded_dir.join("skip.txt"), "world\n").expect("write skip");

        let tables = build_index_tables(&IndexBuildConfig {
            root: tmp.path(),
            follow_links: false,
            exclude_paths: &[PathBuf::from("excluded")],
            include_paths: &[],
        })
        .expect("build tables");
        assert_eq!(tables.files.len(), 1);
        assert_eq!(tables.files[0], PathBuf::from("keep.txt"));
    }

    #[test]
    fn build_index_tables_duplicate_trigrams_deduplicated_in_postings() {
        let tmp = TempDir::new().expect("create temp dir");
        fs::write(tmp.path().join("a.txt"), "ababab\n").expect("write file");
        fs::write(tmp.path().join("b.txt"), "ababab\n").expect("write file");

        let tables = build_index_tables(&IndexBuildConfig {
            root: tmp.path(),
            follow_links: false,
            exclude_paths: &[],
            include_paths: &[],
        })
        .expect("build tables");
        assert_eq!(tables.files.len(), 2);
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
    fn unique_trigrams_for_file_returns_sorted_deduped_keys() {
        let tmp = TempDir::new().expect("create temp dir");
        let file = tmp.path().join("test.txt");
        fs::write(&file, "ababab\n").expect("write file");

        let tris = unique_trigrams_for_file(&file).expect("extract trigrams");
        assert_eq!(tris.len(), 3);
        for i in 0..tris.len() - 1 {
            assert!(tris[i] <= tris[i + 1]);
        }
        let mut prev = None;
        for tri in &tris {
            if let Some(p) = prev {
                assert_ne!(*tri, p);
            }
            prev = Some(*tri);
        }
    }

    #[test]
    fn build_index_tables_include_paths_filters_to_single_file() {
        let tmp = TempDir::new().expect("create temp dir");
        fs::write(tmp.path().join("keep.txt"), "hello\n").expect("write keep");
        fs::write(tmp.path().join("skip.txt"), "world\n").expect("write skip");

        let tables = build_index_tables(&IndexBuildConfig {
            root: tmp.path(),
            follow_links: false,
            exclude_paths: &[],
            include_paths: &[PathBuf::from("keep.txt")],
        })
        .expect("build tables");
        assert_eq!(tables.files.len(), 1);
        assert_eq!(tables.files[0], PathBuf::from("keep.txt"));
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
        assert_eq!(index.file_paths.len(), 1);
        assert_eq!(
            index.file_paths[0],
            PathBuf::from("only.txt"),
            "should only index the specified file, not siblings"
        );
    }
}

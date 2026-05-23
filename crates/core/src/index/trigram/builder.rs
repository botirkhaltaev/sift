//! Walk corpus, extract trigrams, build in-memory index tables.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use memmap2::Mmap;
use rayon::prelude::*;

use crate::grep::parallel_candidate_threshold;
use crate::index::trigram::storage::lexicon::LexiconEntry;
use crate::index::trigram::storage::mmap::open_mmap;
use crate::query::trigram::extract_unique_trigrams_utf8_lossy;

pub struct IndexTables {
    pub files: Vec<PathBuf>,
    pub lexicon: Vec<LexiconEntry>,
    pub postings: Vec<u8>,
}

fn collect_paths(
    root: &Path,
    follow_links: bool,
    exclude_paths: &[PathBuf],
) -> crate::Result<(bool, Vec<PathBuf>)> {
    if root.is_file() {
        let Some(name) = root.file_name() else {
            return Err(crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "single-file corpus must have a file name",
            )));
        };
        let entry = PathBuf::from(name);
        return Ok((true, vec![entry]));
    }

    let mut paths: Vec<PathBuf> = Vec::new();
    let walker = WalkBuilder::new(root)
        .follow_links(follow_links)
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
        let display = path.strip_prefix(root).unwrap_or(path).to_path_buf();
        if exclude_paths
            .iter()
            .any(|excluded| display.starts_with(excluded))
        {
            continue;
        }
        paths.push(display);
    }
    Ok((false, paths))
}

fn open_corpus_bytes(path: &Path) -> crate::Result<Mmap> {
    open_mmap(path).map_err(crate::Error::Io)
}

fn unique_trigrams_for_file(path: &Path) -> crate::Result<Vec<[u8; 3]>> {
    let mmap = open_corpus_bytes(path)?;
    let mut tris: Vec<[u8; 3]> = extract_unique_trigrams_utf8_lossy(mmap.as_ref())
        .into_iter()
        .collect();
    tris.sort_unstable();
    Ok(tris)
}

fn actual_path(root: &Path, is_single_file: bool, display: &Path) -> PathBuf {
    if is_single_file {
        root.to_path_buf()
    } else {
        root.join(display)
    }
}

pub fn build_index_tables(
    root: &Path,
    follow_links: bool,
    exclude_paths: &[PathBuf],
) -> crate::Result<(bool, IndexTables)> {
    let (is_single_file, mut paths) = collect_paths(root, follow_links, exclude_paths)?;
    paths.sort_unstable();

    let min_parallel = parallel_candidate_threshold();
    let per_file: Vec<(PathBuf, Vec<[u8; 3]>)> = if paths.len() >= min_parallel {
        paths
            .par_iter()
            .map(|display| {
                let path = actual_path(root, is_single_file, display);
                unique_trigrams_for_file(&path).map(|tris| (display.clone(), tris))
            })
            .collect::<crate::Result<Vec<_>>>()?
    } else {
        paths
            .iter()
            .map(|display| {
                let path = actual_path(root, is_single_file, display);
                unique_trigrams_for_file(&path).map(|tris| (display.clone(), tris))
            })
            .collect::<crate::Result<Vec<_>>>()?
    };
    let rel_paths: Vec<PathBuf> = per_file.iter().map(|(p, _)| p.clone()).collect();

    let mut map: BTreeMap<[u8; 3], Vec<u32>> = BTreeMap::new();

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

    for ids in map.values_mut() {
        ids.sort_unstable();
        ids.dedup();
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
            trigram: tri,
            offset,
            len,
        });
    }

    Ok((
        is_single_file,
        IndexTables {
            files: rel_paths,
            lexicon: lex_entries,
            postings: posting_bytes,
        },
    ))
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
    pub fn build(self) -> crate::Result<crate::index::TrigramIndex> {
        let canonical = self.root.canonicalize()?;
        let (root, build_root) = if canonical.is_file() {
            let parent = canonical.parent().ok_or_else(|| {
                crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "single-file corpus must have a parent directory",
                ))
            })?;
            (parent.to_path_buf(), canonical)
        } else {
            (canonical.clone(), canonical)
        };
        let exclude_paths = self.excluded_build_paths(&root)?;
        let (is_single_file, tables) =
            build_index_tables(&build_root, self.follow_links, &exclude_paths)?;

        let files = crate::index::trigram::file_table::MappedFilesView::from_paths(&tables.files);
        let lexicon =
            crate::index::trigram::storage::lexicon::MappedLexicon::from_entries(&tables.lexicon);
        let postings =
            crate::index::trigram::storage::postings::MappedPostings::from_bytes(&tables.postings);

        let abs_paths = compute_abs_paths(&root, &tables.files);
        let mut index = crate::index::TrigramIndex {
            root,
            files,
            file_paths: tables.files,
            abs_paths,
            lexicon,
            postings,
            index_dir: None,
            was_single_file_corpus: is_single_file,
        };

        if let Some(dir) = self.dir {
            index.index_dir = Some(dir.clone());
            index.save_to_dir(&dir)?;
        }
        Ok(index)
    }
}

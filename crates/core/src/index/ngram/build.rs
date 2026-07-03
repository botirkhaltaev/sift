//! Corpus walk, file stat, and posting assembly helpers.
//!
//! These are private helpers used by [`Index::build`] and [`Index::update`].

use std::path::{Path, PathBuf};

use super::files::FileFingerprint;
use super::gram::{Gram, GramWidth};
use super::storage::grams::GramSet;
use super::storage::lexicon::LexiconEntry;

use crate::corpus::walk::FileWalk;
use crate::corpus::walk::LinkTraversal;
use crate::index::{CorpusKind, IndexBuildConfig};

/// Collected index data ready for persistence.
pub struct IndexTables {
    pub fingerprints: Vec<FileFingerprint>,
    pub file_grams: Vec<GramSet>,
    pub lexicon: Vec<LexiconEntry>,
    pub postings: Vec<u8>,
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

/// Assembles gram -> file ID posting lists from per-file gram sets.
pub struct PostingTables {
    pub lexicon: Vec<LexiconEntry>,
    pub postings: Vec<u8>,
}

impl PostingTables {
    pub fn assemble(width: GramWidth, file_grams: &[GramSet]) -> crate::Result<Self> {
        let total: usize = file_grams.iter().map(|s| s.as_slice().len()).sum();
        if file_grams.len() > u32::MAX as usize {
            return Err(crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "too many indexed files",
            )));
        }

        let mut pairs = Vec::with_capacity(total);
        for (fid, set) in file_grams.iter().enumerate() {
            let fid32 = u32::try_from(fid).expect("file count checked above");
            for gram in set.as_slice() {
                pairs.push((gram.ordinal(), fid32));
            }
        }
        pairs.sort_unstable();
        Self::encode_pairs(width, &pairs)
    }

    fn encode_pairs(width: GramWidth, pairs: &[(u64, u32)]) -> crate::Result<Self> {
        let mut posting_bytes = Vec::with_capacity(pairs.len() * 3);
        let mut lexicon = Vec::new();
        let mut i = 0;
        while i < pairs.len() {
            let gram_key = pairs[i].0;
            let start = i;
            while i < pairs.len() && pairs[i].0 == gram_key {
                i += 1;
            }
            let offset: u64 = posting_bytes.len().try_into().map_err(|_| {
                crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "postings offset overflow",
                ))
            })?;
            let len = u32::try_from(i - start).map_err(|_| {
                crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "posting list too long",
                ))
            })?;
            let mut prev = 0u64;
            for (j, &(_, fid)) in pairs[start..i].iter().enumerate() {
                let fid = u64::from(fid);
                let raw = if j == 0 {
                    fid
                } else {
                    fid.checked_sub(prev).ok_or_else(|| {
                        crate::Error::Io(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            "non-monotonic posting list",
                        ))
                    })?
                };
                let mut buf = unsigned_varint::encode::u64_buffer();
                let encoded = unsigned_varint::encode::u64(raw, &mut buf);
                posting_bytes.extend_from_slice(encoded);
                prev = fid;
            }
            lexicon.push(LexiconEntry {
                gram: Gram::from_ordinal(width, gram_key)?,
                offset,
                len,
            });
        }
        Ok(Self {
            lexicon,
            postings: posting_bytes,
        })
    }
}

/// Build in-memory index tables from a corpus configuration.
///
/// When `paths` is empty, discovers files via [`FileWalk`]. Otherwise indexes
/// exactly the given corpus-relative paths.
impl IndexTables {
    pub fn build(
        width: GramWidth,
        config: &IndexBuildConfig<'_>,
        paths: &[PathBuf],
    ) -> crate::Result<Self> {
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
                let paths = if paths.is_empty() {
                    FileWalk::new(config.corpus.root)
                        .scopes(config.corpus.include_paths)
                        .excludes(config.corpus.exclude_paths)
                        .visibility(config.visibility.clone())
                        .links(if config.corpus.follow_links {
                            LinkTraversal::Follow
                        } else {
                            LinkTraversal::DoNotFollow
                        })
                        .one_file_system(config.walk.one_file_system)
                        .max_depth(config.walk.max_depth)
                        .max_filesize(config.walk.max_filesize)
                        .collect_records::<PathBuf>()?
                } else {
                    paths.to_vec()
                };
                (paths, config.corpus.root)
            }
        };

        let fingerprints = FingerprintCollector::new(root, &paths).collect()?;

        let file_grams: Vec<GramSet> = fingerprints
            .par_iter()
            .map(|fp| {
                let abs = root.join(&fp.path);
                std::fs::read(&abs).map(|bytes| GramSet::collect(width, &bytes))
            })
            .collect::<std::io::Result<_>>()
            .map_err(crate::Error::Io)?;

        let tables = PostingTables::assemble(width, &file_grams)?;

        Ok(Self {
            fingerprints,
            file_grams,
            lexicon: tables.lexicon,
            postings: tables.postings,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::filter::IgnoreConfig;
    use crate::corpus::walk::FileWalk;
    use crate::corpus::walk::LinkTraversal;
    use crate::grep::{CandidateFilter, CandidateFilterConfig, VisibilityConfig};
    use crate::index::config::IndexWalkConfig;
    use crate::index::ngram::gram::GramWidth;
    use crate::index::ngram::storage::postings::Postings;
    use crate::index::{CorpusKind, CorpusSpec, IndexBuildConfig};
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    struct TablesFixture;

    impl TablesFixture {
        fn no_ignore_config(root: &Path) -> IndexBuildConfig<'_> {
            IndexBuildConfig {
                corpus: CorpusSpec {
                    root,
                    kind: CorpusKind::Directory,
                    follow_links: false,
                    include_paths: &[],
                    exclude_paths: &[],
                },
                walk: IndexWalkConfig::new(false),
                visibility: VisibilityConfig {
                    ignore: IgnoreConfig::disabled(),
                    ..Default::default()
                },
            }
        }

        fn build(root: &Path) -> IndexTables {
            let config = Self::no_ignore_config(root);
            IndexTables::build(GramWidth::TRIGRAM, &config, &[]).expect("build tables")
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
                    Postings::decode_sorted(slice)
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

        let config = IndexBuildConfig {
            corpus: CorpusSpec {
                root: tmp.path(),
                kind: CorpusKind::Directory,
                follow_links: false,
                include_paths: &[],
                exclude_paths: &[PathBuf::from("excluded")],
            },
            walk: IndexWalkConfig::new(false),
            visibility: VisibilityConfig {
                ignore: IgnoreConfig::disabled(),
                ..Default::default()
            },
        };
        let tables = IndexTables::build(GramWidth::TRIGRAM, &config, &[]).expect("build tables");
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
            corpus: CorpusSpec {
                root: tmp.path(),
                kind: CorpusKind::Directory,
                follow_links: false,
                include_paths: &[],
                exclude_paths: &[],
            },
            walk: IndexWalkConfig::new(false),
            visibility: VisibilityConfig::default(),
        };
        let paths = FileWalk::new(config.corpus.root)
            .visibility(config.visibility.clone())
            .links(LinkTraversal::DoNotFollow)
            .collect_records::<PathBuf>()
            .expect("walk corpus");
        assert!(paths.iter().any(|p| p == Path::new("keep.txt")));
        assert!(!paths.iter().any(|p| p.starts_with("skip")));
        assert!(!paths.iter().any(|p| p.starts_with("also_skip")));
    }

    #[test]
    fn corpus_walk_with_empty_ignore_sources_includes_gitignored() {
        let tmp = TempDir::new().expect("create temp dir");
        FilterCorpus::write(tmp.path());

        let config = IndexBuildConfig {
            corpus: CorpusSpec {
                root: tmp.path(),
                kind: CorpusKind::Directory,
                follow_links: false,
                include_paths: &[],
                exclude_paths: &[],
            },
            walk: IndexWalkConfig::new(false),
            visibility: VisibilityConfig {
                ignore: IgnoreConfig::disabled(),
                ..Default::default()
            },
        };
        let paths = FileWalk::new(config.corpus.root)
            .visibility(config.visibility.clone())
            .links(LinkTraversal::DoNotFollow)
            .collect_records::<PathBuf>()
            .expect("walk corpus");
        assert!(paths.iter().any(|p| p.starts_with("skip")));
        assert!(paths.iter().any(|p| p.starts_with("also_skip")));
    }

    #[test]
    fn corpus_walk_agrees_with_candidate_filter() {
        let tmp = TempDir::new().expect("create temp dir");
        FilterCorpus::write(tmp.path());

        let config = IndexBuildConfig {
            corpus: CorpusSpec {
                root: tmp.path(),
                kind: CorpusKind::Directory,
                follow_links: false,
                include_paths: &[],
                exclude_paths: &[],
            },
            walk: IndexWalkConfig::new(false),
            visibility: VisibilityConfig::default(),
        };
        let indexed = FileWalk::new(config.corpus.root)
            .visibility(config.visibility.clone())
            .links(LinkTraversal::DoNotFollow)
            .collect_records::<PathBuf>()
            .expect("walk corpus");
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
            corpus: CorpusSpec {
                root: tmp.path(),
                kind: CorpusKind::Directory,
                follow_links: false,
                include_paths: &[PathBuf::from("keep.txt")],
                exclude_paths: &[],
            },
            walk: IndexWalkConfig::new(false),
            visibility: VisibilityConfig {
                ignore: IgnoreConfig::disabled(),
                ..Default::default()
            },
        };
        let tables = IndexTables::build(GramWidth::TRIGRAM, &config, &[]).expect("build tables");
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
        let config = IndexBuildConfig {
            corpus: CorpusSpec {
                root: tmp.path(),
                kind: CorpusKind::SingleFile,
                follow_links: false,
                include_paths: &[only_txt],
                exclude_paths: &[],
            },
            walk: IndexWalkConfig::new(false),
            visibility: VisibilityConfig::default(),
        };
        let tables = IndexTables::build(GramWidth::TRIGRAM, &config, &[]).expect("build tables");
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

        let config = IndexBuildConfig {
            corpus: CorpusSpec {
                root: tmp.path(),
                kind: CorpusKind::Directory,
                follow_links: false,
                include_paths: &[],
                exclude_paths: &[],
            },
            walk: IndexWalkConfig::new(false),
            visibility: VisibilityConfig::default(),
        };
        let tables = IndexTables::build(GramWidth::TRIGRAM, &config, &[]).expect("build tables");
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
    fn build_prunes_gitignored_directory() {
        let tmp = TempDir::new().expect("create temp dir");
        fs::create_dir_all(tmp.path().join("src")).expect("mkdir src");
        fs::create_dir_all(tmp.path().join("target")).expect("mkdir target");
        fs::write(tmp.path().join(".gitignore"), "/target\n").expect("write gitignore");
        fs::write(tmp.path().join("src/keep.txt"), "needle\n").expect("write keep");
        fs::write(tmp.path().join("target/ignored.txt"), "secret\n").expect("write ignored");

        let config = IndexBuildConfig {
            corpus: CorpusSpec {
                root: tmp.path(),
                kind: CorpusKind::Directory,
                follow_links: false,
                include_paths: &[],
                exclude_paths: &[],
            },
            walk: IndexWalkConfig::new(false),
            visibility: VisibilityConfig::default(),
        };
        let tables = IndexTables::build(GramWidth::TRIGRAM, &config, &[]).expect("build tables");
        let paths: Vec<_> = tables.fingerprints.iter().map(|f| f.path.clone()).collect();
        assert!(
            paths.iter().any(|p| p == Path::new("src/keep.txt")),
            "keep must be indexed, got {paths:?}"
        );
        assert!(
            !paths.iter().any(|p| p.starts_with("target")),
            "target/ must not be indexed, got {paths:?}"
        );
    }
}

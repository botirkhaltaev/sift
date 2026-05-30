use std::fs;
use std::path::{Path, PathBuf};

use sift_core::{
    CandidateFilter, CandidateFilterConfig, CorpusKind, IndexKind, IndexStore, Indexes, QueryFlags,
    QuerySpec, VisibilityConfig,
};
use tempfile::TempDir;

use super::common::{build_store, make_filter_corpus, no_ignore_build_config, open_indexes};

#[test]
fn gitignore_honored_without_git_repo() {
    let tmp = TempDir::new().expect("tempdir");
    fs::write(tmp.path().join(".gitignore"), "*.log\n").expect("gitignore");
    fs::write(tmp.path().join("keep.txt"), "hello\n").expect("keep");
    fs::write(tmp.path().join("skip.log"), "secret\n").expect("skip");

    let sift_dir = tmp.path().join(".sift");
    build_store(tmp.path(), &sift_dir);

    let spec = QuerySpec {
        patterns: &["hello".to_string(), "secret".to_string()],
        flags: QueryFlags::empty(),
    };
    let paths: Vec<_> = open_indexes(&sift_dir)
        .candidates(&spec)
        .expect("candidates")
        .into_iter()
        .map(|c| c.rel_path().to_path_buf())
        .collect();
    assert!(
        !paths.iter().any(|p| p.ends_with("skip.log")),
        "paths: {paths:?}"
    );
    assert!(paths.iter().any(|p| p.ends_with("keep.txt")));
}

#[test]
fn empty_ignore_sources_indexes_gitignored_paths() {
    let tmp = TempDir::new().expect("tempdir");
    make_filter_corpus(tmp.path());

    let sift_dir = tmp.path().join(".sift");
    let mut store = IndexStore::open_or_create(
        &sift_dir,
        tmp.path(),
        CorpusKind::Directory,
        false,
        &[IndexKind::Trigram],
    )
    .expect("open");
    let config = no_ignore_build_config(tmp.path(), &[]);
    store.build(&[IndexKind::Trigram], &config).expect("build");

    let spec = QuerySpec {
        patterns: &["beta".to_string()],
        flags: QueryFlags::empty(),
    };
    let paths: Vec<_> = open_indexes(&sift_dir)
        .candidates(&spec)
        .expect("candidates")
        .into_iter()
        .map(|c| c.rel_path().to_path_buf())
        .collect();
    assert!(
        paths.iter().any(|p| p.starts_with("skip")),
        "no-ignore build should index skip/: {paths:?}"
    );
}

#[test]
fn defaults_exclude_gitignored_and_ignore_file_paths() {
    let tmp = TempDir::new().expect("tempdir");
    make_filter_corpus(tmp.path());

    let sift_dir = tmp.path().join(".sift");
    build_store(tmp.path(), &sift_dir);

    let spec = QuerySpec {
        patterns: &["beta".to_string()],
        flags: QueryFlags::empty(),
    };
    let paths: Vec<_> = open_indexes(&sift_dir)
        .candidates(&spec)
        .expect("candidates")
        .into_iter()
        .map(|c| c.rel_path().to_path_buf())
        .collect();
    assert!(paths.iter().any(|p| p == Path::new("keep.txt")));
    assert!(paths.iter().any(|p| p == Path::new("root.txt")));
    assert!(!paths.iter().any(|p| p.starts_with("skip")));
    assert!(!paths.iter().any(|p| p.starts_with("also_skip")));
}

#[test]
fn indexed_paths_match_candidate_filter() {
    let corpus = TempDir::new().expect("tempdir");
    let sift_dir = TempDir::new().expect("sift tempdir");
    make_filter_corpus(corpus.path());

    let mut store = IndexStore::open_or_create(
        sift_dir.path(),
        corpus.path(),
        CorpusKind::Directory,
        false,
        &[IndexKind::Trigram],
    )
    .expect("open");
    store
        .build(
            &[IndexKind::Trigram],
            &super::common::standard_build_config(corpus.path(), &[]),
        )
        .expect("build");

    let spec = QuerySpec {
        patterns: &[
            "beta".to_string(),
            "fn main".to_string(),
            "no match".to_string(),
        ],
        flags: QueryFlags::empty(),
    };
    let indexed: std::collections::HashSet<_> = Indexes::open(sift_dir.path())
        .expect("open")
        .candidates(&spec)
        .expect("candidates")
        .into_iter()
        .map(|c| c.rel_path().to_path_buf())
        .collect();

    let filter = CandidateFilter::new(
        &CandidateFilterConfig {
            visibility: VisibilityConfig::default(),
            ..CandidateFilterConfig::default()
        },
        corpus.path(),
    )
    .expect("filter");

    for rel in AllCorpusFiles::collect(corpus.path()) {
        let should_index = filter.matches_path(&rel);
        let is_indexed = indexed.contains(&rel);
        assert_eq!(
            is_indexed, should_index,
            "path {rel:?}: indexed={is_indexed} filter={should_index}"
        );
    }
}

struct AllCorpusFiles;

impl AllCorpusFiles {
    fn collect(root: &Path) -> Vec<PathBuf> {
        let mut files = Vec::new();
        Self::collect_recursive(root, root, &mut files);
        files.sort_unstable();
        files
    }

    fn collect_recursive(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).expect("read dir").flatten() {
            let path = entry.path();
            if path.is_dir() {
                Self::collect_recursive(root, &path, out);
            } else if path.is_file() {
                out.push(path.strip_prefix(root).expect("under root").to_path_buf());
            }
        }
    }
}

use std::fs;
use std::path::Path;

use sift_core::{IndexKind, IndexStore, Indexes, QueryFlags, QuerySpec};
use tempfile::TempDir;

use super::common::{
    build_store, make_filter_corpus, no_ignore_build_config, open_indexes, sample_store_meta,
};

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
    let tmp_path = tmp.path().to_path_buf();
    let root = tmp_path.canonicalize().unwrap_or(tmp_path);
    let meta = sample_store_meta(root, vec![IndexKind::Trigram]);
    let mut store = IndexStore::open_or_create(&sift_dir, &meta).expect("open");
    let config = no_ignore_build_config(tmp.path(), &[]);
    store
        .build(&[IndexKind::Trigram], &config, &[])
        .expect("build");

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
fn build_respects_hidden_files_by_default() {
    let corpus = TempDir::new().expect("tempdir");
    fs::create_dir_all(corpus.path().join(".secret")).expect("create dir");
    fs::write(corpus.path().join(".secret/hidden.txt"), "beta\n").expect("write");

    let sift_dir = TempDir::new().expect("sift tempdir");
    let corpus_path = corpus.path().to_path_buf();
    let root = corpus_path.canonicalize().unwrap_or(corpus_path);
    let meta = sample_store_meta(root, vec![IndexKind::Trigram]);
    let mut store = IndexStore::open_or_create(sift_dir.path(), &meta).expect("open");
    store
        .build(
            &[IndexKind::Trigram],
            &super::common::standard_build_config(corpus.path(), &[]),
            &[],
        )
        .expect("build");

    let spec = QuerySpec {
        patterns: &["beta".to_string()],
        flags: QueryFlags::empty(),
    };
    let paths: Vec<_> = Indexes::open(sift_dir.path())
        .expect("open")
        .candidates(&spec)
        .expect("candidates")
        .into_iter()
        .map(|c| c.rel_path().to_path_buf())
        .collect();
    assert!(
        !paths.iter().any(|p| p.starts_with(".secret")),
        "hidden files excluded by default: {paths:?}"
    );
}

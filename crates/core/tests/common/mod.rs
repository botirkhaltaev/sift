//! Shared fixtures and helpers for sift-core integration tests.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use sift_core::{
    CorpusKind, IndexBuildConfig, IndexKind, IndexStore, Indexes, TrigramIndex,
    TrigramIndexBuilder, VisibilityConfig,
};

pub struct IndexedPaths;

impl IndexedPaths {
    pub fn from_indexes(indexes: &Indexes) -> Vec<PathBuf> {
        indexes
            .resolve_all_files()
            .into_iter()
            .map(|c| c.rel_path().to_path_buf())
            .collect()
    }
}

pub fn make_parity_corpus(root: &Path) {
    fs::create_dir_all(root.join("a")).expect("create dir");
    fs::create_dir_all(root.join("b")).expect("create dir");
    fs::write(root.join("a/x.txt"), "alpha beta\n").expect("write");
    fs::write(root.join("b/y.txt"), "gamma delta\n").expect("write");
}

pub fn make_filter_corpus(root: &Path) {
    fs::create_dir_all(root.join("a")).expect("create dir");
    fs::create_dir_all(root.join("a/.secret")).expect("create dir");
    fs::create_dir_all(root.join("subdir")).expect("create dir");
    fs::create_dir_all(root.join("skip")).expect("create dir");
    fs::create_dir_all(root.join("also_skip")).expect("create dir");

    fs::write(root.join("a/x.txt"), "alpha beta gamma\n").expect("write");
    fs::write(root.join("a/.hidden.txt"), "beta in hidden file\n").expect("write");
    fs::write(root.join("a/data.rs"), "fn main() {}\n").expect("write");
    fs::write(root.join("a/.secret/log"), "beta in hidden dir\n").expect("write");
    fs::write(root.join("subdir/a.txt"), "beta in subdir\n").expect("write");
    fs::write(root.join("subdir/b.log"), "no match here\n").expect("write");
    fs::write(root.join("root.txt"), "beta at root level\n").expect("write");
    fs::write(root.join("skip/ignored.txt"), "beta gitignored\n").expect("write");
    fs::write(root.join("also_skip/omit.txt"), "beta in .ignore\n").expect("write");
    fs::write(root.join("keep.txt"), "beta outside ignore rules\n").expect("write");

    fs::write(root.join(".gitignore"), "skip/**\n").expect("write gitignore");
    fs::write(root.join(".ignore"), "also_skip/**\n").expect("write ignore");
}

pub fn standard_build_config<'a>(
    root: &'a Path,
    exclude_paths: &'a [PathBuf],
) -> IndexBuildConfig<'a> {
    IndexBuildConfig {
        root,
        follow_links: false,
        exclude_paths,
        include_paths: &[],
        corpus_kind: CorpusKind::Directory,
        visibility: VisibilityConfig::standard(),
    }
}

pub fn no_ignore_build_config<'a>(
    root: &'a Path,
    exclude_paths: &'a [PathBuf],
) -> IndexBuildConfig<'a> {
    IndexBuildConfig {
        root,
        follow_links: false,
        exclude_paths,
        include_paths: &[],
        corpus_kind: CorpusKind::Directory,
        visibility: VisibilityConfig::ignores_disabled(),
    }
}

pub fn build_store(corpus: &Path, sift_dir: &Path) -> IndexStore {
    let mut store = IndexStore::open_or_create(
        sift_dir,
        corpus,
        CorpusKind::Directory,
        false,
        &[IndexKind::Trigram],
    )
    .expect("open store");
    let config = standard_build_config(corpus, &[]);
    store
        .build(&[IndexKind::Trigram], &config)
        .expect("build index");
    store
}

pub fn open_indexes(sift_dir: &Path) -> Indexes {
    Indexes::open(sift_dir).expect("open indexes")
}

pub fn build_trigram_in_dir(corpus: &Path, trigram_dir: &Path) -> TrigramIndex {
    TrigramIndexBuilder::new(corpus)
        .with_dir(trigram_dir)
        .build()
        .expect("build trigram index")
}

pub fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            total = total.saturating_add(dir_size(&path));
        } else if let Ok(meta) = fs::metadata(&path) {
            total = total.saturating_add(meta.len());
        }
    }
    total
}

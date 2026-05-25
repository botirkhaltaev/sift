use std::fs;
use std::path::Path;

use sift_core::{
    CorpusKind, FileId, Indexes, QueryFlags, QuerySpec, TrigramIndex, TrigramIndexBuilder,
};
use tempfile::TempDir;

fn build_index_in_tmp(tmp: &TempDir, corpus_path: &Path) -> TrigramIndex {
    let trigram_dir = tmp.path().join("trigram");
    TrigramIndexBuilder::new(corpus_path)
        .with_dir(&trigram_dir)
        .build()
        .expect("build index")
}

#[test]
fn open_missing_current_returns_empty_registry() {
    let tmp = TempDir::new().expect("create temp dir");
    let indexes = Indexes::open(tmp.path()).expect("open indexes");
    assert!(indexes.is_empty());
}

#[test]
fn open_empty_sift_dir_returns_empty_registry() {
    let tmp = TempDir::new().expect("create temp dir");
    let sift_dir = tmp.path().join(".sift");
    fs::create_dir_all(&sift_dir).expect("create sift dir");
    let indexes = Indexes::open(&sift_dir).expect("open indexes");
    assert!(indexes.is_empty());
}

#[test]
fn open_broken_current_errors() {
    let tmp = TempDir::new().expect("create temp dir");
    let sift_dir = tmp.path().join(".sift");
    fs::create_dir_all(&sift_dir).expect("create sift dir");
    fs::write(sift_dir.join("CURRENT"), "nonexistent-snapshot-id\n").expect("write CURRENT");
    let result = Indexes::open(&sift_dir);
    assert!(result.is_err());
}

#[test]
fn explain_reports_indexed_for_literal() {
    let tmp = TempDir::new().expect("create temp dir");
    let corpus = tmp.path().join("corpus");
    fs::create_dir_all(&corpus).expect("create corpus");
    fs::write(corpus.join("a.txt"), "alpha beta\ngamma delta\n").expect("write file");

    let index = build_index_in_tmp(&tmp, &corpus);
    let spec = QuerySpec {
        patterns: &["foo.*".to_string()],
        flags: QueryFlags::empty(),
    };
    let output = index.explain(&spec);
    assert_eq!(output.pattern, "foo.*");
    assert_eq!(output.mode, sift_core::PlanMode::IndexedCandidates);
}

#[test]
fn explain_reports_full_scan_without_literal() {
    let tmp = TempDir::new().expect("create temp dir");
    let corpus = tmp.path().join("corpus");
    fs::create_dir_all(&corpus).expect("create corpus");
    fs::write(corpus.join("a.txt"), "alpha beta\ngamma delta\n").expect("write file");

    let index = build_index_in_tmp(&tmp, &corpus);
    let spec = QuerySpec {
        patterns: &[r"\w{5}\s+\w{5}".to_string()],
        flags: QueryFlags::empty(),
    };
    let output = index.explain(&spec);
    assert_eq!(output.pattern, r"\w{5}\s+\w{5}");
    assert_eq!(output.mode, sift_core::PlanMode::FullScan);
}

#[test]
fn explain_reports_full_scan_for_invert_match() {
    let tmp = TempDir::new().expect("create temp dir");
    let corpus = tmp.path().join("corpus");
    fs::create_dir_all(&corpus).expect("create corpus");
    fs::write(corpus.join("a.txt"), "alpha beta\n").expect("write file");

    let index = build_index_in_tmp(&tmp, &corpus);
    let mut flags = QueryFlags::empty();
    flags |= QueryFlags::INVERT_MATCH;
    let spec = QuerySpec {
        patterns: &["beta".to_string()],
        flags,
    };
    let output = index.explain(&spec);
    assert_eq!(output.mode, sift_core::PlanMode::FullScan);
}

#[test]
fn single_file_corpus_indexes_correctly() {
    let tmp = TempDir::new().expect("create temp dir");
    let corpus = tmp.path().join("corpus");
    fs::create_dir_all(&corpus).expect("create dir");
    let file = corpus.join("one.txt");
    fs::write(&file, "alpha\nbeta needle\n").expect("write file");

    let index = build_index_in_tmp(&tmp, &file);

    assert_eq!(index.corpus_kind(), CorpusKind::SingleFile);
    assert!(
        index.file_path(FileId::new(0)).is_some(),
        "single-file index should have file 0"
    );
    assert!(
        index.file_path(FileId::new(1)).is_none(),
        "single-file index should only have one file"
    );
    assert_eq!(
        index.file_path(FileId::new(0)).expect("get path"),
        Path::new("one.txt")
    );
}

#[test]
fn single_file_build_ignores_siblings() {
    let tmp = TempDir::new().expect("create temp dir");
    let corpus = tmp.path().join("corpus");
    fs::create_dir_all(&corpus).expect("create dir");
    let file = corpus.join("one.txt");
    fs::write(&file, "needle\n").expect("write file");
    fs::write(corpus.join("two.txt"), "haystack\n").expect("write sibling");

    let index = build_index_in_tmp(&tmp, &file);

    assert_eq!(index.corpus_kind(), CorpusKind::SingleFile);
    assert!(
        index.file_path(FileId::new(0)).is_some(),
        "should only index the specified file"
    );
    assert!(
        index.file_path(FileId::new(1)).is_none(),
        "should only index the specified file"
    );
}

#[test]
fn persisted_index_reopens_with_same_files() {
    let tmp = TempDir::new().expect("create temp dir");
    let corpus = tmp.path().join("corpus");
    fs::create_dir_all(&corpus).expect("create corpus dir");
    fs::write(corpus.join("a.txt"), "hello world\n").expect("write a");
    fs::write(corpus.join("b.txt"), "goodbye world\n").expect("write b");

    let trigram_dir = tmp.path().join("trigram");
    let root = corpus.canonicalize().expect("canonicalize corpus");
    let _ = TrigramIndexBuilder::new(&corpus)
        .with_dir(&trigram_dir)
        .build()
        .expect("build index");

    let reopened =
        TrigramIndex::open(&trigram_dir, &root, CorpusKind::Directory).expect("reopen index");
    assert!(reopened.file_path(FileId::new(0)).is_some());
    assert!(reopened.file_path(FileId::new(1)).is_some());
    assert!(
        reopened.file_path(FileId::new(2)).is_none(),
        "should have exactly two files"
    );
}

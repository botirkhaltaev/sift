use std::fs;
use std::path::Path;

use sift_core::search::{SearchOptions, SearchQueryBuilder};
use sift_core::{CorpusKind, FileId, Indexes, PlanMode};
use tempfile::TempDir;

use crate::common::build_trigram_in_dir;

#[test]
fn open_missing_current_returns_empty_registry() {
    let tmp = TempDir::new().expect("tempdir");
    let indexes = Indexes::open(tmp.path()).expect("open");
    assert!(!indexes.usable());
}

#[test]
fn open_empty_sift_dir_returns_empty_registry() {
    let tmp = TempDir::new().expect("tempdir");
    let sift_dir = tmp.path().join(".sift");
    fs::create_dir_all(&sift_dir).expect("mkdir");
    let indexes = Indexes::open(&sift_dir).expect("open");
    assert!(!indexes.usable());
}

#[test]
fn open_broken_current_errors() {
    let tmp = TempDir::new().expect("tempdir");
    let sift_dir = tmp.path().join(".sift");
    fs::create_dir_all(&sift_dir).expect("mkdir");
    fs::write(sift_dir.join("CURRENT"), "nonexistent-snapshot-id\n").expect("write");
    assert!(Indexes::open(&sift_dir).is_err());
}

#[test]
fn explain_reports_indexed_for_literal() {
    let tmp = TempDir::new().expect("tempdir");
    let corpus = tmp.path().join("corpus");
    fs::create_dir_all(&corpus).expect("mkdir");
    fs::write(corpus.join("a.txt"), "alpha beta\n").expect("write");

    let index = build_trigram_in_dir(&corpus, &tmp.path().join("trigram"));
    let query = SearchQueryBuilder::new(vec!["foo.*".to_string()])
        .options(SearchOptions::default())
        .build()
        .expect("query");
    let output = index.explain(&query);
    assert_eq!(output.pattern, "foo.*");
    assert_eq!(output.mode, PlanMode::IndexedCandidates);
}

#[test]
fn explain_reports_full_scan_without_literal() {
    let tmp = TempDir::new().expect("tempdir");
    let corpus = tmp.path().join("corpus");
    fs::create_dir_all(&corpus).expect("mkdir");
    fs::write(corpus.join("a.txt"), "alpha beta\n").expect("write");

    let index = build_trigram_in_dir(&corpus, &tmp.path().join("trigram"));
    let query = SearchQueryBuilder::new(vec![r"\w{5}\s+\w{5}".to_string()])
        .options(SearchOptions::default())
        .build()
        .expect("query");
    let output = index.explain(&query);
    assert_eq!(output.mode, PlanMode::FullScan);
}

#[test]
fn single_file_corpus_indexes_correctly() {
    let tmp = TempDir::new().expect("tempdir");
    let corpus = tmp.path().join("corpus");
    fs::create_dir_all(&corpus).expect("mkdir");
    let file = corpus.join("one.txt");
    fs::write(&file, "alpha\nbeta needle\n").expect("write");

    let index = build_trigram_in_dir(&file, &tmp.path().join("trigram"));
    assert_eq!(index.corpus_kind(), CorpusKind::SingleFile);
    assert!(index.file_path(FileId::new(0)).is_some());
    assert!(index.file_path(FileId::new(1)).is_none());
    assert_eq!(
        index.file_path(FileId::new(0)).expect("path"),
        Path::new("one.txt")
    );
}

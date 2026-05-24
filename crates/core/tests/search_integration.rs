use std::fs;
use std::path::Path;

use sift_core::{
    FileId, IndexError, IndexKind, IndexMeta, Indexes, META_FILENAME, QueryFlags, QuerySpec,
    TrigramIndex, TrigramIndexBuilder, TrigramIndexError,
};
use tempfile::TempDir;

fn build_index_in_tmp(tmp: &TempDir, corpus_path: &Path) -> TrigramIndex {
    let sift_dir = tmp.path().join(".sift");
    let trigram_dir = sift_dir.join("trigram");
    TrigramIndexBuilder::new(corpus_path)
        .with_dir(&trigram_dir)
        .build()
        .expect("build index")
}

#[test]
fn open_missing_meta_returns_empty_registry() {
    let tmp = TempDir::new().expect("create temp dir");
    let indexes = Indexes::open(tmp.path()).expect("open indexes");
    assert!(indexes.is_empty());
}

#[test]
fn open_missing_table_errors() {
    let tmp = TempDir::new().expect("create temp dir");
    let trigram_dir = tmp.path().join("trigram");
    fs::create_dir_all(&trigram_dir).expect("create trigram dir");
    let root_path = tmp.path().canonicalize().expect("canonicalize");
    let meta = IndexMeta {
        root: root_path,
        kind: IndexKind::Directory,
    };
    fs::write(
        trigram_dir.join(META_FILENAME),
        serde_json::to_string_pretty(&meta).expect("serialize meta"),
    )
    .expect("write meta");

    assert!(matches!(
        Indexes::open(tmp.path()),
        Err(IndexError::Trigram(TrigramIndexError::MissingComponent(_)))
    ));
}

#[test]
fn open_empty_meta_errors() {
    let tmp = TempDir::new().expect("create temp dir");
    let trigram_dir = tmp.path().join("trigram");
    fs::create_dir_all(&trigram_dir).expect("create trigram dir");
    fs::write(trigram_dir.join(META_FILENAME), "").expect("write empty meta");

    assert!(matches!(
        Indexes::open(tmp.path()),
        Err(IndexError::Trigram(TrigramIndexError::InvalidMeta(_)))
    ));
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
    assert_eq!(output.mode, "indexed_candidates");
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
    assert_eq!(output.mode, "full_scan");
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
    assert_eq!(output.mode, "full_scan");
}

#[test]
fn single_file_corpus_indexes_correctly() {
    let tmp = TempDir::new().expect("create temp dir");
    let corpus = tmp.path().join("corpus");
    fs::create_dir_all(&corpus).expect("create dir");
    let file = corpus.join("one.txt");
    fs::write(&file, "alpha\nbeta needle\n").expect("write file");

    let index = build_index_in_tmp(&tmp, &file);

    assert_eq!(index.kind(), IndexKind::SingleFile);
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

    assert_eq!(index.kind(), IndexKind::SingleFile);
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
fn meta_contains_root_path() {
    let tmp = TempDir::new().expect("create temp dir");
    let corpus = tmp.path().join("corpus");
    fs::create_dir_all(&corpus).expect("create dir");
    let file = corpus.join("one.txt");
    fs::write(&file, "alpha\n").expect("write file");

    let sift_dir = tmp.path().join(".sift");
    let trigram_dir = sift_dir.join("trigram");
    let _ = TrigramIndexBuilder::new(&file)
        .with_dir(&trigram_dir)
        .build()
        .expect("build index");

    let meta = fs::read_to_string(trigram_dir.join(META_FILENAME)).expect("read meta");
    assert!(meta.contains("\"root\""), "unexpected meta: {meta}");
    assert!(
        meta.contains("\"SingleFile\""),
        "single-file build should set kind to SingleFile in meta: {meta}"
    );
}

#[test]
fn persisted_index_reopens_with_same_files() {
    let tmp = TempDir::new().expect("create temp dir");
    let corpus = tmp.path().join("corpus");
    fs::create_dir_all(&corpus).expect("create corpus dir");
    fs::write(corpus.join("a.txt"), "hello world\n").expect("write a");
    fs::write(corpus.join("b.txt"), "goodbye world\n").expect("write b");

    let sift_dir = tmp.path().join(".sift");
    let trigram_dir = sift_dir.join("trigram");
    let _ = TrigramIndexBuilder::new(&corpus)
        .with_dir(&trigram_dir)
        .build()
        .expect("build index");

    let reopened = TrigramIndex::open(&trigram_dir).expect("reopen index");
    assert!(reopened.file_path(FileId::new(0)).is_some());
    assert!(reopened.file_path(FileId::new(1)).is_some());
    assert!(
        reopened.file_path(FileId::new(2)).is_none(),
        "should have exactly two files"
    );
}

#[test]
fn open_errors_when_known_index_kind_is_file() {
    let tmp = TempDir::new().expect("create temp dir");
    let sift_dir = tmp.path().join(".sift");
    fs::create_dir_all(&sift_dir).expect("create sift dir");
    let trigram_path = sift_dir.join("trigram");
    fs::write(&trigram_path, "not a directory").expect("write file as trigram");

    assert!(matches!(
        Indexes::open(&sift_dir),
        Err(IndexError::InvalidLayout { path }) if path == trigram_path
    ));
}

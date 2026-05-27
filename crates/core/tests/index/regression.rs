//! CI regression guards for index build size and correctness (SIFTPST2 / SIFTTRI2 only).

use std::path::Path;

use sift_core::{QueryFlags, QuerySpec};
use tempfile::TempDir;

use super::common::{build_store, dir_size, make_filter_corpus, open_indexes};

/// Upper bound on on-disk snapshot size for [`make_filter_corpus`]; bump only when
/// intentionally changing compression or fixture layout.
const FILTER_CORPUS_INDEX_MAX_BYTES: u64 = 512 * 1024;

#[test]
fn filter_corpus_file_set_stable() {
    let tmp = TempDir::new().expect("tempdir");
    make_filter_corpus(tmp.path());
    let sift_dir = tmp.path().join(".sift");
    build_store(tmp.path(), &sift_dir);

    let paths: Vec<_> = open_indexes(&sift_dir)
        .resolve_all_files()
        .into_iter()
        .map(|c| c.rel_path().to_path_buf())
        .collect();

    assert!(paths.iter().any(|p| p == Path::new("keep.txt")));
    assert!(paths.iter().any(|p| p == Path::new("root.txt")));
    assert!(!paths.iter().any(|p| p.starts_with("skip")));
    assert!(!paths.iter().any(|p| p.starts_with("also_skip")));
}

#[test]
fn filter_corpus_index_size_within_budget() {
    let tmp = TempDir::new().expect("tempdir");
    make_filter_corpus(tmp.path());
    let sift_dir = tmp.path().join(".sift");
    build_store(tmp.path(), &sift_dir);

    let size = dir_size(&sift_dir);
    assert!(
        size < FILTER_CORPUS_INDEX_MAX_BYTES,
        "index size {size} exceeds budget {FILTER_CORPUS_INDEX_MAX_BYTES}"
    );
}

#[test]
fn literal_candidates_narrow_to_expected_file() {
    let tmp = TempDir::new().expect("tempdir");
    make_filter_corpus(tmp.path());
    let sift_dir = tmp.path().join(".sift");
    build_store(tmp.path(), &sift_dir);

    let spec = QuerySpec {
        patterns: &["beta".to_string()],
        flags: QueryFlags::empty(),
    };
    let candidates = open_indexes(&sift_dir).resolve_candidates(&spec);
    assert!(!candidates.is_empty());
    assert!(
        candidates
            .iter()
            .any(|c| c.rel_path() == Path::new("keep.txt"))
    );
    assert!(!candidates.iter().any(|c| c.rel_path().starts_with("skip")));
}

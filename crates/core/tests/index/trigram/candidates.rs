use std::path::Path;

use sift_core::{QueryFlags, QuerySpec};
use tempfile::TempDir;

use super::super::common::{
    build_store, build_trigram_in_dir, make_filter_corpus, make_parity_corpus, open_indexes,
};

#[test]
fn literal_query_returns_indexed_candidates() {
    let tmp = TempDir::new().expect("tempdir");
    let corpus = tmp.path().join("corpus");
    make_parity_corpus(&corpus);

    let index = build_trigram_in_dir(&corpus, &tmp.path().join("trigram"));
    let spec = QuerySpec {
        patterns: &["beta".to_string()],
        flags: QueryFlags::empty(),
    };
    let candidates = index.candidates(&spec).expect("candidates");
    assert!(!candidates.is_empty());
    assert!(
        candidates
            .iter()
            .any(|c| c.rel_path() == Path::new("a/x.txt"))
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
    let candidates = open_indexes(&sift_dir)
        .candidates(&spec)
        .expect("candidates");
    assert!(!candidates.is_empty());
    assert!(
        candidates
            .iter()
            .any(|c| c.rel_path() == Path::new("keep.txt"))
    );
    assert!(!candidates.iter().any(|c| c.rel_path().starts_with("skip")));
}

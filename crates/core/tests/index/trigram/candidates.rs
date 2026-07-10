use std::{fs, path::Path};

use sift_core::CandidatePlan;
use sift_core::candidates::{CandidateFlags, CandidateSpec};
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
    let spec = CandidateSpec {
        patterns: &["beta".to_string()],
        flags: CandidateFlags::empty(),
    };
    let candidates = match index.plan(&spec) {
        sift_core::CandidatePlan::Narrowed { candidates, .. } => candidates,
        other => panic!("expected narrowed plan, got {other:?}"),
    };
    assert!(!candidates.is_empty());
    assert!(
        candidates
            .iter()
            .any(|c| c.rel_path() == Path::new("a/x.txt"))
    );
}

#[test]
fn literal_query_matching_every_file_reports_no_narrowing() {
    let tmp = TempDir::new().expect("tempdir");
    let corpus = tmp.path().join("corpus");
    fs::create_dir_all(&corpus).expect("create corpus");
    fs::write(corpus.join("a.txt"), "shared beta\n").expect("write a");
    fs::write(corpus.join("b.txt"), "another beta\n").expect("write b");

    let index = build_trigram_in_dir(&corpus, &tmp.path().join("trigram"));
    let spec = CandidateSpec {
        patterns: &["beta".to_string()],
        flags: CandidateFlags::empty(),
    };

    assert!(matches!(
        index.plan(&spec),
        CandidatePlan::AllIndexed { .. }
    ));
}

#[test]
fn literal_candidates_narrow_to_expected_file() {
    let tmp = TempDir::new().expect("tempdir");
    make_filter_corpus(tmp.path());
    let sift_dir = tmp.path().join(".sift");
    build_store(tmp.path(), &sift_dir);

    let spec = CandidateSpec {
        patterns: &["beta".to_string()],
        flags: CandidateFlags::empty(),
    };
    let candidates = match open_indexes(&sift_dir).plan(&spec) {
        sift_core::CandidatePlan::Narrowed { candidates, .. } => candidates,
        other => panic!("expected narrowed plan, got {other:?}"),
    };
    assert!(!candidates.is_empty());
    assert!(
        candidates
            .iter()
            .any(|c| c.rel_path() == Path::new("keep.txt"))
    );
    assert!(!candidates.iter().any(|c| c.rel_path().starts_with("skip")));
}

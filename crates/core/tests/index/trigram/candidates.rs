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
    let file_ids = match index.plan(&spec) {
        CandidatePlan::Narrowed { file_ids, .. } => file_ids,
        other => panic!("expected narrowed plan, got {other:?}"),
    };
    let candidates = index.materialize_file_ids(&file_ids);
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

    assert!(matches!(index.plan(&spec), CandidatePlan::AllIndexed));
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
    let indexes = open_indexes(&sift_dir);
    let file_ids = match indexes.plan(&spec) {
        CandidatePlan::Narrowed { file_ids, .. } => file_ids,
        other => panic!("expected narrowed plan, got {other:?}"),
    };
    let candidates = indexes.materialize(&file_ids);
    assert!(!candidates.is_empty());
    assert!(
        candidates
            .iter()
            .any(|c| c.rel_path() == Path::new("keep.txt"))
    );
    assert!(!candidates.iter().any(|c| c.rel_path().starts_with("skip")));
}

#[test]
fn case_insensitive_uppercase_corpus_narrows() {
    let tmp = TempDir::new().expect("tempdir");
    let corpus = tmp.path().join("corpus");
    fs::create_dir_all(&corpus).expect("create corpus");
    fs::write(corpus.join("hit.rs"), "let x = ERR_SYS;\n").expect("write hit");
    fs::write(corpus.join("miss.rs"), "let y = other_symbol;\n").expect("write miss");
    for i in 0..40 {
        fs::write(
            corpus.join(format!("noise{i}.rs")),
            format!("fn noise_{i}() {{ let v = {i}; }}\n"),
        )
        .expect("write noise");
    }

    let index = build_trigram_in_dir(&corpus, &tmp.path().join("trigram"));
    let pattern = "err_sys|pme_turn_off".to_string();
    let spec = CandidateSpec {
        patterns: &[pattern],
        flags: CandidateFlags::CASE_INSENSITIVE,
    };
    let file_ids = match index.plan(&spec) {
        CandidatePlan::Narrowed { file_ids, .. } => file_ids,
        other => panic!("expected narrowed casei plan, got {other:?}"),
    };
    let candidates = index.materialize_file_ids(&file_ids);
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].rel_path(), Path::new("hit.rs"));
}

#[test]
fn case_insensitive_alternation_narrows_uppercase_symbols() {
    let tmp = TempDir::new().expect("tempdir");
    let corpus = tmp.path().join("corpus");
    fs::create_dir_all(&corpus).expect("create corpus");
    fs::write(corpus.join("a.rs"), "ERR_SYS\n").expect("write a");
    fs::write(corpus.join("b.rs"), "PME_TURN_OFF\n").expect("write b");
    fs::write(corpus.join("c.rs"), "LINK_REQ_RST\n").expect("write c");
    fs::write(corpus.join("d.rs"), "CFG_BME_EVT\n").expect("write d");
    // Short case-folded prefixes are common in noise; old HIR case folding
    // extracted only those prefixes and collapsed to AllIndexed.
    for i in 0..80 {
        fs::write(
            corpus.join(format!("noise{i}.rs")),
            format!(
                "fn noise_{i}() {{ let err_code = 1; let cfg_x = 2; let link_y = 3; let pme_z = 4; }}\n"
            ),
        )
        .expect("write noise");
    }

    let index = build_trigram_in_dir(&corpus, &tmp.path().join("trigram"));
    let pattern = "ERR_SYS|PME_TURN_OFF|LINK_REQ_RST|CFG_BME_EVT".to_string();
    let spec = CandidateSpec {
        patterns: &[pattern],
        flags: CandidateFlags::CASE_INSENSITIVE,
    };
    let file_ids = match index.plan(&spec) {
        CandidatePlan::Narrowed { file_ids, .. } => file_ids,
        other => panic!("expected narrowed casei alternation, got {other:?}"),
    };
    let candidates = index.materialize_file_ids(&file_ids);
    assert_eq!(candidates.len(), 4);
}

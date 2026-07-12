use std::{fs, path::Path};

use sift_core::grep::FilterAdmission;
use sift_core::search::CaseMode;
use sift_core::search::SearchOptions;
use tempfile::TempDir;

use super::super::common::{
    build_store, index_candidates, make_filter_corpus, make_parity_corpus, open_indexes,
};

#[test]
fn literal_query_returns_indexed_candidates() {
    let tmp = TempDir::new().expect("tempdir");
    let corpus = tmp.path().join("corpus");
    make_parity_corpus(&corpus);

    let sift_dir = tmp.path().join(".sift");
    build_store(&corpus, &sift_dir);

    let indexes = open_indexes(&sift_dir);
    let candidates = index_candidates(
        &indexes,
        &corpus,
        &["beta".to_string()],
        SearchOptions::default(),
        FilterAdmission::Full,
    );
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

    let sift_dir = tmp.path().join(".sift");
    build_store(&corpus, &sift_dir);

    let indexes = open_indexes(&sift_dir);
    let candidates = index_candidates(
        &indexes,
        &corpus,
        &["beta".to_string()],
        SearchOptions::default(),
        FilterAdmission::Full,
    );
    assert_eq!(candidates.len(), 2);
}

#[test]
fn literal_candidates_narrow_to_expected_file() {
    let tmp = TempDir::new().expect("tempdir");
    make_filter_corpus(tmp.path());
    let sift_dir = tmp.path().join(".sift");
    build_store(tmp.path(), &sift_dir);

    let indexes = open_indexes(&sift_dir);
    let candidates = index_candidates(
        &indexes,
        tmp.path(),
        &["beta".to_string()],
        SearchOptions::default(),
        FilterAdmission::Full,
    );
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

    let sift_dir = tmp.path().join(".sift");
    build_store(&corpus, &sift_dir);

    let indexes = open_indexes(&sift_dir);
    let pattern = "err_sys|pme_turn_off".to_string();
    let options = SearchOptions {
        case_mode: CaseMode::Insensitive,
        ..SearchOptions::default()
    };
    let candidates = index_candidates(
        &indexes,
        &corpus,
        &[pattern],
        options,
        FilterAdmission::Full,
    );
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
    for i in 0..80 {
        fs::write(
            corpus.join(format!("noise{i}.rs")),
            format!(
                "fn noise_{i}() {{ let err_code = 1; let cfg_x = 2; let link_y = 3; let pme_z = 4; }}\n"
            ),
        )
        .expect("write noise");
    }

    let sift_dir = tmp.path().join(".sift");
    build_store(&corpus, &sift_dir);

    let indexes = open_indexes(&sift_dir);
    let pattern = "ERR_SYS|PME_TURN_OFF|LINK_REQ_RST|CFG_BME_EVT".to_string();
    let options = SearchOptions {
        case_mode: CaseMode::Insensitive,
        ..SearchOptions::default()
    };
    let candidates = index_candidates(
        &indexes,
        &corpus,
        &[pattern],
        options,
        FilterAdmission::Full,
    );
    assert_eq!(candidates.len(), 4);
}

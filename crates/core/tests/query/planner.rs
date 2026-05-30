use std::fs;
use std::path::{Path, PathBuf};

use sift_core::{
    Candidate, CandidateRequirement, CorpusKind, IndexConfig, IndexKind, IndexStore, Indexes,
    QueryFlags, QueryPlanner, QuerySpec, VisibilityConfig,
};
use tempfile::TempDir;

fn build_indexes(root: &Path, sift_dir: &Path) -> Indexes {
    let mut store = IndexStore::open_or_create(
        sift_dir,
        root,
        CorpusKind::Directory,
        false,
        &[IndexKind::Trigram],
    )
    .expect("open store");
    store
        .build(
            &[IndexKind::Trigram],
            &IndexConfig {
                corpus: sift_core::CorpusSpec {
                    root,
                    kind: CorpusKind::Directory,
                    follow_links: false,
                    include_paths: &[],
                    exclude_paths: &[],
                },
                visibility: VisibilityConfig::default(),
            },
        )
        .expect("build");
    Indexes::open(sift_dir).expect("open indexes")
}

fn make_parity_corpus(root: &Path) {
    fs::create_dir_all(root.join("a")).expect("create dir");
    fs::create_dir_all(root.join("b")).expect("create dir");
    fs::write(root.join("a/x.txt"), "alpha beta gamma\n").expect("write");
    fs::write(root.join("b/y.txt"), "delta epsilon\n").expect("write");
}

fn candidates_from_paths(root: &Path, rels: &[&str]) -> Vec<Candidate> {
    rels.iter()
        .map(|r| Candidate::new(PathBuf::from(r), root.join(r)))
        .collect()
}

#[test]
fn potential_matches_narrowable_uses_index() {
    let tmp = TempDir::new().expect("tempdir");
    let corpus = tmp.path().join("corpus");
    make_parity_corpus(&corpus);
    let sift_dir = tmp.path().join(".sift");
    let indexes = build_indexes(&corpus, &sift_dir);
    let spec = QuerySpec {
        patterns: &["beta".to_string()],
        flags: QueryFlags::empty(),
    };
    let result = QueryPlanner::new(spec)
        .candidates(&indexes, CandidateRequirement::PotentialMatches, || {
            panic!("base should not be called when index narrows")
        })
        .expect("candidates");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].rel_path(), Path::new("a/x.txt"));
}

#[test]
fn potential_matches_non_narrowable_falls_back_to_base() {
    let tmp = TempDir::new().expect("tempdir");
    let corpus = tmp.path().join("corpus");
    make_parity_corpus(&corpus);
    let sift_dir = tmp.path().join(".sift");
    let indexes = build_indexes(&corpus, &sift_dir);
    let spec = QuerySpec {
        patterns: &[".*".to_string()],
        flags: QueryFlags::empty(),
    };
    let base = candidates_from_paths(&corpus, &["a/x.txt", "b/y.txt"]);
    let result = QueryPlanner::new(spec)
        .candidates(&indexes, CandidateRequirement::PotentialMatches, || {
            Ok(base)
        })
        .expect("candidates");
    let mut paths: Vec<_> = result.iter().map(|c| c.rel_path().to_path_buf()).collect();
    paths.sort();
    assert_eq!(
        paths,
        vec![PathBuf::from("a/x.txt"), PathBuf::from("b/y.txt")]
    );
}

#[test]
fn complete_falls_back_to_base() {
    let tmp = TempDir::new().expect("tempdir");
    let corpus = tmp.path().join("corpus");
    make_parity_corpus(&corpus);
    let sift_dir = tmp.path().join(".sift");
    let indexes = build_indexes(&corpus, &sift_dir);
    let spec = QuerySpec {
        patterns: &["beta".to_string()],
        flags: QueryFlags::empty(),
    };
    let base = candidates_from_paths(&corpus, &["a/x.txt", "b/y.txt"]);
    let result = QueryPlanner::new(spec)
        .candidates(&indexes, CandidateRequirement::Complete, || Ok(base))
        .expect("candidates");
    assert_eq!(result.len(), 2);
}

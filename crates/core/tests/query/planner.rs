use std::fs;
use std::path::{Path, PathBuf};

use sift_core::search::{CandidateFilter, CandidateFilterConfig, GlobConfig, VisibilityConfig};
use sift_core::{
    Candidate, CandidateRequirement, CorpusKind, CorpusMeta, FilterMeta, IndexConfig, IndexKind,
    IndexStore, IndexWalkConfig, Indexes, QueryFlags, QueryPlanner, QuerySpec, StoreMeta, WalkMeta,
};
use tempfile::TempDir;

fn build_indexes(root: &Path, sift_dir: &Path) -> Indexes {
    let root_buf = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let meta = StoreMeta::new(
        CorpusMeta {
            root: root_buf,
            kind: CorpusKind::Directory,
            include_paths: Vec::new(),
            exclude_paths: Vec::new(),
        },
        WalkMeta {
            follow_links: false,
            one_file_system: false,
            max_depth: None,
            max_filesize: None,
        },
        FilterMeta {
            visibility: VisibilityConfig::default(),
        },
        vec![IndexKind::Trigram],
    );
    let mut store = IndexStore::open_or_create(sift_dir, &meta).expect("open store");
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
                walk: IndexWalkConfig::new(false),
                visibility: VisibilityConfig::default(),
            },
            &[],
        )
        .expect("build");
    Indexes::open(sift_dir).expect("open indexes")
}

fn default_filter(root: &Path) -> CandidateFilter {
    CandidateFilter::new(
        &CandidateFilterConfig {
            scopes: vec![PathBuf::from("")],
            exclude_paths: Vec::new(),
            glob: GlobConfig::default(),
            visibility: VisibilityConfig::default(),
            follow_links: false,
            max_depth: None,
            max_filesize: None,
            type_definitions: Vec::new(),
            type_include: Vec::new(),
            type_exclude: Vec::new(),
            one_file_system: false,
        },
        root,
    )
    .expect("filter")
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
    let filter = default_filter(&corpus);
    let result = QueryPlanner::new(spec)
        .candidates(
            &indexes,
            CandidateRequirement::PotentialMatches,
            &filter,
            None,
            false,
            || panic!("base should not be called when index narrows"),
        )
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
    let filter = default_filter(&corpus);
    let base = candidates_from_paths(&corpus, &["a/x.txt", "b/y.txt"]);
    let result = QueryPlanner::new(spec)
        .candidates(
            &indexes,
            CandidateRequirement::PotentialMatches,
            &filter,
            None,
            false,
            || Ok(base),
        )
        .expect("candidates");
    let mut paths: Vec<_> = result.iter().map(|c| c.rel_path().to_path_buf()).collect();
    paths.sort();
    assert_eq!(
        paths,
        vec![PathBuf::from("a/x.txt"), PathBuf::from("b/y.txt")]
    );
}

#[test]
fn potential_matches_includes_unindexed_walk_paths() {
    let tmp = TempDir::new().expect("tempdir");
    let corpus = tmp.path().join("corpus");
    make_parity_corpus(&corpus);
    let sift_dir = tmp.path().join(".sift");
    let indexes = build_indexes(&corpus, &sift_dir);
    fs::write(corpus.join("new.txt"), "offline marker\n").expect("write new file");

    let spec = QuerySpec {
        patterns: &["offline marker".to_string()],
        flags: QueryFlags::empty(),
    };
    let filter = default_filter(&corpus);
    let base = candidates_from_paths(&corpus, &["a/x.txt", "b/y.txt", "new.txt"]);
    let result = QueryPlanner::new(spec)
        .candidates(
            &indexes,
            CandidateRequirement::PotentialMatches,
            &filter,
            None,
            true,
            || Ok(base),
        )
        .expect("candidates");
    let paths: Vec<_> = result.iter().map(|c| c.rel_path().to_path_buf()).collect();
    assert_eq!(paths, vec![PathBuf::from("new.txt")]);
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
    let filter = default_filter(&corpus);
    let base = candidates_from_paths(&corpus, &["a/x.txt", "b/y.txt"]);
    let result = QueryPlanner::new(spec)
        .candidates(
            &indexes,
            CandidateRequirement::Complete,
            &filter,
            None,
            false,
            || Ok(base),
        )
        .expect("candidates");
    assert_eq!(result.len(), 2);
}

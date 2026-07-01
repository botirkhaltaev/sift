use std::fs;
use std::path::{Path, PathBuf};

use sift_core::grep::{CandidateFilter, CandidateFilterConfig, GlobConfig, VisibilityConfig};
use sift_core::{
    Candidate, CandidatePlan, CandidateRequirement, CandidateSource, CorpusKind, CorpusMeta,
    FilterMeta, GramWidth, IndexBuildConfig, IndexConfig, IndexCoverage, IndexStore,
    IndexWalkConfig, Indexes, QueryFlags, QueryPlanner, QuerySpec, SnapshotValidation, StoreMeta,
    WalkMeta,
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
        IndexCoverage::Complete,
        WalkMeta {
            follow_links: false,
            one_file_system: false,
            max_depth: None,
            max_filesize: None,
        },
        FilterMeta {
            visibility: VisibilityConfig::default(),
        },
        vec![IndexConfig::ngram(GramWidth::TRIGRAM)],
    );
    let mut store = IndexStore::open_or_create(sift_dir, &meta).expect("open store");
    store
        .build(
            &[IndexConfig::ngram(GramWidth::TRIGRAM)],
            &IndexBuildConfig {
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
    CandidateFilter::new(&default_filter_config(), root).expect("filter")
}

fn default_filter_config() -> CandidateFilterConfig {
    CandidateFilterConfig {
        scopes: vec![PathBuf::from("")],
        exclude_paths: Vec::new(),
        glob: GlobConfig::default(),
        visibility: VisibilityConfig::default(),
        follow_links: false,
        max_depth: None,
        max_filesize: None,
        type_definitions: Vec::new(),
        type_selections: Vec::new(),
        type_include: Vec::new(),
        type_exclude: Vec::new(),
        one_file_system: false,
    }
}

fn default_meta(root: &Path) -> StoreMeta {
    StoreMeta::new(
        CorpusMeta {
            root: root.to_path_buf(),
            kind: CorpusKind::Directory,
            include_paths: Vec::new(),
            exclude_paths: Vec::new(),
        },
        IndexCoverage::Complete,
        WalkMeta {
            follow_links: false,
            one_file_system: false,
            max_depth: None,
            max_filesize: None,
        },
        FilterMeta {
            visibility: VisibilityConfig::default(),
        },
        vec![IndexConfig::ngram(GramWidth::TRIGRAM)],
    )
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
            CandidatePlan {
                indexes: &indexes,
                requirement: CandidateRequirement::PotentialMatches,
                filter: &filter,
                source: CandidateSource {
                    store_meta: None,
                    snapshot: SnapshotValidation::Unvalidated,
                },
            },
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
            CandidatePlan {
                indexes: &indexes,
                requirement: CandidateRequirement::PotentialMatches,
                filter: &filter,
                source: CandidateSource {
                    store_meta: None,
                    snapshot: SnapshotValidation::Unvalidated,
                },
            },
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
fn lazy_potential_matches_includes_unindexed_walk_paths() {
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
    let meta = StoreMeta {
        coverage: IndexCoverage::Lazy,
        ..default_meta(&corpus)
    };
    let result = QueryPlanner::new(spec)
        .candidates(
            CandidatePlan {
                indexes: &indexes,
                requirement: CandidateRequirement::PotentialMatches,
                filter: &filter,
                source: CandidateSource {
                    store_meta: Some(&meta),
                    snapshot: SnapshotValidation::Unvalidated,
                },
            },
            || Ok(base),
        )
        .expect("candidates");
    let paths: Vec<_> = result.iter().map(|c| c.rel_path().to_path_buf()).collect();
    assert_eq!(paths, vec![PathBuf::from("new.txt")]);
}

#[test]
fn potential_matches_validated_snapshot_skips_unindexed_walk() {
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
    let meta = default_meta(&corpus);
    let result = QueryPlanner::new(spec)
        .candidates(
            CandidatePlan {
                indexes: &indexes,
                requirement: CandidateRequirement::PotentialMatches,
                filter: &filter,
                source: CandidateSource {
                    store_meta: Some(&meta),
                    snapshot: SnapshotValidation::Validated,
                },
            },
            || panic!("base should not be called for a validated covered snapshot"),
        )
        .expect("candidates");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].rel_path(), Path::new("a/x.txt"));
}

#[test]
fn potential_matches_validated_snapshot_walks_when_filter_not_covered() {
    let tmp = TempDir::new().expect("tempdir");
    let corpus = tmp.path().join("corpus");
    make_parity_corpus(&corpus);
    let sift_dir = tmp.path().join(".sift");
    let indexes = build_indexes(&corpus, &sift_dir);

    let spec = QuerySpec {
        patterns: &["beta".to_string()],
        flags: QueryFlags::empty(),
    };
    let filter_config = CandidateFilterConfig {
        follow_links: true,
        ..default_filter_config()
    };
    let filter = CandidateFilter::new(&filter_config, &corpus).expect("filter");
    let meta = default_meta(&corpus);
    let base = candidates_from_paths(&corpus, &["a/x.txt", "b/y.txt", "new.txt"]);
    let result = QueryPlanner::new(spec)
        .candidates(
            CandidatePlan {
                indexes: &indexes,
                requirement: CandidateRequirement::PotentialMatches,
                filter: &filter,
                source: CandidateSource {
                    store_meta: Some(&meta),
                    snapshot: SnapshotValidation::Validated,
                },
            },
            || Ok(base),
        )
        .expect("candidates");
    let mut paths: Vec<_> = result.iter().map(|c| c.rel_path().to_path_buf()).collect();
    paths.sort();
    assert_eq!(
        paths,
        vec![
            PathBuf::from("a/x.txt"),
            PathBuf::from("b/y.txt"),
            PathBuf::from("new.txt")
        ]
    );
}

#[test]
fn potential_matches_stale_complete_snapshot_falls_back_to_base() {
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
    let meta = default_meta(&corpus);
    let base = candidates_from_paths(&corpus, &["a/x.txt", "b/y.txt", "new.txt"]);
    let result = QueryPlanner::new(spec)
        .candidates(
            CandidatePlan {
                indexes: &indexes,
                requirement: CandidateRequirement::PotentialMatches,
                filter: &filter,
                source: CandidateSource {
                    store_meta: Some(&meta),
                    snapshot: SnapshotValidation::Stale,
                },
            },
            || Ok(base),
        )
        .expect("candidates");
    let mut paths: Vec<_> = result.iter().map(|c| c.rel_path().to_path_buf()).collect();
    paths.sort();
    assert_eq!(
        paths,
        vec![
            PathBuf::from("a/x.txt"),
            PathBuf::from("b/y.txt"),
            PathBuf::from("new.txt")
        ]
    );
}

#[test]
fn potential_matches_validated_snapshot_walks_when_partial_index_does_not_cover_default_scope() {
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
    let mut meta = default_meta(&corpus);
    meta.corpus.include_paths = vec![PathBuf::from("a/x.txt")];
    let base = candidates_from_paths(&corpus, &["a/x.txt", "b/y.txt", "new.txt"]);
    let result = QueryPlanner::new(spec)
        .candidates(
            CandidatePlan {
                indexes: &indexes,
                requirement: CandidateRequirement::PotentialMatches,
                filter: &filter,
                source: CandidateSource {
                    store_meta: Some(&meta),
                    snapshot: SnapshotValidation::Validated,
                },
            },
            || Ok(base),
        )
        .expect("candidates");
    let mut paths: Vec<_> = result.iter().map(|c| c.rel_path().to_path_buf()).collect();
    paths.sort();
    assert_eq!(
        paths,
        vec![
            PathBuf::from("a/x.txt"),
            PathBuf::from("b/y.txt"),
            PathBuf::from("new.txt")
        ]
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
    let filter = default_filter(&corpus);
    let base = candidates_from_paths(&corpus, &["a/x.txt", "b/y.txt"]);
    let result = QueryPlanner::new(spec)
        .candidates(
            CandidatePlan {
                indexes: &indexes,
                requirement: CandidateRequirement::Complete,
                filter: &filter,
                source: CandidateSource {
                    store_meta: None,
                    snapshot: SnapshotValidation::Unvalidated,
                },
            },
            || Ok(base),
        )
        .expect("candidates");
    assert_eq!(result.len(), 2);
}

#[test]
fn complete_lazy_snapshot_falls_back_to_base() {
    let tmp = TempDir::new().expect("tempdir");
    let corpus = tmp.path().join("corpus");
    make_parity_corpus(&corpus);
    let sift_dir = tmp.path().join(".sift");
    let indexes = build_indexes(&corpus, &sift_dir);
    fs::write(corpus.join("new.txt"), "offline marker\n").expect("write new file");

    let spec = QuerySpec {
        patterns: &["beta".to_string()],
        flags: QueryFlags::empty(),
    };
    let filter = default_filter(&corpus);
    let base = candidates_from_paths(&corpus, &["a/x.txt", "b/y.txt", "new.txt"]);
    let meta = StoreMeta {
        coverage: IndexCoverage::Lazy,
        ..default_meta(&corpus)
    };
    let result = QueryPlanner::new(spec)
        .candidates(
            CandidatePlan {
                indexes: &indexes,
                requirement: CandidateRequirement::Complete,
                filter: &filter,
                source: CandidateSource {
                    store_meta: Some(&meta),
                    snapshot: SnapshotValidation::Unvalidated,
                },
            },
            || Ok(base),
        )
        .expect("candidates");
    let paths: Vec<_> = result.iter().map(|c| c.rel_path().to_path_buf()).collect();
    assert_eq!(
        paths,
        vec![
            PathBuf::from("a/x.txt"),
            PathBuf::from("b/y.txt"),
            PathBuf::from("new.txt")
        ]
    );
}

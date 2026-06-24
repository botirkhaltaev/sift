//! Composable indexed code search engine.
//!
//! `sift-core` builds on-disk indexes over codebases and uses them to narrow
//! candidate files before running the full regex engine. The index layer is
//! designed for multiple coexisting index types: each type independently
//! narrows candidates, and the [`Indexes`] registry intersects their results.
//!
//! Today the shipped index type is a trigram index ([`TrigramIndex`]), which
//! records overlapping 3-byte sequences and achieves up to 60x speedup over
//! ripgrep on indexed queries. Additional index types (AST indexes, dependency
//! graphs, vector indexes) can be added by extending the [`IndexKind`] and
//! [`Index`] enums.
//!
//! # Architecture
//!
//! ```text
//! IndexStore::build(kinds) -> snapshot with index artifacts
//! Indexes::open(sift_dir)  -> registry of opened indexes
//! QueryPlanner::candidates -> intersect index candidate sets
//! SearchQuery::run          -> regex scan over narrowed candidates
//! ```
//!
//! **Walking:** [`WalkBuilder`] from the [`ignore`] crate.

pub mod candidate;
pub mod grep;
pub use grep::GrepRun;
mod index;
pub mod query;
pub mod search;

pub use candidate::Candidate;

pub use search::{SearchError, SearchOutcome, SearchQuery};

pub use ignore::{Walk, WalkBuilder};

pub use index::config::IndexWalkConfig;
pub use index::meta::StoreMeta;
pub use index::store::IndexStore;
pub use index::trigram::{TrigramIndex, TrigramIndexError};
pub use index::{
    CorpusKind, CorpusMeta, CorpusSpec, FileId, FilterMeta, Index, IndexConfig, IndexError,
    IndexId, IndexKind, Indexes, PlanMode, QueryPlanOutput, ReconcileOutcome, SnapshotId, WalkMeta,
};

pub use query::{
    CandidatePlan, CandidateRequirement, CandidateSource, QueryFlags, QueryPlanner, QuerySpec,
    SnapshotValidation, UnindexedPolicy,
};

use thiserror::Error;

pub const SIFT_DIR: &str = ".sift";
pub const FILES_BIN: &str = "files.bin";
pub const LEXICON_BIN: &str = "lexicon.bin";
pub const POSTINGS_BIN: &str = "postings.bin";
pub const TRIGRAMS_BIN: &str = "trigrams.bin";

/// Top-level umbrella error for all core operations.
#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Index(#[from] IndexError),

    #[error(transparent)]
    Search(#[from] SearchError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("ignore walk error: {0}")]
    Ignore(#[from] ignore::Error),

    #[error("regex error: {0}")]
    Regex(#[from] Box<regex_automata::meta::BuildError>),
}

impl From<crate::index::trigram::TrigramIndexError> for Error {
    fn from(e: crate::index::trigram::TrigramIndexError) -> Self {
        Self::Index(IndexError::Trigram(e))
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::{CaseMode, SearchOptions, VisibilityConfig};
    use std::fs;
    use tempfile::TempDir;

    fn build_trigram_in_tmp(tmp: &TempDir, corpus_path: &std::path::Path) -> TrigramIndex {
        let sift_dir = tmp.path().join(".sift");
        let trigram_dir = sift_dir.join("trigram");
        let config = IndexConfig {
            corpus: CorpusSpec {
                root: corpus_path,
                kind: CorpusKind::Directory,
                follow_links: false,
                include_paths: &[],
                exclude_paths: &[],
            },
            walk: IndexWalkConfig::new(false),
            visibility: VisibilityConfig::default(),
        };
        TrigramIndex::build(&config, &trigram_dir, &[]).expect("build index")
    }

    fn build_index_in_tmp(tmp: &TempDir, corpus_path: &std::path::Path) -> Index {
        Index::Trigram(build_trigram_in_tmp(tmp, corpus_path))
    }

    #[test]
    fn build_open_search_finds_line() {
        let tmp = TempDir::new().expect("create temp dir");
        let corpus = tmp.path().join("corpus");
        fs::create_dir_all(corpus.join("src")).expect("create src dir");
        fs::write(corpus.join("src/lib.rs"), "fn hello() {\n  let x = 1;\n}\n")
            .expect("write test file");

        let tri = build_trigram_in_tmp(&tmp, &corpus);
        assert!(
            tri.file_path(FileId::new(0)).is_some(),
            "should have indexed files"
        );
        let index = Index::Trigram(tri);

        let pat = vec![r"let\s+x".to_string()];
        let q = SearchQuery::new(&pat, SearchOptions::default()).expect("compile search");
        let hits = q.collect_index_matches(&index).expect("search");
        assert_eq!(hits.len(), 1);
        assert!(hits[0].file.ends_with("src/lib.rs"));
        assert_eq!(hits[0].line, 2);
    }

    #[test]
    fn indexed_search_matches_walk_search_for_literal() {
        let tmp = TempDir::new().expect("create temp dir");
        let corpus = tmp.path().join("corpus");
        fs::create_dir_all(corpus.join("a")).expect("create dir a");
        fs::create_dir_all(corpus.join("b")).expect("create dir b");
        fs::write(corpus.join("a/x.txt"), "alpha beta\n").expect("write file a");
        fs::write(corpus.join("b/y.txt"), "gamma delta\n").expect("write file b");

        let index = build_index_in_tmp(&tmp, &corpus);

        let pat = vec!["beta".to_string()];
        let opts = SearchOptions::default();
        let q = SearchQuery::new(&pat, opts).expect("compile search");
        let naive = q.collect_walk_matches(&corpus).expect("walk search");
        let hits = q.collect_index_matches(&index).expect("index search");
        assert_eq!(hits, naive);
    }

    #[test]
    fn full_scan_parallel_candidate_path_finds_all_files() {
        let tmp = TempDir::new().expect("create temp dir");
        let corpus = tmp.path().join("corpus");
        fs::create_dir_all(corpus.join("d")).expect("create dir");
        let n_files = 8;
        for i in 0..n_files {
            fs::write(
                corpus.join("d").join(format!("f{i}.txt")),
                format!("line {i} needle\n"),
            )
            .expect("write file");
        }

        let tri = build_trigram_in_tmp(&tmp, &corpus);
        for i in 0..n_files {
            assert!(
                tri.file_path(FileId::new(i)).is_some(),
                "file {i} should be indexed"
            );
        }
        let index = Index::Trigram(tri);

        let pat = vec!["needle".to_string()];
        let q = SearchQuery::new(&pat, SearchOptions::default()).expect("compile search");
        let hits = q.collect_index_matches(&index).expect("search");
        assert_eq!(hits.len(), n_files);
    }

    #[test]
    fn index_and_walk_return_same_matches_for_literal_pattern() {
        let tmp = TempDir::new().expect("create temp dir");
        let corpus = tmp.path().join("corpus");
        fs::create_dir_all(corpus.join("keep")).expect("create keep dir");
        fs::write(corpus.join("keep/a.txt"), "one\ntwo beta\n").expect("write file a");
        fs::write(corpus.join("keep/b.txt"), "three\n").expect("write file b");

        let index = build_index_in_tmp(&tmp, &corpus);

        let pat = vec!["one".to_string(), "three".to_string()];
        let q = SearchQuery::new(&pat, SearchOptions::default()).expect("compile search");
        let mut from_index = q.collect_index_matches(&index).expect("index search");
        let mut from_walk = q.collect_walk_matches(&corpus).expect("walk search");
        from_index.sort_by(|a, b| (&a.file, a.line, &a.text).cmp(&(&b.file, b.line, &b.text)));
        from_walk.sort_by(|a, b| (&a.file, a.line, &a.text).cmp(&(&b.file, b.line, &b.text)));
        assert_eq!(from_index, from_walk);
    }

    #[test]
    fn multi_pattern_search_matches_either() {
        let tmp = TempDir::new().expect("create temp dir");
        let corpus = tmp.path().join("corpus");
        fs::create_dir_all(&corpus).expect("create corpus");
        fs::write(corpus.join("a.txt"), "hello world\nfoo bar\n").expect("write file");

        let index = build_index_in_tmp(&tmp, &corpus);

        let pat = vec!["hello".to_string(), "foo".to_string()];
        let q = SearchQuery::new(&pat, SearchOptions::default()).expect("compile search");
        let hits = q.collect_index_matches(&index).expect("search");
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn case_insensitive_search_matches_variants() {
        let tmp = TempDir::new().expect("create temp dir");
        let corpus = tmp.path().join("corpus");
        fs::create_dir_all(&corpus).expect("create corpus");
        fs::write(corpus.join("a.txt"), "Hello WORLD\n").expect("write file");

        let index = build_index_in_tmp(&tmp, &corpus);

        let pat = vec!["hello".to_string()];
        let opts = SearchOptions {
            case_mode: CaseMode::Insensitive,
            ..SearchOptions::default()
        };
        let q = SearchQuery::new(&pat, opts).expect("compile search");
        let hits = q.collect_index_matches(&index).expect("search");
        assert_eq!(hits.len(), 1);
    }
}

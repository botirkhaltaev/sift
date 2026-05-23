//! Fast indexed regex search over codebases — core engine.
//!
//! **Walking:** [`WalkBuilder`] from the [`ignore`] crate.

mod grep;
mod index;
mod query;

pub use grep::{
    BinaryMode, CandidateInfo, CaseMode, ColorChoice, CompiledSearch, FilenameMode, GlobConfig,
    HiddenMode, IgnoreConfig, IgnoreSources, LineStyleFlags, Match, OutputEmission, PathDisplay,
    PatternCompiler, SearchError, SearchFilter, SearchFilterConfig, SearchLineStyle,
    SearchMatchFlags, SearchMode, SearchOptions, SearchOutput, SearchOutputFormat,
    SearchRecordStyle, SearchSeparators, SearchStats, TypeDef, VisibilityConfig, compile_pattern,
    compile_search_pattern, pattern_branch, walk_file_paths,
};

pub use ignore::{Walk, WalkBuilder};

pub use index::trigram::{TrigramIndex, TrigramIndexBuilder, TrigramIndexError};
pub use index::{FileId, IndexError, IndexId, IndexMeta, Indexes, QueryPlanOutput, SearchIndex};

pub use query::{QueryFlags, QueryPlanner, QuerySpec};

use thiserror::Error;

pub const SIFT_DIR: &str = ".sift";
pub const INDEX_SUBDIR: &str = ".index";
pub const META_FILENAME: &str = "sift.meta";
pub const FILES_BIN: &str = "files.bin";
pub const LEXICON_BIN: &str = "lexicon.bin";
pub const POSTINGS_BIN: &str = "postings.bin";

/// Top-level umbrella error for all core operations.
///
/// Concrete domain errors are defined in their respective modules
/// and aggregated here. Prefer handling module-specific errors when
/// calling module APIs directly.
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
    use std::fs;
    use tempfile::TempDir;

    fn build_index_in_tmp(tmp: &TempDir, corpus_path: &std::path::Path) -> TrigramIndex {
        let sift_dir = tmp.path().join(".sift");
        let trigram_dir = sift_dir.join("trigram");
        TrigramIndexBuilder::new(corpus_path)
            .with_dir(&trigram_dir)
            .build()
            .expect("build index")
    }

    #[test]
    fn build_open_search_finds_line() {
        let tmp = TempDir::new().expect("create temp dir");
        let corpus = tmp.path().join("corpus");
        fs::create_dir_all(corpus.join("src")).expect("create src dir");
        fs::write(corpus.join("src/lib.rs"), "fn hello() {\n  let x = 1;\n}\n")
            .expect("write test file");

        let index = build_index_in_tmp(&tmp, &corpus);
        assert!(index.file_count() > 0);

        let pat = vec![r"let\s+x".to_string()];
        let q = CompiledSearch::new(&pat, SearchOptions::default()).expect("compile search");
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
        let q = CompiledSearch::new(&pat, opts).expect("compile search");
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

        let index = build_index_in_tmp(&tmp, &corpus);
        assert_eq!(index.file_count(), n_files);

        let pat = vec!["needle".to_string()];
        let q = CompiledSearch::new(&pat, SearchOptions::default()).expect("compile search");
        let hits = q.collect_index_matches(&index).expect("search");
        assert_eq!(hits.len(), n_files);
    }

    #[test]
    fn full_scan_uses_files_bin_same_hits_as_fresh_walk() {
        let tmp = TempDir::new().expect("create temp dir");
        let corpus = tmp.path().join("corpus");
        fs::create_dir_all(corpus.join("keep")).expect("create keep dir");
        fs::write(corpus.join("keep/a.txt"), "one\ntwo beta\n").expect("write file a");
        fs::write(corpus.join("keep/b.txt"), "three\n").expect("write file b");
        fs::write(corpus.join(".ignore"), "ignored\n").expect("write ignore");
        fs::create_dir_all(corpus.join("ignored")).expect("create ignored dir");
        fs::write(corpus.join("ignored/hidden.txt"), "beta skip\n").expect("write ignored file");

        let index = build_index_in_tmp(&tmp, &corpus);

        let pat = vec![".*".to_string()];
        let q = CompiledSearch::new(&pat, SearchOptions::default()).expect("compile search");
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
        let q = CompiledSearch::new(&pat, SearchOptions::default()).expect("compile search");
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
        let q = CompiledSearch::new(&pat, opts).expect("compile search");
        let hits = q.collect_index_matches(&index).expect("search");
        assert_eq!(hits.len(), 1);
    }
}

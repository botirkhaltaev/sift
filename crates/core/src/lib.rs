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
pub use index::{FileId, IndexError, IndexId, Indexes, QueryPlanOutput, SearchIndex};

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

    fn normalized_path(p: &std::path::Path) -> std::path::PathBuf {
        let s = p.display().to_string();
        #[cfg(windows)]
        let s = s.strip_prefix("\\\\?\\").unwrap_or(&s).to_string();
        #[cfg(target_os = "macos")]
        let s = s.replace("/private", "");
        std::path::PathBuf::from(s)
    }

    #[test]
    fn build_open_search_finds_line() {
        let tmp = std::env::temp_dir().join(format!("sift-core-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("src")).unwrap();
        fs::write(tmp.join("src/lib.rs"), "fn hello() {\n  let x = 1;\n}\n").unwrap();

        let sift_dir = tmp.join(".sift");
        let trigram_dir = sift_dir.join("trigram");
        let _ = TrigramIndexBuilder::new(&tmp)
            .with_dir(&trigram_dir)
            .build()
            .unwrap();

        let indexes = Indexes::open(&sift_dir).unwrap();
        assert!(!indexes.is_empty());
        let index_slice = indexes.refs();
        let index = index_slice[0];
        assert!(index.file_count() > 0);
        let pat = vec![r"let\s+x".to_string()];
        let q = CompiledSearch::new(&pat, SearchOptions::default()).unwrap();
        let hits = q.collect_index_matches(index).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].file.ends_with("src/lib.rs"));
        assert_eq!(hits[0].line, 2);
    }

    #[test]
    fn open_missing_meta_errors() {
        let tmp = std::env::temp_dir().join(format!("sift-missing-meta-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let indexes = Indexes::open(&tmp).unwrap();
        assert!(indexes.is_empty());
    }

    #[test]
    fn open_missing_table_errors() {
        let tmp = std::env::temp_dir().join(format!("sift-missing-table-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let trigram_dir = tmp.join("trigram");
        fs::create_dir_all(&trigram_dir).unwrap();
        let root_path = std::env::temp_dir().join("sift-test-root");
        let meta = crate::index::IndexMeta {
            root: root_path,
            is_single_file_corpus: false,
        };
        fs::write(
            trigram_dir.join(META_FILENAME),
            serde_json::to_string_pretty(&meta).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            Indexes::open(&tmp),
            Err(IndexError::Trigram(TrigramIndexError::MissingComponent(_)))
        ));
    }

    #[test]
    fn open_empty_meta_errors() {
        let tmp = std::env::temp_dir().join(format!("sift-empty-meta-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let trigram_dir = tmp.join("trigram");
        fs::create_dir_all(&trigram_dir).unwrap();
        fs::write(trigram_dir.join(META_FILENAME), "").unwrap();
        assert!(matches!(
            Indexes::open(&tmp),
            Err(IndexError::Trigram(TrigramIndexError::InvalidMeta(_)))
        ));
    }

    #[test]
    fn explain_returns_indexed_plan_for_literal_prefix() {
        let tmp = std::env::temp_dir().join(format!("sift-explain-indexed-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("a.txt"), "alpha beta\ngamma delta\n").unwrap();
        let sift_dir = tmp.join(".sift");
        let trigram_dir = sift_dir.join("trigram");
        let _ = TrigramIndexBuilder::new(&tmp)
            .with_dir(&trigram_dir)
            .build()
            .unwrap();
        let index = TrigramIndex::open(&trigram_dir).unwrap();
        let plan = index.explain("foo.*");
        assert_eq!(plan.pattern, "foo.*");
        assert_eq!(plan.mode, "indexed_candidates");
    }

    #[test]
    fn explain_returns_full_scan_for_true_no_literal() {
        let tmp =
            std::env::temp_dir().join(format!("sift-explain-fullscan-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("a.txt"), "alpha beta\ngamma delta\n").unwrap();
        let sift_dir = tmp.join(".sift");
        let trigram_dir = sift_dir.join("trigram");
        let _ = TrigramIndexBuilder::new(&tmp)
            .with_dir(&trigram_dir)
            .build()
            .unwrap();
        let index = TrigramIndex::open(&trigram_dir).unwrap();
        let plan = index.explain(r"\w{5}\s+\w{5}");
        assert_eq!(plan.pattern, r"\w{5}\s+\w{5}");
        assert_eq!(plan.mode, "full_scan");
    }

    #[test]
    fn indexed_search_matches_naive_for_literal() {
        let tmp = std::env::temp_dir().join(format!("sift-idx-parity-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("a")).unwrap();
        fs::create_dir_all(tmp.join("b")).unwrap();
        fs::write(tmp.join("a/x.txt"), "alpha beta\n").unwrap();
        fs::write(tmp.join("b/y.txt"), "gamma delta\n").unwrap();

        let sift_dir = tmp.join(".sift");
        let trigram_dir = sift_dir.join("trigram");
        let _ = TrigramIndexBuilder::new(&tmp)
            .with_dir(&trigram_dir)
            .build()
            .unwrap();
        let indexes = Indexes::open(&sift_dir).unwrap();
        let index_slice = indexes.refs();
        let index = index_slice[0];

        let pat = vec!["beta".to_string()];
        let opts = SearchOptions::default();
        let q = CompiledSearch::new(&pat, opts).unwrap();
        let naive = q.collect_walk_matches(&tmp).unwrap();
        let hits = q.collect_index_matches(index).unwrap();
        assert_eq!(hits, naive);
    }

    #[test]
    fn full_scan_parallel_candidate_path_finds_all_files() {
        let tmp = std::env::temp_dir().join(format!("sift-parallel-fs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("d")).unwrap();
        let n_files = 8;
        for i in 0..n_files {
            fs::write(
                tmp.join("d").join(format!("f{i}.txt")),
                format!("line {i} needle\n"),
            )
            .unwrap();
        }
        let sift_dir = tmp.join(".sift");
        let trigram_dir = sift_dir.join("trigram");
        let _ = TrigramIndexBuilder::new(&tmp)
            .with_dir(&trigram_dir)
            .build()
            .unwrap();
        let indexes = Indexes::open(&sift_dir).unwrap();
        let index_slice = indexes.refs();
        let index = index_slice[0];
        assert_eq!(index.file_count(), n_files);

        let pat = vec!["needle".to_string()];
        let opts = SearchOptions::default();
        let q = CompiledSearch::new(&pat, opts).unwrap();
        let hits = q.collect_index_matches(index).unwrap();
        assert_eq!(hits.len(), n_files);
    }

    #[test]
    fn full_scan_uses_files_bin_same_hits_as_fresh_walk() {
        let tmp = std::env::temp_dir().join(format!("sift-fullscan-parity-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("keep")).unwrap();
        fs::write(tmp.join("keep/a.txt"), "one\ntwo beta\n").unwrap();
        fs::write(tmp.join("keep/b.txt"), "three\n").unwrap();
        fs::write(tmp.join(".ignore"), "ignored\n").unwrap();
        fs::create_dir_all(tmp.join("ignored")).unwrap();
        fs::write(tmp.join("ignored/hidden.txt"), "beta skip\n").unwrap();

        let sift_dir = tmp.join(".sift");
        let trigram_dir = sift_dir.join("trigram");
        let _ = TrigramIndexBuilder::new(&tmp)
            .with_dir(&trigram_dir)
            .build()
            .unwrap();
        let indexes = Indexes::open(&sift_dir).unwrap();
        let index_slice = indexes.refs();
        let index = index_slice[0];

        let pat = vec![".*".to_string()];
        let opts = SearchOptions::default();
        let q = CompiledSearch::new(&pat, opts).unwrap();
        let mut from_index = q.collect_index_matches(index).unwrap();
        let mut from_walk = q.collect_walk_matches(&tmp).unwrap();
        from_index.sort_by(|a, b| (&a.file, a.line, &a.text).cmp(&(&b.file, b.line, &b.text)));
        from_walk.sort_by(|a, b| (&a.file, a.line, &a.text).cmp(&(&b.file, b.line, &b.text)));
        assert_eq!(from_index, from_walk);
    }

    #[test]
    fn build_open_single_file_search_finds_line() {
        let tmp = std::env::temp_dir().join(format!("sift-single-file-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let file = tmp.join("one.txt");
        fs::write(&file, "alpha\nbeta needle\n").unwrap();

        let sift_dir = tmp.join(".sift");
        let trigram_dir = sift_dir.join("trigram");
        let _ = TrigramIndexBuilder::new(&file)
            .with_dir(&trigram_dir)
            .build()
            .unwrap();
        let indexes = Indexes::open(&sift_dir).unwrap();
        let index_slice = indexes.refs();
        let index = index_slice[0];

        let expected_root = file.canonicalize().unwrap().parent().unwrap().to_path_buf();
        assert_eq!(
            normalized_path(indexes.root()),
            normalized_path(&expected_root)
        );
        assert!(index.is_single_file());
        assert_eq!(index.file_count(), 1);
        assert_eq!(
            index.file_path(FileId::new(0)).unwrap(),
            std::path::Path::new("one.txt")
        );

        let pat = vec!["needle".to_string()];
        let q = CompiledSearch::new(&pat, SearchOptions::default()).unwrap();
        let hits = q.collect_index_matches(index).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(
            normalized_path(&hits[0].file),
            normalized_path(&file.canonicalize().unwrap())
        );
        assert_eq!(hits[0].line, 2);
    }

    #[test]
    fn single_file_meta_is_json_with_explicit_kind() {
        let tmp =
            std::env::temp_dir().join(format!("sift-single-file-meta-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let file = tmp.join("one.txt");
        fs::write(&file, "alpha\n").unwrap();

        let sift_dir = tmp.join(".sift");
        let trigram_dir = sift_dir.join("trigram");
        let _ = TrigramIndexBuilder::new(&file)
            .with_dir(&trigram_dir)
            .build()
            .unwrap();
        let meta = fs::read_to_string(trigram_dir.join(META_FILENAME)).unwrap();

        assert!(meta.contains("\"root\""), "unexpected meta: {meta}");
        assert!(
            meta.contains("\"is_single_file_corpus\": true"),
            "unexpected meta: {meta}"
        );
    }
}

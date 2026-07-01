use std::borrow::Cow;

use sift_core::grep::{
    CandidateFilter, CandidateFilterConfig, CandidatePolicyConfig,
    CandidateScope, CorpusState, IndexFallback, Inputs, MatchOptions, Query, Session, StatsMode,
};
use tempfile::TempDir;

use super::common::{make_parity_corpus, open_indexes};

#[test]
fn grep_finds_match_in_indexed_corpus() {
    let tmp = TempDir::new().expect("tempdir");
    let corpus = tmp.path().join("corpus");
    make_parity_corpus(&corpus);

    let sift_dir = tmp.path().join(".sift");
    super::common::build_store(&corpus, &sift_dir);

    let indexes = open_indexes(&sift_dir);
    let filter = CandidateFilter::new(&CandidateFilterConfig::default(), &corpus).expect("filter");
    let query = Query::new(vec!["beta".to_string()])
        .expect("query")
        .options(MatchOptions::default());

    let session = Session::new(&indexes, &filter, None);
    let compiled = query.compile().expect("compile");
    let policy = CandidatePolicyConfig {
            output_scope: CandidateScope::Indexed,
            corpus: CorpusState::Indexed,
            fallback: IndexFallback::WalkOnStaleSnapshot,
            order: Default::default(),
        }
        .policy(compiled);
    let candidates = query.candidates(&session, policy).expect("candidates");
    let mut inputs = Inputs::with_capacity(candidates.len());
    for candidate in &candidates {
        inputs.push_path(candidate);
    }

    let report = query
        .search(&inputs, StatsMode::Off)
        .expect("grep run");
    assert!(report.matched());
}

#[test]
fn grep_finds_match_in_stdin_stream() {
    let query = Query::new(vec!["needle".to_string()])
        .expect("query")
        .options(MatchOptions::default());

    let mut inputs = Inputs::with_capacity(1);
    inputs.push_bytes(
        Cow::Borrowed("<stdin>"),
        Cow::Borrowed(b"hello needle world\n"),
        None,
    );

    let report = query
        .search(&inputs, StatsMode::Off)
        .expect("grep run");
    assert!(report.matched());
    assert_eq!(report.matches.len(), 1);
    assert!(report.matches[0].text.contains("needle"));
}

use sift_core::grep::{GrepRequest, run};
use sift_core::{
    CandidateFilter, CandidateFilterConfig, SearchOptions, SearchOutput, SearchQuery,
    SearchSeparators,
};
use tempfile::TempDir;

use super::common::{build_store, make_parity_corpus, open_indexes};

#[test]
fn grep_finds_match_in_indexed_corpus() {
    let tmp = TempDir::new().expect("tempdir");
    let corpus = tmp.path().join("corpus");
    make_parity_corpus(&corpus);

    let sift_dir = tmp.path().join(".sift");
    build_store(&corpus, &sift_dir);

    let indexes = open_indexes(&sift_dir);
    let filter = CandidateFilter::new(&CandidateFilterConfig::default(), &corpus).expect("filter");
    let query = SearchQuery::new(&["beta".to_string()], SearchOptions::default()).expect("query");
    let outcome = run(
        &query,
        &GrepRequest {
            indexes: &indexes,
            filter: &filter,
            output: SearchOutput::default(),
            separators: &SearchSeparators::default(),
            collect_stats: false,
        },
    )
    .expect("grep run");
    assert!(outcome.matched);
}

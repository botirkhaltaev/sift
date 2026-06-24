use sift_core::SearchQuery;
use sift_core::grep::GrepRequest;
use sift_core::search::{
    CandidateFilter, CandidateFilterConfig, SearchCollection, SearchOptions, SearchOutput,
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
    let grep_run = GrepRequest {
        indexes: &indexes,
        filter: &filter,
        output: SearchOutput::default(),
        separators: &SearchSeparators::default(),
        collect: SearchCollection::none(),
        store_meta: None,
        unindexed: sift_core::UnindexedStrategy::Skip,
    }
    .run(&query)
    .expect("grep run");
    assert!(grep_run.outcome.matched);
}

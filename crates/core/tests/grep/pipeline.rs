use sift_core::SnapshotValidation;
use sift_core::grep::{
    CandidateFilter, CandidateFilterConfig, GrepCollection, GrepOptions, GrepOutput, GrepSeparators,
};
use sift_core::grep::{CandidateIndexState, CandidateOrder, Grep, GrepCorpus, GrepQuery};
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
    let query = GrepQuery::new(vec!["beta".to_string()])
        .expect("query")
        .options(GrepOptions::default());
    let corpus = GrepCorpus::new(
        &indexes,
        &filter,
        CandidateIndexState {
            store_meta: None,
            snapshot: SnapshotValidation::Unvalidated,
        },
    )
    .order(CandidateOrder::default());
    let grep_run = Grep::new(query)
        .corpus(corpus)
        .output(GrepOutput::default())
        .separators(&GrepSeparators::default())
        .collect(GrepCollection::none())
        .run()
        .expect("grep run");
    assert!(grep_run.outcome.matched);
}

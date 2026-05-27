use std::path::Path;

use sift_core::{QueryFlags, QuerySpec};
use tempfile::TempDir;

use super::super::common::{build_trigram_in_dir, make_parity_corpus};

#[test]
fn literal_query_returns_indexed_candidates() {
    let tmp = TempDir::new().expect("tempdir");
    let corpus = tmp.path().join("corpus");
    make_parity_corpus(&corpus);

    let index = build_trigram_in_dir(&corpus, &tmp.path().join("trigram"));
    let spec = QuerySpec {
        patterns: &["beta".to_string()],
        flags: QueryFlags::empty(),
    };
    let candidates = index.candidates(&spec);
    assert!(!candidates.is_empty());
    assert!(
        candidates
            .iter()
            .any(|c| c.rel_path() == Path::new("a/x.txt"))
    );
}

use super::super::common::{build_indexes, dir_size, make_filter_corpus};

const FILTER_CORPUS_INDEX_MAX_BYTES: u64 = 512 * 1024;

#[test]
fn filter_corpus_index_size_within_budget() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    make_filter_corpus(tmp.path());
    let sift_dir = tmp.path().join(".sift");
    build_indexes(tmp.path(), &sift_dir);

    let size = dir_size(&sift_dir);
    assert!(
        size < FILTER_CORPUS_INDEX_MAX_BYTES,
        "index size {size} exceeds budget {FILTER_CORPUS_INDEX_MAX_BYTES}"
    );
}

use sift_core::StoreMeta;
use tempfile::TempDir;

use super::common::build_indexes;

#[test]
fn store_meta_written_on_create() {
    let tmp = TempDir::new().expect("tempdir");
    let corpus = tmp.path().join("corpus");
    std::fs::create_dir_all(&corpus).expect("mkdir");
    std::fs::write(corpus.join("f.txt"), "x\n").expect("write");

    let sift_dir = tmp.path().join(".sift");
    build_indexes(&corpus, &sift_dir);

    let meta = StoreMeta::read(&sift_dir).expect("read meta");
    assert_eq!(meta.corpus.root, corpus.canonicalize().unwrap_or(corpus));
}

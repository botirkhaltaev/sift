use std::fs;

use sift_core::{CorpusKind, IndexKind, IndexStore};
use tempfile::TempDir;

use super::common::standard_build_config;

#[test]
fn build_writes_current_snapshot() {
    let tmp = TempDir::new().expect("tempdir");
    let corpus = tmp.path().join("corpus");
    fs::create_dir_all(&corpus).expect("mkdir");
    fs::write(corpus.join("f.txt"), "data\n").expect("write");

    let sift_dir = tmp.path().join(".sift");
    let config = standard_build_config(&corpus, &[]);
    let mut store = IndexStore::open_or_create(
        &sift_dir,
        &corpus,
        CorpusKind::Directory,
        false,
        &[IndexKind::Trigram],
    )
    .expect("open");
    store.build(&[IndexKind::Trigram], &config).expect("build");

    let id = store.current_id().expect("snapshot id");
    assert!(!id.is_empty());
    let snap = store.snapshot_dir(id);
    assert!(snap.join("trigram").join("postings.bin").is_file());
}

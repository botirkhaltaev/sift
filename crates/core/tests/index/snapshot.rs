use std::fs;

use sift_core::{IndexKind, IndexStore, NGramKind};
use tempfile::TempDir;

use super::common::{sample_store_meta, standard_build_config};

#[test]
fn build_writes_current_snapshot() {
    let tmp = TempDir::new().expect("tempdir");
    let corpus = tmp.path().join("corpus");
    fs::create_dir_all(&corpus).expect("mkdir");
    fs::write(corpus.join("f.txt"), "data\n").expect("write");

    let sift_dir = tmp.path().join(".sift");
    let config = standard_build_config(&corpus, &[]);
    let corpus_path = corpus.clone();
    let root = corpus.canonicalize().unwrap_or(corpus_path);
    let meta = sample_store_meta(root, vec![IndexKind::NGram(NGramKind::Trigram)]);
    let mut store = IndexStore::open_or_create(&sift_dir, &meta).expect("open");
    store
        .build(&[IndexKind::NGram(NGramKind::Trigram)], &config, &[])
        .expect("build");

    let id = store.current_id().expect("snapshot id");
    assert!(!id.is_empty());
    let snap = store.snapshot_dir(id);
    assert!(snap.join("trigram").join("postings.bin").is_file());
    assert!(snap.join("trigram").join("grams.bin").is_file());
}

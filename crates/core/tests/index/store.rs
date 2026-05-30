use std::fs;

use sift_core::{CorpusKind, IndexKind, IndexStore, QueryFlags, QuerySpec};
use tempfile::TempDir;

use super::common::{build_store, open_indexes, standard_build_config};

#[test]
fn build_and_reopen_indexes() {
    let tmp = TempDir::new().expect("tempdir");
    let corpus = tmp.path().join("corpus");
    fs::create_dir_all(&corpus).expect("mkdir");
    fs::write(corpus.join("a.txt"), "hello world\n").expect("write");

    let sift_dir = tmp.path().join(".sift");
    build_store(&corpus, &sift_dir);

    let indexes = open_indexes(&sift_dir);
    assert!(!indexes.is_empty());
    let spec = QuerySpec {
        patterns: &["hello".to_string()],
        flags: QueryFlags::empty(),
    };
    let files = indexes.candidates(&spec).expect("candidates");
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].rel_path().as_os_str(), "a.txt");
}

#[test]
fn update_skips_rebuild_when_unchanged() {
    let tmp = TempDir::new().expect("tempdir");
    let corpus = tmp.path().join("corpus");
    fs::create_dir_all(&corpus).expect("mkdir");
    fs::write(corpus.join("f.txt"), "hello\n").expect("write");

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
    let id = store.current_id().expect("id").to_string();

    let changed = store
        .update(&[IndexKind::Trigram], &config)
        .expect("update");
    assert_eq!(changed, None, "expected no rebuild when corpus unchanged");
    assert_eq!(store.current_id().unwrap(), id);
}

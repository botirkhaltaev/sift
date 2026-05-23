#![no_main]

use libfuzzer_sys::fuzz_target;
use sift_core::{
    compile_search_pattern, CompiledSearch, SearchIndex, SearchMatchFlags, SearchOptions,
    TrigramIndex, TrigramIndexBuilder,
};
use std::fs;
use std::sync::OnceLock;

const MAX_PATTERN_LEN: usize = 512;

struct IndexHolder {
    _tmp: tempfile::TempDir,
    index: TrigramIndex,
}

static INDEX: OnceLock<IndexHolder> = OnceLock::new();

fn indexed() -> &'static TrigramIndex {
    &INDEX
        .get_or_init(|| {
            let tmp = tempfile::tempdir().expect("tempdir");
            let corpus = tmp.path().join("c");
            fs::create_dir_all(&corpus).expect("mkdir");
            fs::write(corpus.join("a.txt"), b"hello world\nfoo bar\n").expect("a.txt");
            fs::write(corpus.join("b.txt"), b"baz\nquux line\n").expect("b.txt");
            let index_dir = tmp.path().join(".sift");
            TrigramIndexBuilder::new(&corpus)
                .with_dir(&index_dir)
                .build()
                .expect("build_index");
            let index = TrigramIndex::open(&index_dir).expect("open");
            IndexHolder { _tmp: tmp, index }
        })
        .index
}

fn lossy_pattern(data: &[u8]) -> String {
    String::from_utf8_lossy(data)
        .chars()
        .take(MAX_PATTERN_LEN)
        .collect()
}

fn opts_from_bytes(data: &[u8]) -> SearchOptions {
    let flags = data
        .first()
        .map(|b| SearchMatchFlags::from_bits_truncate(*b))
        .unwrap_or_default();
    let max_results = data.get(1).map(|b| (*b as usize).min(10_000));
    SearchOptions {
        flags,
        max_results,
        ..SearchOptions::default()
    }
}

fuzz_target!(|data: &[u8]| {
    if data.len() < 2 {
        return;
    }

    let opts = opts_from_bytes(data);
    let index = indexed();

    let indexes: &[&dyn SearchIndex] = &[index];

    let pat1 = lossy_pattern(&data[2..]);
    if let Ok(q) = CompiledSearch::new(&[pat1], opts) {
        let _ = q.run_indexes(
            indexes,
            &sift_core::SearchFilter::new(&sift_core::SearchFilterConfig::default(), index.root()).unwrap(),
            sift_core::SearchOutput {
                emission: sift_core::OutputEmission::Quiet,
                ..sift_core::SearchOutput::default()
            },
            &sift_core::SearchSeparators::default(),
        );
    }

    if data.len() > 4 {
        let mid = 2 + (data.len() - 2) / 2;
        let p_a = lossy_pattern(&data[2..mid]);
        let p_b = lossy_pattern(&data[mid..]);
        if let Ok(q) = CompiledSearch::new(&[p_a, p_b], opts) {
            let _ = q.run_indexes(
                indexes,
                &sift_core::SearchFilter::new(&sift_core::SearchFilterConfig::default(), index.root()).unwrap(),
                sift_core::SearchOutput {
                    emission: sift_core::OutputEmission::Quiet,
                    ..sift_core::SearchOutput::default()
                },
                &sift_core::SearchSeparators::default(),
            );
        }
    }

    let p = lossy_pattern(&data[2..]);
    let _ = compile_search_pattern(&[p], &opts);
    if data.len() > 4 {
        let mid = 2 + (data.len() - 2) / 2;
        let p_a = lossy_pattern(&data[2..mid]);
        let p_b = lossy_pattern(&data[mid..]);
        let _ = compile_search_pattern(&[p_a, p_b], &opts);
    }
});

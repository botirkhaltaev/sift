#![no_main]

//! Fuzzes the posting-list decoder on untrusted bytes.
//!
//! A real index is built once, then each iteration overwrites `postings.bin`
//! with a valid container header wrapping arbitrary payload bytes and reopens
//! the index. Opening runs the lexicon/postings integrity check, which decodes
//! every posting list, so this exercises `Postings::decode_sorted` (block
//! headers, `num_bits`, bitpacked blocks, and the delta-varint tail) against
//! adversarial input. Decoding must always return an error rather than panic,
//! over-allocate, or read out of bounds.

use libfuzzer_sys::fuzz_target;
use sift_core::grep::VisibilityConfig;
use sift_core::{
    CorpusKind, CorpusSpec, GramWidth, IndexBuildConfig, IndexWalkConfig, Indexes, NGramConfig,
};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::{fs, io::Write};

struct Harness {
    _temp: tempfile::TempDir,
    sift_dir: PathBuf,
    postings: PathBuf,
    header: Vec<u8>,
}

static HARNESS: OnceLock<Harness> = OnceLock::new();

fn harness() -> &'static Harness {
    HARNESS.get_or_init(|| {
        let tmp = tempfile::tempdir().expect("tempdir");
        let corpus = tmp.path().join("c");
        fs::create_dir_all(&corpus).expect("mkdir");
        fs::write(corpus.join("a.txt"), b"hello world\nfoo bar\n").expect("a.txt");
        fs::write(corpus.join("b.txt"), b"baz\nquux line\n").expect("b.txt");
        let sift_dir = tmp.path().join(".sift");
        let trigram_dir = sift_dir.join("trigram");
        let config = IndexBuildConfig {
            corpus: CorpusSpec {
                root: &corpus,
                kind: CorpusKind::Directory,
                follow_links: false,
                include_paths: &[],
                exclude_paths: &[],
            },
            walk: IndexWalkConfig::new(false),
            visibility: VisibilityConfig::default(),
        };
        NGramConfig::new(GramWidth::TRIGRAM)
            .build(&config, &trigram_dir, &[])
            .expect("build_index");
        let postings = trigram_dir.join("postings.bin");
        // Reuse the real 8-byte magic so reopened blobs pass the header check
        // and reach the decoder.
        let magic = fs::read(&postings).expect("read postings")[..8].to_vec();
        Harness {
            _temp: tmp,
            sift_dir,
            postings,
            header: magic,
        }
    })
}

fuzz_target!(|data: &[u8]| {
    let h = harness();
    let Ok(len) = u32::try_from(data.len()) else {
        return;
    };
    let mut blob = Vec::with_capacity(h.header.len() + 4 + data.len());
    blob.extend_from_slice(&h.header);
    blob.extend_from_slice(&len.to_le_bytes());
    blob.extend_from_slice(data);

    let Ok(mut file) = fs::File::create(&h.postings) else {
        return;
    };
    if file.write_all(&blob).is_err() {
        return;
    }
    drop(file);

    let _ = Indexes::open(&h.sift_dir);
});

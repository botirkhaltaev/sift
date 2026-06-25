# index/ngram/

N-gram index implementation. The shipped index is `NGramIndex<TrigramSpec>`, a trigram-specialized N-gram index that maps overlapping 3-byte sequences to candidate files. The same module provides generic `NGramSpec` machinery for future widths or optimized specializations.

## How It Works

An N-gram index is an inverted index mapping each fixed-width byte sequence found in the corpus to the set of files that contain it. At query time, the planner extracts required literal sequences from the regex pattern, looks up their grams in the index, and intersects the resulting file sets to produce a narrow candidate list. Only those candidate files are scanned with the full regex engine.

`TrigramSpec` is the first specialization. It keeps optimized 24-bit gram packing and postings assembly while sharing lifecycle, storage, and query dispatch with the generic N-gram implementation.

## Modules

| File | Description |
|------|-------------|
| [`mod.rs`](mod.rs) | `NGramIndex<S>`, `NGramSpec`, `TrigramSpec`, candidate narrowing, build/open/update lifecycle, error type, and module exports |
| [`gram.rs`](gram.rs) | `GramWidth`, `GramKey`, `PackedGram<N>`, `Trigram`, gram window iteration |
| [`build.rs`](build.rs) | `IndexTables`: corpus walk, gram extraction, incremental table construction |
| [`files.rs`](files.rs) | File ID to relative path + fingerprint mapping |
| [`storage/`](storage/) | Binary persistence format (lexicon, postings, gram sets, file table) |

## On-Disk Format

Each table file starts with an 8-byte magic header:

| File | Magic | Contents |
|------|-------|----------|
| `files.bin` | `SIFTFIL1` | Offset table + length-prefixed UTF-8 paths with fingerprints (mtime, size) |
| `lexicon.bin` | `SIFTLEX2` | Width-aware sorted gram entries with postings offsets |
| `postings.bin` | `SIFTPST1` | Flat array of `u32` file IDs referenced by lexicon |
| `grams.bin` | `SIFTGRM1` | Per-file sorted unique gram sets for incremental rebuild |

All integers are little-endian. Width-bearing files reject mismatched gram widths at open time.

## API

```rust
use sift_core::{IndexConfig, IndexKind, IndexStore, NGramIndex, NGramKind, TrigramSpec};

// Build through IndexStore when working with stores or snapshots.
let mut store = IndexStore::open_or_create(&sift_dir, &meta)?;
store.build(&[IndexKind::NGram(NGramKind::Trigram)], &config, &paths)?;

// Build/open the concrete specialization directly in tests or lower-level code.
let index = NGramIndex::build(TrigramSpec, &config, &index_dir, &paths)?;
let reopened = NGramIndex::open(TrigramSpec, &index_dir, &root, corpus_kind)?;
```

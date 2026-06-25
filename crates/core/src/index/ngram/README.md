# index/ngram/

Runtime-width N-gram index implementation. The default configured index is `ngram-3`, which maps overlapping 3-byte sequences to candidate files, but the implementation is width-aware and can build/open other configured widths.

## How It Works

An N-gram index is an inverted index mapping each fixed-width byte sequence found in the corpus to the set of files that contain it. At query time, the planner extracts required literal sequences from the regex pattern, looks up their grams in the index, and intersects the resulting file sets to produce a narrow candidate list. Only those candidate files are scanned with the full regex engine.

`Config` records the configured gram width. `Index` is the opened runtime handle backed by memory-mapped storage.

## Modules

| File | Description |
|------|-------------|
| [`mod.rs`](mod.rs) | `Config`, `Index`, candidate narrowing, build/open/update lifecycle, error type, and module exports |
| [`gram.rs`](gram.rs) | `GramWidth`, `Gram`, runtime-width gram window iteration |
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
use sift_core::{GramWidth, IndexBuildConfig, IndexConfig, IndexStore, NGramConfig};

// Build through IndexStore when working with stores or snapshots.
let mut store = IndexStore::open_or_create(&sift_dir, &meta)?;
store.build(&[IndexConfig::ngram(GramWidth::TRIGRAM)], &config, &paths)?;

// Build/open the concrete N-gram family directly in tests or lower-level code.
let ngram = NGramConfig::new(GramWidth::TRIGRAM);
let index = ngram.build(&config, &index_dir, &paths)?;
let reopened = NGramConfig::open(GramWidth::TRIGRAM, &index_dir, &root, corpus_kind)?;
```

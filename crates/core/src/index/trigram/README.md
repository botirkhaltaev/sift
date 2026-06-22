# index/trigram/

Trigram index -- the first shipped index type in Sift. Walks the corpus, extracts overlapping 3-byte sequences, writes persistence files, and provides zero-copy memory-mapped access for queries.

## How It Works

A trigram index is an inverted index mapping every 3-byte sequence found in the corpus to the set of files that contain it. At query time, the planner extracts required literal sequences from the regex pattern, looks up their trigrams in the index, and intersects the resulting file sets to produce a narrow candidate list. Only those candidate files are scanned with the full regex engine.

## Modules

| File | Description |
|------|-------------|
| [`mod.rs`](mod.rs) | `TrigramIndex` struct, posting list intersection, `Index` impl |
| [`builder.rs`](builder.rs) | `IndexTableBuilder`: corpus walk, trigram extraction, incremental table construction |
| [`lifecycle.rs`](lifecycle.rs) | Build, open, update, and persist lifecycle for `TrigramIndex` |
| [`file_table.rs`](file_table.rs) | `MappedFilesView`, `FileFingerprint`: file ID to relative path + fingerprint mapping |
| [`storage/`](storage/) | Binary persistence format (lexicon, postings, trigram sets, mmap, format constants) |

## On-Disk Format

Each table file starts with an 8-byte magic header:

| File | Magic | Contents |
|------|-------|----------|
| `files.bin` | `SIFTFIL1` | Offset table + length-prefixed UTF-8 paths with fingerprints (mtime, size) |
| `lexicon.bin` | `SIFTLEX1` | Sorted trigram entries with postings offsets |
| `postings.bin` | `SIFTPST1` | Flat array of `u32` file IDs referenced by lexicon |
| `trigrams.bin` | `SIFTTRI1` | Per-file sorted unique trigram sets for incremental rebuild |

All integers are little-endian.

## API

```rust
use sift_core::{TrigramIndex, IndexConfig, IndexKind};

// Build (via IndexKind lifecycle -- preferred)
IndexKind::Trigram.build(&config, dest, &paths)?;

// Open
let index = TrigramIndex::open_tables(source, &root, corpus_kind)?;

// Query
let candidates = index.candidates(&query_spec);
```

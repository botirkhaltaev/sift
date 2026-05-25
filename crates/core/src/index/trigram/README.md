# index/trigram/

Trigram index construction and in-memory index handle. Walks the corpus, extracts trigrams, writes persistence files, and provides zero-copy access for queries.

## Modules

| File | Description |
|------|-------------|
| [`mod.rs`](mod.rs) | `TrigramIndex` struct, posting list intersection, `Index` impl |
| [`builder.rs`](builder.rs) | `TrigramIndexBuilder`, `IndexTableBuilder` — corpus walk, trigram extraction, incremental table construction |
| [`file_table.rs`](file_table.rs) | `MappedFilesView`, `FileFingerprint` — file ID → relative path + fingerprint mapping |
| [`storage/`](storage/) | Binary persistence format (lexicon, postings, trigram sets, mmap, format constants) |

## API

```rust
use sift_core::{TrigramIndex, TrigramIndexBuilder};

// Build
let index = TrigramIndexBuilder::new(&corpus_root).with_dir(&index_dir).build()?;

// Open
let index = TrigramIndex::open(&index_dir, &root, corpus_kind)?;

// Query
let path = index.file_path(FileId::new(0));
```

## Format

Each table file starts with an 8-byte magic header:

| File | Magic | Contents |
|------|-------|----------|
| `files.bin` | `SIFTFIL1` | Offset table + length-prefixed UTF-8 paths with fingerprints (mtime, size) |
| `lexicon.bin` | `SIFTLEX1` | Sorted trigram entries with postings offsets |
| `postings.bin` | `SIFTPST1` | Flat array of `u32` file IDs referenced by lexicon |
| `trigrams.bin` | `SIFTTRI1` | Per-file sorted unique trigram sets for incremental rebuild |

All integers are little-endian.

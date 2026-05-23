# index/trigram/

Trigram index construction and in-memory index handle. Walks the corpus, extracts trigrams, writes persistence files, and provides zero-copy access for queries.

## Modules

| File | Description |
|------|-------------|
| [`mod.rs`](mod.rs) | `TrigramIndex` struct, posting list intersection, `Index`/`CandidateSource` impls |
| [`builder.rs`](builder.rs) | `TrigramIndexBuilder` — corpus walk, trigram extraction, table construction |
| [`file_table.rs`](file_table.rs) | `MappedFilesView` — file ID → relative path mapping |
| [`storage/`](storage/) | Binary persistence format (lexicon, postings, mmap, format constants) |

## API

```rust
use sift_core::{TrigramIndex, TrigramIndexBuilder};

// Build
let index = TrigramIndexBuilder::new(&corpus_root).with_dir(&index_dir).build()?;

// Open
let index = TrigramIndex::open(&index_dir)?;

// Query
let file_count = index.file_count();
let path = index.file_path(FileId::new(0));
```

## Format

Each table file starts with an 8-byte magic header:

| File | Magic | Contents |
|------|-------|----------|
| `files.bin` | `SIFTFIL2` | Offset table + length-prefixed UTF-8 paths |
| `lexicon.bin` | `SIFTLEX1` | Sorted trigram entries with postings offsets |
| `postings.bin` | `SIFTPST1` | Flat array of `u32` file IDs referenced by lexicon |

All integers are little-endian.

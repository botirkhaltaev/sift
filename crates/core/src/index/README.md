# index/

Generic index traits and concrete index implementations. The `SearchIndex` trait defines how any index narrows candidates for a query; concrete implementations live in subdirectories.

## Modules

| Module | Description |
|--------|-------------|
| [`mod.rs`](mod.rs) | `SearchIndex` trait, `Indexes` registry, `FileId`, `CorpusKind`, `IndexMeta` |
| [`trigram/`](trigram/) | Trigram index: build, load, search, and persistence |

## API

```rust
use sift_core::{Indexes, SearchIndex, FileId, TrigramIndex, TrigramIndexBuilder};

// Build (using the shipped trigram index)
let index = TrigramIndexBuilder::new(&corpus_root).with_dir(&index_dir).build()?;

// Open
let index = TrigramIndex::open(&index_dir)?;

// Concrete index methods
let path = index.file_path(FileId::new(0));
```

## Pluggable Index Architecture

The `SearchIndex` trait abstracts over any index kind. Each implementation decides how to narrow candidates for a given `QuerySpec`. The `Indexes` registry opens all available index kinds under a `.sift` directory and intersects their candidate sets at query time, so multiple indexes together produce tighter narrowing than any single index alone.

```text
index/
  trigram/     -- shipped trigram-based index
  (future)     -- suffix array, symbol table, or other workload-tuned indexes
```

Each kind implements `SearchIndex` and lives as a sibling of `trigram/`.

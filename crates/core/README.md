# sift-core

Indexed grep-style search engine. Build a trigram index on disk, then run regex or fixed-string queries with automatic candidate narrowing.

## Modules

| Module | Description |
|--------|-------------|
| [`query/`](src/query/) | Query description (`QuerySpec`), planning (`QueryPlanner`) |
| [`query/trigram.rs`](src/query/trigram.rs) | Raw trigram extraction utilities |
| [`index/`](src/index/) | `SearchIndex` trait, `Indexes` registry, shared types (`FileId`, `IndexId`, `IndexMeta`) |
| [`index/trigram/`](src/index/trigram/) | Trigram index: build, load, search, and persistence |
| [`index/trigram/storage/`](src/index/trigram/storage/) | Binary persistence format (lexicon, postings, file tables) |
| [`grep/`](src/grep/) | `CompiledSearch`, pattern compilation, parallel file scanning, filtering, output formatting |
| [`lib.rs`](src/lib.rs) | Public API re-exports, error types, constants |

## API

```rust
use sift_core::{TrigramIndexBuilder, TrigramIndex, CompiledSearch, SearchOptions};

// Build
let index = TrigramIndexBuilder::new(&corpus_root).with_dir(&index_dir).build()?;

// Open
let index = TrigramIndex::open(&index_dir)?;

// Search
let search = CompiledSearch::new(&patterns, SearchOptions::default())?;
let hits = search.collect_index_matches(&index)?;
```

`CompiledSearch` compiles the regex once; repeated `run_index` / `collect_index_matches` calls reuse the compiled matcher and searcher cache.

## Features

| Feature | Effect |
|---------|--------|
| `profile` | Enables `sift-profile` binary and `tempfile` dependency |

## Testing

Unit tests are co-located with implementation files in `#[cfg(test)] mod tests` blocks. Integration tests live in `tests/`.

```bash
cargo test -p sift-core
cargo bench -p sift-core --bench search
```

See [`benches/README.md`](benches/README.md) for the full benchmark and profiling workflow.

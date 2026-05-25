# sift-core

Indexed grep-style search engine. Build a trigram index on disk, then run regex or fixed-string queries with automatic candidate narrowing.

## Modules

| Module | Description |
|--------|-------------|
| [`query/`](src/query/) | Query description (`QuerySpec`), planning (`QueryPlanner`) |
| [`query/trigram.rs`](src/query/trigram.rs) | Trigram extraction, scoring sort, candidate selection |
| [`index/`](src/index/) | `SearchIndex` trait, `Indexes` registry, shared types (`FileId`, `IndexId`, `IndexMeta`) |
| [`index/trigram/`](src/index/trigram/) | Trigram index: build, load, search, and persistence |
| [`index/trigram/storage/`](src/index/trigram/storage/) | Binary persistence format (lexicon, postings, file tables) |
| [`grep/`](src/grep/) | Pipeline orchestration — `GrepRequest`, `run()` |
| [`lib.rs`](src/lib.rs) | Public API re-exports, error types, constants |

## API

```rust
use sift_core::{SearchOptions, SearchQuery, TrigramIndex, TrigramIndexBuilder, Indexes, CandidateFilter};

// Build
let index = TrigramIndexBuilder::new(&corpus_root).with_dir(&index_dir).build()?;

// Open
let index = TrigramIndex::open(&index_dir)?;

// Search
let indexes = Indexes::open(&sift_dir)?;
let search = SearchQuery::new(&patterns, SearchOptions::default())?;
let candidates = indexes.candidates(&query.spec(), coverage);
search.run(SearchExecution { candidates: &candidates, output, separators, collect_stats: false })?;
```

`SearchQuery` compiles the regex once; repeated `run` calls reuse the compiled matcher and searcher cache.

## Features

| Feature | Effect |
|---------|--------|
| `profile` | Enables `sift-profile` binary and `tempfile` dependency |

## Testing

Unit tests are co-located with implementation files in `#[cfg(test)] mod tests` blocks. Integration tests live in `tests/`.

```bash
cargo test -p sift-core
cargo bench -p sift-core --bench query
cargo bench -p sift-core --bench index
cargo bench -p sift-core --bench grep
```

See [`benches/README.md`](benches/README.md) for the full benchmark and profiling workflow.

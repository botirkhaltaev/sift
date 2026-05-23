# sift-core

Indexed grep-style search engine. Build a trigram index on disk, then run regex or fixed-string queries with automatic candidate narrowing.

## Modules

| Module | Description |
|--------|-------------|
| [`query/`](src/query/) | Query description, planning, and candidate plans |
| [`index/`](src/index/) | Generic index traits (`Index`, `CandidateSource`) and concrete implementations |
| [`index/trigram/`](src/index/trigram/) | Trigram index: build, load, search, and persistence |
| [`grep/`](src/grep/) | `CompiledSearch`, parallel file scanning, filtering, and output formatting |
| [`verify.rs`](src/verify.rs) | Pattern shaping (`-F`/`-w`/`-x`) and regex compilation |
| [`lib.rs`](src/lib.rs) | Public API re-exports, error types, constants |
| [`bin/sift_profile/`](src/bin/sift_profile/) | `sift-profile` binary for hot-loop benchmarking (feature-gated) |

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

```bash
cargo test -p sift-core
cargo bench -p sift-core --bench search
```

See [`benches/README.md`](benches/README.md) for the full benchmark and profiling workflow.

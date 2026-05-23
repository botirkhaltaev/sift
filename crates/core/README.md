# sift-core

Indexed grep-style search engine. Build a trigram index on disk, then run regex or fixed-string queries with automatic candidate narrowing.

## Modules

| Module | Description |
|--------|-------------|
| [`index/`](src/index/) | Corpus walker, trigram extraction, index builder, and `Index` handle |
| [`search/`](src/search/) | `CompiledSearch`, parallel file scanning, filtering, and output formatting |
| [`storage/`](src/storage/) | On-disk binary format for files, lexicon, and postings tables |
| [`planner.rs`](src/planner.rs) | Regex → trigram plan: literal extraction, arm decomposition, or full-scan fallback |
| [`verify.rs`](src/verify.rs) | Pattern shaping (`-F`/`-w`/`-x`) and regex compilation |
| [`lib.rs`](src/lib.rs) | Public API re-exports, error types, constants |

## API

```rust
use sift_core::{IndexBuilder, Index, CompiledSearch, SearchOptions};

// Build
IndexBuilder::new(&corpus_root).with_dir(&index_dir).build()?;

// Open
let index = Index::open(&index_dir)?;

// Search
let search = CompiledSearch::new(&patterns, SearchOptions::default())?;
let hits = search.collect_index_matches(&index)?;
```

`CompiledSearch` compiles the regex once; repeated `search_index` / `run_index` calls reuse the compiled matcher and searcher cache.

## Testing

```bash
cargo test -p sift-core
cargo bench -p sift-core --bench search
```

See [`benches/README.md`](benches/README.md) for the full benchmark and profiling workflow.

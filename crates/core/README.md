# sift-core

Core engine for indexed code search. Build on-disk indexes over a codebase, then run regex or fixed-string queries with automatic candidate narrowing.

## Index Architecture

The engine is built around composable indexes. Each index type independently narrows candidates for a query; the `Indexes` registry combines their results via set intersection. Today the shipped index type is a **trigram index**; the `IndexKind` and `Index` enums provide static dispatch for adding future index kinds (AST indexes, dependency graphs, vector indexes, etc.) without changing the query planner or search pipeline.

```
IndexKind::Trigram  ──build/open/update──>  Index::Trigram(TrigramIndex)
IndexKind::???      ──build/open/update──>  Index::???(...)
                                                 │
                                          Indexes registry
                                                 │
                              intersect candidate sets at query time
```

## Modules

| Module | Description |
|--------|-------------|
| [`query/`](src/query/) | Query description (`QuerySpec`), planning -- index-agnostic |
| [`index/`](src/index/) | `IndexKind` / `Index` enums, `Indexes` registry, `IndexStore`, snapshot persistence |
| [`index/trigram/`](src/index/trigram/) | Trigram index: build, load, search, and on-disk storage |
| [`grep/`](src/grep/) | Pipeline orchestration: `GrepRequest`, `run()` |
| [`search/`](src/search/) | Regex execution, scanning, output formatting, parallelism |
| [`lib.rs`](src/lib.rs) | Public API re-exports, error types, constants |

## API

```rust
use sift_core::{
    IndexStore, IndexKind, IndexConfig, Indexes,
    SearchQuery, SearchOptions, GrepRequest, CandidateFilter,
};

// Build indexes (currently trigram; extensible to multiple kinds)
let mut store = IndexStore::open_or_create(&sift_dir, &meta)?;
store.build(&[IndexKind::Trigram], &config, &[])?;

// Open all indexes in the store
let indexes = Indexes::open(&sift_dir)?;

// Search via the grep pipeline
let query = SearchQuery::new(&patterns, SearchOptions::default())?;
let run = GrepRequest {
    indexes: &indexes,
    filter: &filter,
    output,
    separators: &separators,
    collect: SearchCollection::none(),
    store_meta: None,
    walk_unindexed: false,
}
.run(&query)?;
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

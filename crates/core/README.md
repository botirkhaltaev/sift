# sift-core

Core engine for indexed code search. Build on-disk indexes over a codebase, then run regex or fixed-string queries with automatic candidate narrowing.

## Index Architecture

The engine is built around composable indexes. Each configured index independently narrows candidates for a query; the `Indexes` registry combines their results via set intersection. Today the shipped index is a runtime-width N-gram index that defaults to trigram width. `IndexConfig` records configured/persisted identity, `IndexStore` owns build/open/update transactions, and `Index` is the opened query-time dispatch.

```
IndexConfig::ngram(GramWidth::TRIGRAM)  ──IndexStore──>  Index::NGram(NGramIndex)
IndexConfig::???                         ──IndexStore──>  Index::???(...)
                                                                  │
                                                           Indexes registry
                                                                  │
                                               intersect candidate sets at query time
```

## Modules

| Module | Description |
|--------|-------------|
| [`query/`](src/query/) | Query description (`QuerySpec`), planning -- index-agnostic |
| [`walk/`](src/walk/) | Shared filesystem discovery for search candidates and index builds |
| [`index/`](src/index/) | `IndexConfig` / `Index` dispatch, `Indexes` registry, `IndexStore`, snapshot persistence |
| [`index/ngram/`](src/index/ngram/) | Runtime-width N-gram index: build, load, search, and on-disk storage |
| [`grep/`](src/grep/) | Grep execution: query/options/filter/output, candidate resolution, scanning, rendering |
| [`lib.rs`](src/lib.rs) | Public API re-exports, error types, constants |

## API

```rust
use sift_core::{
    CandidateIndexState, GramWidth, Grep, GrepCorpus, GrepQuery, IndexBuildConfig, IndexConfig,
    IndexStore, Indexes, SnapshotValidation,
};
use sift_core::grep::{GrepCollection, GrepOptions};

// Build indexes (currently trigram-specialized N-gram; extensible to multiple kinds)
let mut store = IndexStore::open_or_create(&sift_dir, &meta)?;
store.build(&[IndexConfig::ngram(GramWidth::TRIGRAM)], &config, &[])?;

// Open all indexes in the store
let indexes = Indexes::open(&sift_dir)?;

// Search via the grep pipeline.
let query = GrepQuery::new(patterns)?.options(GrepOptions::default());
let corpus = GrepCorpus::new(
    &indexes,
    &filter,
    CandidateIndexState {
        store_meta: Some(&meta),
        snapshot: SnapshotValidation::Unvalidated,
    },
);
let report = Grep::new(query)
    .corpus(corpus)
    .output(output)
    .separators(&separators)
    .collect(GrepCollection::none())
    .run()?;
```

`GrepQuery` compiles the regex lazily; `Grep::run` resolves candidates, materializes grep inputs, and runs the matcher through the grep runner.

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

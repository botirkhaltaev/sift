# sift-core

Core engine for indexed code search. Build on-disk indexes over a codebase, then run regex or fixed-string queries with automatic candidate narrowing.

## Index Architecture

The engine is built around composable indexes. Each configured index independently narrows candidates for a query; the `Indexes` registry combines their results via set intersection. Today the shipped index is a runtime-width N-gram index that defaults to trigram width. `IndexConfig` records configured/persisted identity, `IndexStore` owns build/open/update transactions, and `Index` is the opened query-time dispatch.

```
IndexConfig::ngram(GramWidth::TRIGRAM)  ──IndexStore──>  Index::NGram(NGramIndex)
                                                                  │
                                                           Indexes registry
                                                                  │
                                               intersect candidate sets at query time
```

## Modules

| Module | Description |
|--------|-------------|
| [`index/`](src/index/) | `IndexConfig` / `Index` dispatch, `Indexes` registry, `IndexStore`, snapshot persistence |
| [`index/ngram/`](src/index/ngram/) | Runtime-width N-gram index: build, load, search, and on-disk storage |
| [`grep/`](src/grep/) | Public search API: `Query`, `Report`, `Inputs` |
| [`candidates/`](src/candidates/) | Candidate planning and resolution: `CandidateSource`, `CandidateRequest`, `CandidateSpec` |
| [`corpus/`](src/corpus/) | Internal: candidates, filters, filesystem walk |

## Search API

```rust
use sift_core::{
    CandidateRequest, CandidateScope, CandidateSource, CorpusMode, IndexFallback,
    Indexes, Inputs, SearchOptions, Query, StatsMode, StoreMeta,
};

let indexes = Indexes::open(&sift_dir)?;
let source = CandidateSource {
    indexes: &indexes,
    filter: &filter,
    store_meta: store_meta.as_ref(),
};

let query = Query::new(vec!["pattern".into()])?.options(SearchOptions::default());
let request = CandidateRequest {
    scope: CandidateScope::Indexed,
    corpus: CorpusMode::Indexed,
    fallback: IndexFallback::WalkOnStaleSnapshot,
    order: Default::default(),
};

let candidates = query.candidates(&source, request)?;
let mut inputs = Inputs::with_capacity(candidates.len());
for candidate in &candidates {
    inputs.push_path(candidate);
}

let report = query.search(&inputs, StatsMode::Off)?;
```

Formatting and stdout live in `sift-grep` (`SearchPrinter`).

## Testing

```bash
cargo test -p sift-core
cargo bench -p sift-core --bench query
cargo bench -p sift-core --bench index
cargo bench -p sift-core --bench grep
```

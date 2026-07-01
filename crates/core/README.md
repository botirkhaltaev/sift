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
| [`grep/`](src/grep/) | Public search API: `Query`, `Session`, `Report`, `CandidatePolicy`, `Inputs` |
| [`corpus/`](src/corpus/) | Internal: candidates, filters, filesystem walk |
| [`query/`](src/query/) | Internal: pure query planning (`QuerySpec`, `QueryPlanner`) |

## Search API

```rust
use sift_core::grep::{
    CandidatePolicy, CandidatePolicyConfig, CandidateScope, CorpusState, IndexFallback,
    Inputs, MatchOptions, Query, Session, StatsMode,
};
use sift_core::{Indexes, StoreMeta};

let indexes = Indexes::open(&sift_dir)?;
let session = Session::new(&indexes, &filter, store_meta.as_ref());

let query = Query::new(vec!["pattern".into()])?.options(MatchOptions::default());
let compiled = query.compile()?;
let policy = CandidatePolicyConfig {
        output_scope: CandidateScope::Indexed,
        corpus: CorpusState::Indexed,
        fallback: IndexFallback::WalkOnStaleSnapshot,
        order: Default::default(),
    }
    .policy(compiled);

let candidates = query.candidates(&session, policy)?;
let mut inputs = Inputs::with_capacity(candidates.len());
for candidate in &candidates {
    inputs.push_path(candidate);
}

let report = compiled.report(&query, &inputs, StatsMode::Off)?;
// or `query.search(&inputs, StatsMode::Off)?` when compile is not cached yet
```

Formatting and stdout live in `sift-grep` (`SearchPrinter`).

## Testing

```bash
cargo test -p sift-core
cargo bench -p sift-core --bench query
cargo bench -p sift-core --bench index
cargo bench -p sift-core --bench grep
```

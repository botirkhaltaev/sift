# sift-core

Core engine for indexed code search. Build on-disk indexes over a codebase, then run regex or fixed-string queries with automatic candidate narrowing.

## Index architecture

Composable indexes narrow candidates independently; `Indexes` intersects their results at query time.

```
IndexConfig ──IndexStore──> snapshot on disk
                                │
                    Snapshot::open_current
                                │
                           Indexes (search)
                                │
              CandidatePlanner → CandidatePlan::resolve
                                │
                           Grep::search
```

| Type | Role |
|------|------|
| `IndexStore` | Build, update, publish |
| `Snapshot` | Open committed or in-memory snapshot |
| `Indexes` | Query, file ids, hydrate candidates |
| `Grep` | Resolve candidates + run search |

## Modules

| Module | Description |
|--------|-------------|
| [`index/`](src/index/) | Lifecycle, snapshot, search facade |
| [`index/ngram/`](src/index/ngram/) | N-gram index implementation |
| [`grep/`](src/grep/) | Public search API |
| [`candidates/`](src/candidates/) | Planning and resolution |

## Search API

```rust
use sift_core::{
    CandidateSelection, CandidateSource, Grep, GrepRequest, IndexFallback, Indexes, Inputs,
    InputConversion, PathDisplay, SearchMode, SearchOptions, SearchQuery, StatsMode,
};

let indexes = Indexes::open(&sift_dir)?;
let source = CandidateSource {
    indexes: &indexes,
    filter: &filter,
    store_meta: store_meta.as_ref(),
};

let grep = Grep::new(source);
let request = GrepRequest {
    query: SearchQuery::new(vec!["pattern".into()])?.options(SearchOptions::default()),
    selection: CandidateSelection::Index {
        fallback: IndexFallback::WalkOnStaleSnapshot,
        order: Default::default(),
    },
    streams: Inputs::empty(),
    conversion: InputConversion::for_candidates(&[], PathDisplay::Relative, None),
    mode: SearchMode::Lines,
    stats: StatsMode::Off,
};

let report = grep.search(request)?;
```

Formatting lives in `sift-grep`.

## Testing

```bash
cargo test -p sift-core
cargo bench -p sift-core --bench index
cargo bench -p sift-core --bench grep
cargo bench -p sift-core --bench candidates
```

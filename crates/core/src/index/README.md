# index/

Index registry and concrete index implementations. Owns configured index identity, snapshot lifecycle orchestration, and query-time dispatch.

## Design

Sift's index layer is built for composition. `IndexConfig` records configured/persisted index identity, `IndexStore` owns lifecycle transactions (build, open, update), and the `Index` enum drives query-time dispatch (candidate narrowing). The `Indexes` registry opens all configured indexes in a `.sift` snapshot and intersects their candidate sets, so multiple indexes together produce tighter narrowing than any single index alone.

Today, the default configured index is `IndexConfig::ngram(GramWidth::TRIGRAM)` / `Index::NGram`, backed by a runtime-width N-gram implementation. Adding a new index family means adding a configured identity variant and an opened runtime variant. Everything above the index layer -- query planning, search execution, the CLI -- works unchanged.

```
index/
  ngram/       -- Runtime-width N-gram index (default width 3)
  (future)     -- AST index, dependency graph, vector index, etc.
```

## Modules

| Module | Description |
|--------|-------------|
| [`kinds.rs`](kinds.rs) | `IndexConfig` enum (configured identity), `Index` enum (query dispatch), `FileId`, `IndexId` |
| [`registry.rs`](registry.rs) | `Indexes`: opens all indexes in a snapshot, intersects candidate sets |
| [`store.rs`](store.rs) | `IndexStore`: snapshot-based persistence, atomic build/update/publish |
| [`config.rs`](config.rs) | `IndexBuildConfig`, `CorpusSpec`, `CorpusKind`, `WalkOptions` |
| [`meta.rs`](meta.rs) | `StoreMeta`: persistent manifest (`meta.json`) with corpus, walk, filter, and index configuration metadata |
| [`artifacts.rs`](artifacts.rs) | `IndexSource`, `IndexDestination`: read/write dispatch for directories vs snapshots |
| [`snapshot/`](snapshot/) | Snapshot store: atomic persistence, leases, manifests |
| [`error.rs`](error.rs) | `IndexError` |
| [`ngram/`](ngram/) | Runtime-width N-gram index implementation |

## API

```rust
use sift_core::{FileId, GramWidth, Index, IndexConfig, IndexStore, Indexes};

// Build via IndexStore (snapshot-managed)
let mut store = IndexStore::open_or_create(&sift_dir, &meta)?;
store.build(&[IndexConfig::ngram(GramWidth::TRIGRAM)], &config, &[])?;

// Open all indexes for search
let indexes = Indexes::open(&sift_dir)?;
let candidates = indexes.candidates(&query_spec);
```

## Adding a New Index Kind

1. Add a variant to `IndexConfig` and `Index` in `kinds.rs`.
2. Implement configured lifecycle routing through `IndexConfig` and query dispatch through `Index`.
3. Implement `root`, `corpus_kind`, `candidates`, `all_files` in the `Index` match arms.
4. Create a sibling module to `ngram/` with the index implementation.

The `Indexes` registry, `IndexStore`, snapshot layer, query planner, and CLI require no changes.

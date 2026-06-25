# index/

Index registry and concrete index implementations. Owns the lifecycle (build, open, update) and query-time dispatch for all index kinds.

## Design

Sift's index layer is built for composition. The `IndexKind` enum drives lifecycle dispatch (build, open, update), and the `Index` enum drives query-time dispatch (candidate narrowing). The `Indexes` registry opens all index kinds in a `.sift` snapshot and intersects their candidate sets, so multiple indexes together produce tighter narrowing than any single index alone.

Today, the shipped index is `IndexKind::NGram(NGramKind::Trigram)` / `Index::NGram`, backed by a generic N-gram implementation with a trigram specialization. Adding a new index kind means adding a variant to each enum and implementing the corresponding lifecycle and query methods. Everything above the index layer -- query planning, search execution, the CLI -- works unchanged.

```
index/
  ngram/       -- N-gram index with trigram specialization (shipped)
  (future)     -- AST index, dependency graph, vector index, etc.
```

## Modules

| Module | Description |
|--------|-------------|
| [`kinds.rs`](kinds.rs) | `IndexKind` enum (lifecycle dispatch), `Index` enum (query dispatch), `FileId`, `IndexId` |
| [`registry.rs`](registry.rs) | `Indexes`: opens all indexes in a snapshot, intersects candidate sets |
| [`store.rs`](store.rs) | `IndexStore`: snapshot-based persistence, atomic build/update/publish |
| [`config.rs`](config.rs) | `IndexConfig`, `CorpusSpec`, `CorpusKind`, `WalkOptions` |
| [`meta.rs`](meta.rs) | `StoreMeta`: persistent manifest (`meta.json`) with corpus, walk, filter, and index kind metadata |
| [`artifacts.rs`](artifacts.rs) | `IndexSource`, `IndexDestination`: read/write dispatch for directories vs snapshots |
| [`snapshot/`](snapshot/) | Snapshot store: atomic persistence, leases, manifests |
| [`error.rs`](error.rs) | `IndexError` |
| [`ngram/`](ngram/) | N-gram index implementation and trigram specialization |

## API

```rust
use sift_core::{FileId, Index, IndexKind, IndexStore, Indexes, NGramKind};

// Build via IndexStore (snapshot-managed)
let mut store = IndexStore::open_or_create(&sift_dir, &meta)?;
store.build(&[IndexKind::NGram(NGramKind::Trigram)], &config, &[])?;

// Open all indexes for search
let indexes = Indexes::open(&sift_dir)?;
let candidates = indexes.candidates(&query_spec);
```

## Adding a New Index Kind

1. Add a variant to `IndexKind` and `Index` in `kinds.rs`.
2. Implement `build`, `open`, `update` in the `IndexKind` match arms.
3. Implement `root`, `corpus_kind`, `candidates`, `all_files` in the `Index` match arms.
4. Create a sibling module to `ngram/` with the index implementation.

The `Indexes` registry, `IndexStore`, snapshot layer, query planner, and CLI require no changes.

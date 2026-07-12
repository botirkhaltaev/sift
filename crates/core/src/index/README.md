# index/

Composable index lifecycle, snapshot persistence, and search-time dispatch.

## Layers

| Layer | Types | Role |
|-------|-------|------|
| Lifecycle | `IndexStore`, `IndexConfig`, `StoreMeta` | Build, update, publish snapshots |
| Snapshot | `Snapshot`, `SnapshotId` | Immutable opened snapshot + indexes |
| Search | `Indexes`, `IndexAvailability`, `IndexedCorpus` | Query, intersect, hydrate candidates |
| Dispatch | `Index`, `IndexConfig` | Per-kind lifecycle and query routing |

`IndexStore` owns write transactions. `Snapshot::open_current` and `Indexes::from_snapshot` own read/search. Candidate resolution lives in `Grep` / `CandidatePlanner`, not on `Indexes`.

## Design

Multiple configured indexes can coexist. Each narrows candidates independently; `Indexes` intersects their query results. Today the default is `IndexConfig::ngram(GramWidth::TRIGRAM)` / `Index::NGram`.

```
index/
  search.rs    -- Indexes: open snapshot, query, hydrate
  snapshot/    -- atomic persistence, leases, manifests
  store.rs     -- IndexStore: build/update/publish
  ngram/       -- runtime-width N-gram index (default width 3)
```

## Modules

| Module | Description |
|--------|-------------|
| [`kinds.rs`](kinds.rs) | `IndexConfig`, `Index`, `FileId`, `IndexId` |
| [`search.rs`](search.rs) | `Indexes`: snapshot search facade |
| [`snapshot/mod.rs`](snapshot/mod.rs) | `Snapshot::open_current`, `Snapshot::from_indexes` |
| [`store.rs`](store.rs) | `IndexStore`: lifecycle orchestration |
| [`paths.rs`](paths.rs) | `IndexedCorpus`: covered path set |
| [`config.rs`](config.rs) | `IndexBuildConfig`, `CorpusSpec`, `CorpusKind` |
| [`meta.rs`](meta.rs) | `StoreMeta` (`.sift/meta.json`) |
| [`artifacts.rs`](artifacts.rs) | `IndexSource`, `IndexDestination` |
| [`ngram/`](ngram/) | N-gram implementation |

## API

```rust
use sift_core::{GramWidth, Index, IndexConfig, IndexStore, Indexes, Snapshot};

// Lifecycle
let mut store = IndexStore::open_or_create(&sift_dir, &meta)?;
store.build(&[IndexConfig::ngram(GramWidth::TRIGRAM)], &config, &[])?;

// Search (committed snapshot)
let indexes = Indexes::open(&sift_dir)?;

// Tests/benches (in-memory snapshot)
let snapshot = Snapshot::from_indexes(root, vec![Index::NGram(index)]);
let indexes = Indexes::from_snapshot(snapshot);
```

Resolve candidates through `Grep::resolve_candidates`, not `Indexes` directly.

## Adding a New Index Kind

1. Add variants to `IndexConfig` and `Index` in `kinds.rs`.
2. Route lifecycle through `IndexConfig` and queries through `Index`.
3. Implement `query`, `hydrate_row`, `all_file_ids`, `coverage` on the `Index` arm.
4. Add a sibling module to `ngram/`.

`Indexes`, `IndexStore`, snapshot layer, candidate planner, and CLI require no changes.

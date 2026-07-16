# index/

Uniform index kinds, snapshot persistence, and search orchestration.

## Layers

| Layer | Types | Role |
|-------|-------|------|
| Contract | `Index`, `IndexWrite`, `IndexRecord` | Kind interface + persistable identity |
| Orchestrator | `Indexes`, `StoreMeta` | `open` / `build` / `update` + query/hydrate |
| Snapshot | `Snapshot`, `SnapshotId` | Opened `Box<dyn Index>` vec |
| Kind | `ngram::Index` | First shipped impl |

```
index/
  contract.rs  -- Index trait, IndexRecord, IndexWrite
  search.rs    -- Indexes: lifecycle + search
  snapshot/    -- atomic persistence, leases, manifests
  ngram/       -- runtime-width N-gram index (default width 3)
```

## Modules

| Module | Description |
|--------|-------------|
| [`contract.rs`](contract.rs) | `Index` trait, `IndexRecord`, `IndexWrite` |
| [`search.rs`](search.rs) | `Indexes` orchestrator |
| [`snapshot/`](snapshot/) | Snapshot persistence |
| [`kinds.rs`](kinds.rs) | `FileId`, `IndexId`, plan output types |
| [`paths.rs`](paths.rs) | `IndexedCorpus` |
| [`config.rs`](config.rs) | `IndexConfig` (corpus write inputs), `CorpusSpec` |
| [`meta.rs`](meta.rs) | `StoreMeta` |
| [`artifacts.rs`](artifacts.rs) | `IndexSource`, `IndexDestination` |
| [`ngram/`](ngram/) | N-gram implementation |

## API

```rust
use sift_core::{GramWidth, IndexRecord, Indexes, NGramIndex, StoreMeta};

let mut indexes = Indexes::open(&sift_dir, &meta)?;
let catalog = [Box::new(NGramIndex::new()) as Box<dyn sift_core::Index>];
indexes.build(&catalog, &config, &[])?;

// Or refresh from meta.indexes via IndexRecord::to_index()
```

Resolve candidates through `Grep::resolve_candidates`.

## Adding a New Index Kind

1. Implement `Index` on the kind type.
2. Register in `IndexRecord::to_index`.
3. Add a sibling module under `index/`.

No central enum dispatch.

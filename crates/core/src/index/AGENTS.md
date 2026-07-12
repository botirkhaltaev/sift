# AGENTS.md -- index/

## Responsibility

Configured index identity, snapshot lifecycle, and search-time dispatch.

## Layer split

Do not mix lifecycle, snapshot I/O, and search orchestration on one type.

| Layer | Types | Owns |
|-------|-------|------|
| Lifecycle | `IndexStore`, `StoreMeta` | `build`, `update`, `current_id` |
| Snapshot | `Snapshot`, `SnapshotId` | `open_current`, `from_indexes`, opened `Index` vec |
| Search | `Indexes`, `IndexAvailability`, `IndexedCorpus` | `query`, `file_ids`, `indexed_candidates`, `hydrate_*` |
| Kind dispatch | `Index`, `IndexConfig` | per-kind lifecycle + `query` |

CLI owns daemon orchestration (`SnapshotRefresh`, path debouncing). Core does not expose `reconcile`, `unindexed_hit_paths`, or walk-merge helpers on `Indexes`.

## Composable search API

Callers compose primitives. Do not add use-case constructors or search shortcuts on `Indexes`.

| Do | Don't |
|----|-------|
| `Snapshot::from_indexes` + `Indexes::from_snapshot` | `from_single`, `from_test_*` |
| `Grep::resolve_candidates` / `CandidatePlanner` | `Indexes::candidates(SearchQuery, …)` |
| `indexed_corpus().retain_unindexed(paths)` | `unindexed_hit_paths`, daemon filters in core |
| `hydrate_row` / `hydrate_rows` | `materialize_*` |

`lead_index()` (private) is the manifest-first index used to hydrate file rows. Multi-index intersection happens in `query` / `file_ids`, not at hydrate time.

## Key types

- `IndexConfig` — configured/persisted identity
- `Index` — opened runtime dispatch (`query`, `hydrate_*`, `coverage`)
- `Indexes` — search facade over one `Snapshot`
- `IndexStore` — write-path orchestration only
- `IndexedCorpus` — cheap clone of covered rel-paths; `retain_unindexed` filters paths
- `IndexSource` / `IndexDestination` — directory vs snapshot I/O

## Conventions

- `grep/` and `candidates/` talk to `Indexes` and `Index`, never `ngram/` internals.
- `Index::query` may over-return; it must not under-return.
- Each configured index narrows independently; `Indexes` intersects matched file-id sets.
- `IndexAvailability.snapshot` is `None` for in-memory snapshots (`Snapshot::from_indexes`).

## Adding a new index kind

1. Variants in `kinds.rs`.
2. Lifecycle on `IndexConfig`; query/hydrate on `Index`.
3. Sibling module under `index/`.

No changes to `Indexes`, `IndexStore`, `CandidatePlanner`, or `grep/`.

## Do NOT

- Add N-gram logic outside `ngram/`.
- Add daemon or CLI orchestration to core.
- Add free functions — use methods on the owning type.
- Add parallel `*_with_*` / test-only constructors.

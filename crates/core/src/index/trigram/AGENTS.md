# index/trigram/

Trigram index implementation: corpus walk, trigram extraction, index building, file table, and persistence storage.

## Key Types

- `TrigramIndex` — memory-mapped handle over the trigram index files.
- `TrigramIndexBuilder` — fluent builder for corpus indexing.
- `MappedFilesView` — O(1) file ID → path lookup.

## Conventions

- File paths are always relative to the corpus root.
- Trigram extraction is parallelized via Rayon when file count exceeds the threshold.
- `build_index_tables` returns in-memory tables; persistence is done by the caller.
- The only `unsafe` in the crate lives in `storage/mmap.rs` with a documented safety invariant.

## Do NOT

- Change the file-path sort order (breaks stable file IDs).
- Add new persistence files without updating `TrigramIndex::open` and the storage format docs.
- Add `unsafe` outside `storage/mmap.rs`.

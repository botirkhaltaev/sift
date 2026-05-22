# AGENTS.md — index/

## Responsibility

Trigram index construction and in-memory index handle. Walks the corpus, extracts trigrams, writes persistence files, and provides zero-copy access for queries.

## Key Types

- `Index` — memory-mapped handle over the three index files.
- `IndexBuilder` — fluent builder for corpus indexing.
- `MappedFilesView` — O(1) file ID → path lookup.
- `IndexMeta` — serialized metadata (`sift.meta` JSON).

## Conventions

- File paths are always relative to the corpus root.
- Trigram extraction is parallelized via Rayon when file count exceeds the threshold.
- `build_index_tables` returns in-memory tables; persistence is done by the caller (`IndexBuilder`).

## Do NOT

- Change the file-path sort order (breaks stable file IDs).
- Add new persistence files without updating `Index::open` and the storage format docs.

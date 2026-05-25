# AGENTS.md -- index/trigram/

Trigram index implementation: corpus walk, trigram extraction, index building, file table, and persistence storage.

## Key Types

- `TrigramIndex`: memory-mapped handle over the trigram index files (files, lexicon, postings, trigram sets).
- `IndexTableBuilder`: incremental table builder; reuses cached trigrams for unchanged files.
- `MappedFilesView`: O(1) file ID to path + fingerprint lookup.
- `FileFingerprint`: per-file change detection data (path, mtime, size).

## Conventions

- File paths are always relative to the corpus root.
- Trigram extraction is parallelized via Rayon.
- `IndexTableBuilder::build()` returns in-memory tables; persistence is done by the caller.
- The only `unsafe` in the crate lives in `storage/mmap.rs` with a documented safety invariant.

## Do NOT

- Change the file-path sort order (breaks stable file IDs).
- Add new persistence files without updating `TrigramIndex::open` and the storage format docs.
- Add `unsafe` outside `storage/mmap.rs`.

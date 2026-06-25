# AGENTS.md -- index/ngram/

N-gram index implementation: corpus walk, runtime-width gram extraction, index building, file table, and persistence storage. The module owns configured N-gram identity (`Config`) and the opened runtime index (`Index`).

## Key Types

- `Config`: configured N-gram identity, currently represented by a runtime `GramWidth`.
- `Index`: memory-mapped opened runtime handle over N-gram index files.
- `GramWidth`, `Gram`, `GramWindows`: runtime-width gram domain primitives.
- `IndexTables`: table builder output; reuses cached grams for unchanged files.
- `FileFingerprint`: per-file change detection data (path, mtime, size).

## Conventions

- File paths are always relative to the corpus root.
- Filesystem discovery uses `walk::FileWalk`; do not add N-gram-specific walk visitors.
- Gram extraction is parallelized via Rayon.
- `IndexTables::build()` returns in-memory tables; `Config` persists them through `IndexDestination` in `mod.rs`.
- Generic behavior is runtime-width. Do not add specialization layers until a measured hot path justifies them.
- The only `unsafe` in the index crate lives in `index/mmap.rs` with a documented safety invariant.

## Do NOT

- Change the file-path sort order; it defines stable file IDs.
- Add new persistence files without updating N-gram persistence, storage docs, and snapshot tests.
- Add specialization logic before the general runtime-width implementation has a measured need.
- Add `unsafe` outside `index/mmap.rs`.

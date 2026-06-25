# AGENTS.md -- index/ngram/

N-gram index implementation: corpus walk, gram extraction, index building, file table, and persistence storage. The module owns the generic `NGramIndex<S>` machinery and optimized `NGramSpec` specializations such as `TrigramSpec`.

## Key Types

- `NGramIndex<S>`: memory-mapped handle over N-gram index files, parameterized by a concrete `NGramSpec`.
- `NGramSpec`: domain trait for gram width, gram collection, and postings assembly. Keep generic fallback behavior here and override only specialization-worthy hot paths.
- `TrigramSpec`: shipped optimized specialization for 3-byte grams.
- `GramWidth`, `GramKey`, `PackedGram<N>`, `Trigram`: gram domain primitives.
- `IndexTables<G>`: table builder output; reuses cached grams for unchanged files.
- `FileFingerprint`: per-file change detection data (path, mtime, size).

## Conventions

- File paths are always relative to the corpus root.
- Gram extraction is parallelized via Rayon.
- `IndexTables::build()` returns in-memory tables; `NGramIndex<S>` persists them through `IndexDestination` in `mod.rs`.
- Generic behavior lives behind `NGramSpec`; callers should not branch on implementation details such as gram width.
- The only `unsafe` in the index crate lives in `index/mmap.rs` with a documented safety invariant.

## Do NOT

- Change the file-path sort order; it defines stable file IDs.
- Add new persistence files without updating `NGramIndex<S>` persistence, storage docs, and snapshot tests.
- Add specialization logic outside `ngram/`.
- Add `unsafe` outside `index/mmap.rs`.

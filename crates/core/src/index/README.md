# index/

Trigram index construction and in-memory index handle.

## Modules

| File | Description |
|------|-------------|
| [`mod.rs`](mod.rs) | `Index` struct (memory-mapped tables), `IndexBuilder`, `IndexMeta`, `QueryPlan`, posting-list intersection |
| [`builder.rs`](builder.rs) | `build_index_tables` — corpus walk, parallel trigram extraction, in-memory table construction |
| [`trigram.rs`](trigram.rs) | `extract_trigrams`, `extract_trigrams_from_bytes` — overlapping 3-byte window extraction |
| [`files.rs`](files.rs) | `MappedFilesView` — read/write `files.bin` (file ID → relative path, O(1) lookup) |

## Key Types

- **`Index`** — zero-copy handle over memory-mapped `files.bin`, `lexicon.bin`, `postings.bin`. Opening is cheap (just mmap, no deserialization).
- **`IndexBuilder`** — fluent API: `.with_dir()`, `.follow_links()`, `.exclude()`, then `.build()` to persist.
- **`QueryPlan`** — explains whether a pattern uses indexed narrowing or full scan.
- **`CorpusKind`** — `Directory` (tree walk) or `File` (single-file index).

## File Format

```
files.bin:    SIFTFIL2 | count(4) | offsets[count](4*count) | path_len(4) path_bytes(n)...
lexicon.bin:  SIFTLEX1 | count(4) | [trigram(3) offset(8) len(4)]...
postings.bin: SIFTPST1 | len(4)   | u32 LE file-ids...
```

## Invariants

- File paths are sorted lexicographically (stable file IDs across builds).
- Parallel extraction uses the same Rayon gating heuristic as parallel search.

# AGENTS.md — sift-core

## Responsibility

Core search engine: query planning, trigram index, grep-style execution, and parallel file scanning.

## Public API

Re-exported from `lib.rs`: `TrigramIndex`, `TrigramIndexBuilder`, `CompiledSearch`, `SearchOptions`, `QueryPlanner`, `QuerySpec`, `CandidatePlan`, `Index`, `CandidateSource`, `FileId`, `walk_file_paths`, storage helpers.

## Source Map

| Module | Responsibility |
|--------|----------------|
| `query/` | Query description (`QuerySpec`), planning (`QueryPlanner`), candidate plans |
| `query/trigram.rs` | Raw trigram extraction utilities |
| `index/mod.rs` | `Index` trait, `CandidateSource<P>` trait, `FileId`, `CorpusKind`, `IndexMeta` |
| `index/trigram/mod.rs` | `TrigramIndex` struct, posting list intersection, trait impls |
| `index/trigram/builder.rs` | `TrigramIndexBuilder` — corpus walk, trigram extraction, table construction |
| `index/trigram/file_table.rs` | `MappedFilesView` — file ID → relative path mapping |
| `index/trigram/storage/` | Binary persistence format for lexicon, postings, and file tables |
| `grep/mod.rs` | Module declarations and public re-exports |
| `grep/types.rs` | `CompiledSearch`, `SearchOptions`, output types |
| `grep/execute.rs` | `run_index`, `run_walk`, parallel scanning, output writing |
| `grep/filter.rs` | Glob, hidden-file, ignore-rule, and scope filtering |
| `grep/matcher.rs` | `grep_regex`/`grep_searcher` integration |
| `verify.rs` | `pattern_branch`, `compile_search_pattern` — `-F`/`-w`/`-x` shaping |
| `bin/sift_profile/` | `sift-profile` — feature `profile` only |

## Invariants

- **Determinism:** parallel search merges hits sorted by `(file, line, text)`.
- **Index file order:** lexicographic relative paths (stable file IDs).
- **Rayon gating:** same effective-worker heuristic for parallel search and parallel index extraction.

## Testing

```bash
cargo test -p sift-core
```

Integration-style tests in `lib.rs` `mod tests`; unit tests co-located in modules.

## Do NOT

- Break the public API without updating the CLI crate.
- Add `unsafe` outside `index/trigram/storage/mmap.rs`.
- Use `#[allow(clippy::…)]` without a documented reason.
- Have `grep/` import from `index::trigram` — use traits only.

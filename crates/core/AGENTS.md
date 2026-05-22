# AGENTS.md — sift-core

## Responsibility

Core search engine: trigram index construction, query planning, pattern compilation, and parallel file scanning.

## Public API

Re-exported from `lib.rs`: `Index`, `IndexBuilder`, `QueryPlan`, `CompiledSearch`, `SearchOptions`, `TrigramPlan`, `walk_file_paths`, storage helpers.

## Source Map

| Module | Responsibility |
|--------|----------------|
| `index/` | `Index`, `IndexBuilder`, corpus walk, trigram extraction, persistence |
| `index/builder.rs` | `build_index_tables` — in-memory trigram table construction |
| `index/trigram.rs` | `extract_trigrams`, `extract_trigrams_from_bytes` |
| `index/files.rs` | Read/write `files.bin` (file ID ↔ relative path) |
| `planner.rs` | `TrigramPlan::for_patterns` — literal/alternation → narrow arms or full scan |
| `search/execute.rs` | `run_index`, `search_index`, parallel scanning, output writing |
| `search/filter.rs` | Glob, hidden-file, ignore-rule, and scope filtering |
| `search/matcher.rs` | `grep_regex`/`grep_searcher` integration |
| `search/types.rs` | `CompiledSearch`, `SearchOptions`, `SearchMatchFlags`, output types |
| `verify.rs` | `pattern_branch`, `compile_search_pattern` — `-F`/`-w`/`-x` shaping |
| `storage/` | Binary format for lexicon, postings, and file tables |
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
- Add `unsafe` outside `storage/mmap.rs`.
- Use `#[allow(clippy::…)]` without a documented reason.

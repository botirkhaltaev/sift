# AGENTS.md — sift-core

## Responsibility

Core search engine: query planning, trigram index, grep-style execution, and parallel file scanning.

## Public API

Re-exported from `lib.rs`: `TrigramIndex`, `TrigramIndexBuilder`, `CompiledSearch`, `SearchOptions`, `QueryPlanner`, `QuerySpec`, `SearchIndex`, `FileId`, `IndexId`, `FileCandidate`, `walk_file_paths`, storage helpers.

## Source Map

| Module | Responsibility |
|--------|----------------|
| `query/` | Query description (`QuerySpec`), planning (`QueryPlanner`) |
| `query/trigram.rs` | Raw trigram extraction utilities |
| `index/mod.rs` | `SearchIndex` trait, shared types (`FileId`, `IndexId`, `FileCandidate`, `IndexMeta`) |
| `index/trigram/mod.rs` | `TrigramIndex` struct, posting list intersection, `SearchIndex` impl |
| `index/trigram/builder.rs` | `TrigramIndexBuilder` — corpus walk, trigram extraction, table construction |
| `index/trigram/file_table.rs` | `MappedFilesView` — file ID → relative path mapping |
| `index/trigram/storage/` | Binary persistence format for lexicon, postings, and file tables |
| `grep/mod.rs` | Module declarations and public re-exports |
| `grep/types.rs` | `CompiledSearch`, `SearchOptions`, output types |
| `grep/execute.rs` | `run_indexes`, `run_walk`, parallel scanning, output writing |
| `grep/filter.rs` | Glob, hidden-file, ignore-rule, and scope filtering |
| `grep/matcher.rs` | `grep_regex`/`grep_searcher` integration |
| `verify.rs` | `pattern_branch`, `compile_search_pattern` — `-F`/`-w`/`-x` shaping |
| `bin/sift_profile/` | `sift-profile` — feature `profile` only |

## Architecture

### SearchIndex Trait
```rust
pub trait SearchIndex: Sync + Send {
    fn root(&self) -> &Path;
    fn file_count(&self) -> usize;
    fn file_path(&self, id: FileId) -> Option<&Path>;
    fn file_abs_path(&self, id: FileId) -> Option<PathBuf>;
    fn candidates(&self, query: &QuerySpec<'_>) -> Vec<FileId>;
    fn is_single_file(&self) -> bool;
}
```

### Search Flow
```text
CompiledSearch::run_indexes(&[&dyn SearchIndex], ...)
  -> build QuerySpec from patterns + options
  -> QueryPlanner::should_use_indexes(spec)
  -> if false: enumerate all files from all indexes
  -> if true: call index.candidates(spec) for each index
  -> resolve paths, apply SearchFilter
  -> scan candidates with regex engine
```

### Key Types
- `FileId` — type-safe file identifier within an index
- `IndexId` — type-safe index identifier in a multi-index search
- `FileCandidate` — resolved file with index_id, file_id, rel_path, abs_path
- `CandidateInfo` — pre-filtered candidate with rel_path, rel_str, abs_path (used by grep)

## Invariants

- **Determinism:** parallel search merges hits sorted by `(file, line, text)`.
- **Index file order:** lexicographic relative paths (stable file IDs).
- **Rayon gating:** same effective-worker heuristic for parallel search and parallel index extraction.
- **Conservative candidates:** `SearchIndex::candidates` may over-return but must not under-return.

## Testing

```bash
cargo test -p sift-core
```

Integration-style tests in `lib.rs` `mod tests`; unit tests co-located in modules.

## Do NOT

- Break the public API without updating the CLI crate.
- Add `unsafe` outside `index/trigram/storage/mmap.rs`.
- Use `#[allow(clippy::…)]` without a documented reason.
- Have `grep/` import from `index::trigram` — use `SearchIndex` trait only.

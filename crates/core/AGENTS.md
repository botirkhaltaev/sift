# AGENTS.md — sift-core

## Responsibility

Core search engine: query planning, trigram index, grep-style execution, and parallel file scanning.

## Public API

Re-exported from `lib.rs`: `TrigramIndex`, `TrigramIndexBuilder`, `Indexes`, `CompiledSearch`, `SearchOptions`, `QueryPlanner`, `QuerySpec`, `SearchIndex`, `FileId`, `IndexId`, `discover_files`, `PatternCompiler`, `SearchError`, storage helpers.

## Source Map

| Module | Responsibility |
|--------|----------------|
| `query/` | Query description (`QuerySpec`), planning (`QueryPlanner`) |
| `query/trigram.rs` | Raw trigram extraction utilities |
| `index/mod.rs` | `Indexes` registry, `SearchIndex` trait, shared types (`FileId`, `IndexId`, `IndexMeta`), `IndexError` |
| `index/trigram/mod.rs` | `TrigramIndex` struct, posting list intersection, `SearchIndex` impl, `TrigramIndexError` |
| `index/trigram/builder.rs` | `TrigramIndexBuilder` — corpus walk, trigram extraction, table construction |
| `index/trigram/file_table.rs` | `MappedFilesView` — file ID → relative path mapping |
| `index/trigram/storage/` | Binary persistence format for lexicon, postings, and file tables |
| `grep/mod.rs` | Module declarations, `SearchError` aggregate, public re-exports |
| `grep/options/` | `SearchOptions`, `SearchMatchFlags`, `CaseMode`, `BinaryMode` |
| `grep/search/` | `CompiledSearch`, `Match` |
| `grep/compile/` | `PatternCompiler` — composable regex builder |
| `grep/matcher/` | `grep_regex`/`grep_searcher` integration, matcher/searcher cache |
| `grep/filter/` | `SearchFilter`, `CandidateInfo`, config/ignore/type_filter |
| `grep/output/` | `SearchOutput`, style/mode/format/passthru |
| `grep/execution/` | `run_indexes`, `run_walk`, workers, sinks, stats, candidate prep |
| `bin/sift_profile/` | `sift-profile` — feature `profile` only |

## Error Ownership

Each grep sub-module defines its own error type; `grep/mod.rs` aggregates them into a unified `SearchError` via `From` impls:

| Module | Error Type | Mapped To |
|--------|-----------|-----------|
| `compile/` | `CompileError` | `SearchError::RegexBuild` |
| `matcher/` | `MatcherError` | `SearchError::RegexBuild` |
| `filter/` | `FilterError` | `SearchError::RegexBuild`, `SearchError::Ignore` |
| `output/` | `OutputError` | `SearchError::JsonOutputIncompatibleMode`, `JsonSerialize`, `Io` |
| `execution/` | `ExecutionError` | `SearchError::InvalidMaxCount`, `Io`, `Ignore` |

`SearchError` variants not owned by a sub-module (`EmptyPatterns`, `RegexBuild` direct) live in the aggregate. `crate::Error` aggregates `SearchError` as `Error::Search`.

Public grep APIs (`CompiledSearch::new`, `run_indexes`, `run_walk`, `discover_files`) return `crate::Result<T>` and rely on `From<SearchError> for crate::Error`.

## Architecture

### SearchIndex Trait
```rust
pub trait SearchIndex: Sync + Send {
    fn root(&self) -> &Path;
    fn kind(&self) -> IndexKind;
    fn candidates(&self, query: &QuerySpec<'_>) -> Vec<SearchCandidate>;
    fn all_files(&self) -> Vec<SearchCandidate>;
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
- `Indexes` — registry of opened indexes; owns initialization via `Indexes::open(sift_dir)`
- `FileId` — type-safe file identifier within an index
- `IndexId` — type-safe index identifier in a multi-index search
- `CandidateInfo` — pre-filtered candidate with rel_path, rel_str, abs_path (used by grep)
- `PatternCompiler` — composable regex builder with bitflags; `shape()`, `compile()`, `compile_one()`

## Invariants

- **Determinism:** parallel search merges hits sorted by `(file, line, text)`.
- **Index file order:** lexicographic relative paths (stable file IDs).
- **Rayon gating:** same effective-worker heuristic for parallel search and parallel index extraction.
- **Conservative candidates:** `SearchIndex::candidates` may over-return but must not under-return.

## Testing

```bash
cargo test -p sift-core
```

Unit tests are co-located with implementation files in `#[cfg(test)] mod tests` blocks. Integration tests live in `crates/core/tests/`.

## Benchmarking

Benchmarks live in `benches/` and mirror the `src/` module layout:

| File | Coverage |
|------|----------|
| `query.rs` | `QueryPlanner`, `PatternCompiler`, `CompiledSearch::new` |
| `index.rs` | `TrigramIndexBuilder`, `TrigramIndex`, `Indexes`, `SearchIndex` trait, candidates, explain, save/reopen |
| `grep.rs` | `run_indexes`, `run_walk`, `SearchFilter`, output modes |

### Conventions

- **Public API only.** No `bench-internals` feature, no `pub mod internals`, no direct benchmarking of private helpers.
- **Storage is benchmarked indirectly** through `index.rs` build/open/save/reopen paths — storage is private to the index module.
- **Benchmarks mirror implementation modules.** One bench file per domain (`query`, `index`, `grep`).
- **Fixture placement:** build benches materialize corpus + build inside `b.iter`; search/open/candidate benches build fixtures outside `b.iter`.
- **Shared fixtures** live in `benches/common/mod.rs`.

### Running

```bash
cargo bench -p sift-core --bench query
cargo bench -p sift-core --bench index
cargo bench -p sift-core --bench grep
```

See [`benches/README.md`](benches/README.md) for the full benchmark and profiling workflow.

## Do NOT

- Break the public API without updating the CLI crate.
- Add `unsafe` outside `index/trigram/storage/mmap.rs`.
- Use `#[allow(clippy::…)]` without a documented reason.
- Have `grep/` import from `index::trigram` — use `SearchIndex` trait only.
- Add variants to `crate::Error` — define them in the owning module's error type.
- Expose internal APIs for benchmarking purposes.

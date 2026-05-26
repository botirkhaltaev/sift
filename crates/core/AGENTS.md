# AGENTS.md -- sift-core

## Responsibility

Core search engine: query planning, index-backed candidate narrowing, grep-style execution, and parallel file scanning.

## Public API

Re-exported from `lib.rs`: `TrigramIndex`, `TrigramIndexBuilder`, `Indexes`, `SearchQuery`, `SearchOptions`, `QueryPlanner`, `QuerySpec`, `Index`, `IndexKind`, `FileId`, `IndexId`, `discover_files`, `PatternCompiler`, `SearchError`, storage helpers.

## Source Map

| Module | Responsibility |
|--------|----------------|
| `query/` | Query description (`QuerySpec`), planning |
| `index/mod.rs` | `Indexes` registry, `Index` enum (runtime dispatch), `IndexKind` enum (lifecycle dispatch), `IndexBuildConfig`, shared types (`FileId`, `IndexId`), `IndexError` |
| `index/store.rs` | `IndexStore`: snapshot management, `StoreMeta`, timestamp-based IDs, `gc_snapshots` |
| `index/trigram/mod.rs` | `TrigramIndex` struct, posting list intersection, inherent build/open/update/candidates methods, `TrigramIndexError` |
| `index/trigram/builder.rs` | `IndexTableBuilder`: corpus walk, fingerprint collection, trigram extraction, table construction |
| `index/trigram/file_table.rs` | `MappedFilesView`: file ID to relative path mapping with fingerprints |
| `index/trigram/storage/` | Binary persistence format for lexicon, postings, and file tables |
| `grep/mod.rs` | Pipeline orchestration: `GrepRequest`, `run()` |
| `search/` | Regex execution (scan workers, output, pattern, filter) |
| `search/options/` | `SearchOptions`, `SearchMatchFlags`, `CaseMode`, `BinaryMode` |
| `search/query/` | `SearchQuery`, `Match` |
| `search/pattern/` | `PatternCompiler`: composable regex builder |
| `search/request/` | `SearchExecution`, `WalkOptions`, `LinkTraversal` |
| `search/candidates/` | Walk-based candidate collection |
| `search/scan/` | Text / summary / JSON scanning workers |
| `search/emit/` | Output formatting, result chunks, stats helpers |
| `search/filter/` | `CandidateFilter`, `CandidateFilterConfig`, ignore/type_filter |
| `search/output/` | `SearchOutput`, style/mode/format/passthru |
| `candidate.rs` | `Candidate`: single file candidate with `rel_path`, `abs_path`, filtering |
| `bin/sift_profile/` | `sift-profile`, feature `profile` only |

## Error Ownership

Each grep sub-module defines its own error type; `grep/mod.rs` aggregates them into a unified `SearchError` via `From` impls:

| Module | Error Type | Mapped To |
|--------|-----------|-----------|
| `pattern/` | `CompileError` | `SearchError::RegexBuild` |
| `filter/` | `FilterError` | `SearchError::RegexBuild`, `SearchError::Ignore` |
| `output/` | `OutputError` | `SearchError::JsonOutputIncompatibleMode`, `JsonSerialize`, `Io` |
| `emit/` | `ExecutionError` | `SearchError::InvalidMaxCount`, `Io`, `Ignore` |

`SearchError` variants not owned by a sub-module (`EmptyPatterns`, `RegexBuild` direct) live in the aggregate. `crate::Error` aggregates `SearchError` as `Error::Search`.

Public grep APIs (`SearchQuery::new`, `SearchQuery::run`, `discover_files`, `Indexes::open`) return `crate::Result<T>` and rely on `From<SearchError> for crate::Error`.

## Architecture

### Index Enum (runtime dispatch)
```rust
pub enum Index {
    Trigram(TrigramIndex),
}
impl Index {
    pub fn root(&self) -> &Path;
    pub fn corpus_kind(&self) -> CorpusKind;
    pub fn candidates(&self, query: &QuerySpec<'_>) -> Vec<Candidate>;
    pub fn all_files(&self) -> Vec<Candidate>;
}
```

### IndexKind Enum (lifecycle dispatch)
```rust
pub enum IndexKind { Trigram }
impl IndexKind {
    pub const ALL: &[Self];
    pub fn as_str(self) -> &'static str;
    pub(crate) fn build_to_dir(self, config, output_dir) -> Result<()>;
    pub(crate) fn open_from_dir(self, index_dir, root, corpus_kind) -> Result<Index>;
    pub(crate) fn try_update(self, snapshot_dir, config, output_dir) -> Result<bool>;
}
```

### Search Flow
```text
grep::run(query, GrepRequest { indexes, filter, output, separators, collect_stats })
  -> QuerySpec from query.spec()
  -> Indexes::candidates(spec, coverage) or walk::collect_candidates
  -> candidate.matches(filter) via par_iter
  -> SearchExecution { candidates, output, separators, collect_stats }
  -> query.search(SearchExecution)
  -> scan with regex engine
  -> emit output
```

### Key Types
- `Indexes`: registry of opened indexes; owns initialization via `Indexes::open(sift_dir)`
- `IndexStore`: snapshot-based persistence for indexes, with non-generic `build`/`update` taking `&[IndexKind]`
- `StoreMeta`: single source of truth for root, corpus_kind, follow_links, and index kinds
- `FileId`: type-safe file identifier within an index
- `IndexId`: type-safe index identifier in a multi-index search
- `Candidate`: single file with rel_path, abs_path, filtering predicates
- `PatternCompiler`: composable regex builder with bitflags; `shape()`, `compile()`, `compile_one()`

## Invariants

- **Determinism:** parallel search merges hits sorted by `(file, line, text)`.
- **Index file order:** lexicographic relative paths (stable file IDs).
- **Rayon gating:** same effective-worker heuristic for parallel search and parallel index extraction.
- **Conservative candidates:** `Index::candidates` may over-return but must not under-return.

## Testing

```bash
cargo test -p sift-core
```

Unit tests are co-located with implementation files in `#[cfg(test)] mod tests` blocks. Integration tests live in `crates/core/tests/`.

## Benchmarking

Benchmarks live in `benches/` and mirror the `src/` module layout:

| File | Coverage |
|------|----------|
| `query.rs` | `QueryPlanner`, `PatternCompiler`, `SearchQuery::new` |
| `index.rs` | `TrigramIndexBuilder`, `TrigramIndex`, `Indexes`, `Index` enum, candidates, explain, save/reopen |
| `grep.rs` | `SearchQuery::run`, `CandidateFilter`, output modes |

### Conventions

- **Public API only.** No `bench-internals` feature, no `pub mod internals`, no direct benchmarking of private helpers.
- **Storage is benchmarked indirectly** through `index.rs` build/open/save/reopen paths. Storage is private to the index module.
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
- Use `#[allow(clippy::...)]` without a documented reason.
- Have `grep/` import from `index::trigram`. Use `Index` enum only.
- Add variants to `crate::Error`. Define them in the owning module's error type.
- Expose internal APIs for benchmarking purposes.

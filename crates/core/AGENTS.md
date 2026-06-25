# AGENTS.md -- sift-core

## Responsibility

Core engine for composable indexed code search: index registry, query planning, candidate narrowing, grep-style execution, and parallel file scanning.

The engine is designed around multiple coexisting index types. The `IndexKind` enum drives lifecycle dispatch (build/open/update), the `Index` enum drives query-time dispatch (candidate narrowing), and the `Indexes` registry intersects candidate sets from all available indexes. Today the shipped variant is `IndexKind::NGram(NGramKind::Trigram)`; future index kinds (AST, dependency graph, vector) slot in by adding variants to these enums.

## Public API

Re-exported from `lib.rs`: `NGramIndex`, `NGramSpec`, `TrigramSpec`, `GramWidth`, `Indexes`, `SearchQuery`, `SearchOptions`, `QueryPlanner`, `QuerySpec`, `Index`, `IndexKind`, `NGramKind`, `FileId`, `IndexId`, `discover_files`, `PatternCompiler`, `SearchError`, storage helpers.

## Source Map

| Module | Responsibility |
|--------|----------------|
| `query/` | Index-agnostic query description (`QuerySpec`), candidate planning |
| `index/mod.rs` | `Indexes` registry, `Index` enum (query dispatch), `IndexKind` enum (lifecycle dispatch), shared types (`FileId`, `IndexId`), `IndexError` |
| `index/store.rs` | `IndexStore`: snapshot-based persistence, atomic build/update/publish |
| `index/ngram/mod.rs` | `NGramIndex<S>` struct, `NGramSpec`, trigram specialization, posting list intersection, lifecycle, candidate narrowing, `NGramIndexError` |
| `index/ngram/build.rs` | `IndexTables`: corpus walk, fingerprint collection, N-gram extraction, table construction |
| `index/ngram/files.rs` | File ID to relative path mapping with fingerprints |
| `index/ngram/storage/` | Binary persistence format for gram sets, lexicon, postings, and file tables |
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
    NGram(NGramIndex<TrigramSpec>),
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
pub enum IndexKind { NGram(NGramKind) }
pub enum NGramKind { Trigram }
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
grep::run(query, GrepRequest { indexes, filter, output, separators, collect })
  -> QuerySpec from `SearchQuery::build_query_spec()` (internal)
  -> Indexes::candidates(spec, coverage) or walk::collect_candidates
  -> candidate.matches(filter) via par_iter
  -> SearchExecution { candidates, output, separators, collect }
  -> query.search(SearchExecution)
  -> scan with regex engine
  -> emit output
```

### Key Types
- `Indexes`: registry of opened indexes; opens all kinds in a snapshot and intersects their candidate sets at query time
- `IndexStore`: snapshot-based persistence for indexes, with `build`/`update` taking `&[IndexKind]`
- `NGramIndex<S>`: generic N-gram index implementation parameterized by an `NGramSpec`; `TrigramSpec` is the shipped optimized specialization
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
- **Index independence:** each index kind narrows candidates independently; the registry combines results. No index kind depends on another.

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
| `index.rs` | `NGramIndex<TrigramSpec>`, `Indexes`, `Index` enum, candidates, explain, save/reopen |
| `grep.rs` | `SearchQuery::run`, `CandidateFilter`, output modes |

### Conventions

- **Public API only.** No `bench-internals` feature, no `pub mod internals`, no direct benchmarking of private helpers.
- **Storage is benchmarked indirectly** through `index.rs` build/open/save/reopen paths. Storage is private to the index module.
- **Benchmarks mirror implementation modules.** One bench file per domain (`query`, `index`, `grep`).
- **Fixture placement:** build benches materialize corpus + build inside `b.iter`; search/open/candidate benches build fixtures outside `b.iter`.
- **Shared fixtures** live in `benches/common/mod.rs`. Prefer `Default` on domain types for baseline fixtures; override fields with struct update. Avoid `default_*()` helpers that duplicate `Default`.

### Running

```bash
cargo bench -p sift-core --bench query
cargo bench -p sift-core --bench index
cargo bench -p sift-core --bench grep
```

See [`benches/README.md`](benches/README.md) for the full benchmark and profiling workflow.

## Do NOT

- Break the public API without updating the CLI crate.
- Add `unsafe` outside `index/mmap.rs`.
- Use `#[allow(clippy::...)]` without a documented reason.
- Have `grep/` import from concrete index modules such as `index::ngram`. Use `Index` enum only.
- Add variants to `crate::Error`. Define them in the owning module's error type.
- Expose internal APIs for benchmarking purposes.

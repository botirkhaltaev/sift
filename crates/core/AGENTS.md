# AGENTS.md -- sift-core

## Responsibility

Core engine for composable indexed code search: index registry, query planning, candidate narrowing, grep-style execution, and parallel file scanning.

The engine is designed around multiple coexisting configured indexes. `IndexConfig` records configured/persisted identity, `IndexStore` owns build/open/update snapshot transactions, the `Index` enum drives query-time candidate narrowing, and the `Indexes` registry intersects candidate sets from all available indexes. Today the default configured index is `IndexConfig::ngram(GramWidth::TRIGRAM)`.

## Public API

Re-exported from `lib.rs`: `NGramIndex`, `NGramConfig`, `GramWidth`, `Gram`, `Indexes`, `Grep`, `GrepQuery`, `GrepCorpus`, `CandidateIndexState`, `GrepOptions`, `QueryPlanner`, `QuerySpec`, `Index`, `IndexConfig`, `IndexBuildConfig`, `FileId`, `IndexId`, `discover_files`, `PatternCompiler`, `GrepError`, storage helpers.

## Source Map

| Module | Responsibility |
|--------|----------------|
| `query/` | Index-agnostic query description (`QuerySpec`), candidate planning |
| `walk/` | Shared filesystem discovery (`FileWalk`) for search candidate collection and index builds |
| `index/mod.rs` | `Indexes` registry, `IndexConfig` configured identity, `Index` enum (query dispatch), shared types (`FileId`, `IndexId`), `IndexError` |
| `index/store.rs` | `IndexStore`: snapshot-based persistence, atomic build/update/publish |
| `index/ngram/mod.rs` | Runtime-width N-gram `Config` and `Index`, posting list intersection, lifecycle, candidate narrowing, `NGramIndexError` |
| `index/ngram/build.rs` | `IndexTables`: fingerprint collection, N-gram extraction, table construction |
| `index/ngram/files.rs` | File ID to relative path mapping with fingerprints |
| `index/ngram/storage/` | Binary persistence format for gram sets, lexicon, postings, and file tables |
| `grep/mod.rs` | Public grep entrypoint and stable re-exports |
| `grep/options/` | `GrepOptions`, `GrepMatchFlags`, `CaseMode`, `BinaryMode` |
| `grep/query/` | `GrepQuery`, `CompiledGrepQuery`, `Match`, matcher cache |
| `grep/pattern/` | `PatternCompiler`: composable regex builder |
| `grep/corpus.rs` | `GrepCorpus`, index state, transformed content source |
| `grep/candidates.rs` | `CandidateResolver`, `CandidateSet`, candidate coverage, candidate ordering |
| `grep/input.rs` | `GrepInput`, `GrepInputs`, `GrepStream`, transformed candidate bytes |
| `grep/runner/` | `GrepRunner`: traversal, parallel reporter execution, and result merging |
| `grep/sink/` | Per-file grep reporters for standard, summary, and JSON output |
| `grep/report.rs` | `GrepReport`, `GrepOutcome`, `GrepCollection` |
| `grep/stats.rs` | `GrepStats` and internal text-mode counters |
| `grep/filter/` | `CandidateFilter`, `CandidateFilterConfig`, ignore/type_filter |
| `grep/output/` | `GrepOutput`, style/mode/format/passthru |
| `candidate.rs` | `Candidate`: single file candidate with `rel_path`, `abs_path`, filtering |
| `bin/sift_profile/` | `sift-profile`, feature `profile` only |

## Error Ownership

Each grep sub-module defines its own error type; `grep/error.rs` aggregates them into a unified `GrepError` via `From` impls:

| Module | Error Type | Mapped To |
|--------|-----------|-----------|
| `pattern/` | `CompileError` | `GrepError::RegexBuild` |
| `filter/` | `FilterError` | `GrepError::RegexBuild`, `GrepError::Ignore` |
| `output/` | `OutputError` | `GrepError::JsonOutputIncompatibleMode`, `JsonSerialize`, `Io` |

`GrepError` variants not owned by a sub-module (`EmptyPatterns`, `RegexBuild` direct) live in the aggregate. `crate::Error` aggregates `GrepError` as `Error::Search`.

Public grep APIs (`GrepQuery::new`, `Grep::run`, `discover_files`, `Indexes::open`) return `crate::Result<T>` or `GrepError` and rely on `From<GrepError> for crate::Error`.

## Architecture

### Index Enum (runtime dispatch)
```rust
pub enum Index {
    NGram(NGramIndex),
}
impl Index {
    pub fn root(&self) -> &Path;
    pub fn corpus_kind(&self) -> CorpusKind;
    pub fn candidates(&self, query: &QuerySpec<'_>) -> Vec<Candidate>;
    pub fn all_files(&self) -> Vec<Candidate>;
}
```

### IndexConfig Enum (configured identity)
```rust
pub enum IndexConfig { NGram(NGramConfig) }
impl IndexConfig {
    pub const ALL: &[Self];
    pub fn name(self) -> String;
    pub(crate) fn build(self, build, dest, paths) -> Result<()>;
    pub(crate) fn open(self, source, root, corpus_kind) -> Result<Index>;
    pub(crate) fn update(self, source, build, dest, paths) -> Result<bool>;
}
```

### Search Flow
```text
Grep::new(GrepQuery).corpus(GrepCorpus).run()
  -> GrepQuery::compile() and QuerySpec from GrepQuery
  -> CandidateResolver using QueryPlanner with index coverage or FileWalk fallback
  -> CandidateSet filtering and ordering
  -> GrepInputs from paths, transformed bytes, and streams
  -> GrepRunner over normalized path/byte inputs
  -> write grep output and return GrepReport
```

### Key Types
- `Indexes`: registry of opened indexes; opens all kinds in a snapshot and intersects their candidate sets at query time
- `IndexStore`: snapshot-based persistence for indexes, with `build`/`update` taking `&[IndexConfig]`
- `NGramIndex`: runtime-width N-gram index implementation opened from persisted storage
- `StoreMeta`: single source of truth for root, corpus_kind, follow_links, and index configurations
- `FileId`: type-safe file identifier within an index
- `IndexId`: type-safe index identifier in a multi-index search
- `Candidate`: single file with rel_path, abs_path, filtering predicates
- `PatternCompiler`: composable regex builder with bitflags; `shape()`, `compile()`, `compile_one()`

## Invariants

- **Determinism:** parallel search merges hits sorted by `(file, line, text)`.
- **Index file order:** lexicographic relative paths (stable file IDs).
- **Rayon gating:** same effective-worker heuristic for parallel search and parallel index extraction.
- **Conservative candidates:** `Index::candidates` may over-return but must not under-return.
- **Index independence:** each configured index narrows candidates independently; the registry combines results. No index depends on another.

## Testing

```bash
cargo test -p sift-core
```

Unit tests are co-located with implementation files in `#[cfg(test)] mod tests` blocks. Integration tests live in `crates/core/tests/`.

## Benchmarking

Benchmarks live in `benches/` and mirror the `src/` module layout:

| File | Coverage |
|------|----------|
| `query.rs` | `QueryPlanner`, `PatternCompiler`, `GrepQuery::new` |
| `index.rs` | Runtime-width `NGramIndex`, `Indexes`, `Index` enum, candidates, explain, save/reopen |
| `grep.rs` | `Grep::run`, `CandidateFilter`, output modes |

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

# AGENTS.md -- sift-core

## Responsibility

Core engine for composable indexed code search: index registry, query planning, candidate narrowing, and grep-style matching.

The engine is designed around multiple coexisting configured indexes. `IndexConfig` records configured/persisted identity, `IndexStore` owns build/open/update snapshot transactions, the `Index` enum drives query-time candidate narrowing, and the `Indexes` registry intersects candidate sets from all available indexes. Today the default configured index is `IndexConfig::ngram(GramWidth::TRIGRAM)`.

## Public API

Primary search entrypoint (re-exported from `lib.rs`):

- `Session`, `Query`, `Report`, `Stats`, `MatchOptions`, `Inputs`, `Input`, `CandidatePolicy`
- Index types: `Indexes`, `Index`, `IndexConfig`, `IndexStore`, `NGramIndex`, `NGramConfig`, `GramWidth`, `Gram`
- Supporting grep types: `MatchFlags`, `CandidateFilter`, `CandidateScope`, `FileWalk`, `CompiledQuery`

Internal modules (`pub(crate)`): `corpus/`, `query/`.

## Source Map

| Module | Responsibility |
|--------|----------------|
| `index/` | `Indexes` registry, `IndexConfig`, `Index` enum, `IndexStore`, snapshot persistence |
| `index/ngram/` | Runtime-width N-gram index: build, load, search, storage (split submodules) |
| `grep/` | Public search API — `Query::candidates`, `Query::search` |
| `grep/session.rs` | `Session` — indexes, filter, store meta (data only) |
| `grep/policy.rs` | `CandidatePolicy`, `CandidatePolicyConfig`, `CandidateScope`, `CorpusState` |
| `grep/input.rs` | `Input`, `Inputs` — push API for paths and byte streams |
| `grep/query.rs` | `Query` lifecycle, candidate resolution entrypoint, library search entrypoint |
| `grep/compiled.rs` | `CompiledQuery`, regex engine selection, concrete matcher construction |
| `grep/collection.rs` | Private `ReportCollector` and search execution for library reports |
| `grep/matched.rs` | `Match` result type |
| `corpus/coverage.rs` | `CandidateCoverage` — shared planning enum |
| `corpus/order.rs` | `CandidateOrder` — sort keys for resolved candidates |
| `corpus/` | `Candidate`, `CandidateFilter`, `FileWalk` |
| `query/planner.rs` | Pure planning: `QueryPlanner::plan` → `ResolutionPlan` |
| `query/resolve.rs` | Candidate resolution I/O: `QueryPlanner::resolve` |

Output formatting lives in `sift-grep/src/format/` (not in core).

## Search Flow

```text
query.compile() -> CompiledQuery
CandidatePolicyConfig::policy(compiled) -> CandidatePolicy
Query::candidates(&session, policy) -> Vec<Candidate>
InputSources::build_inputs(candidates, transform) -> Inputs
Query::search(&inputs, stats_mode) -> Report   // library path

CLI format path:
  SearchPrinter::print(&inputs) -> Report       // uses grep-printer with concrete CompiledQuery matchers
```

## Error Ownership

`grep/error.rs` defines `grep::Error`. `crate::Error` wraps it as `Error::Search` (re-exported as `GrepError`).

## Invariants

- **Determinism:** parallel search merges hits sorted by `(file, line, text)`.
- **Index file order:** lexicographic relative paths (stable file IDs).
- **Conservative candidates:** `Index::candidates` may over-return but must not under-return.
- **Index independence:** each configured index narrows candidates independently; the registry combines results.

## Testing

```bash
cargo test -p sift-core
```

Integration tests: `crates/core/tests/`. Unit tests co-located in `#[cfg(test)]` blocks.

## Benchmarking

```bash
cargo bench -p sift-core --bench query
cargo bench -p sift-core --bench index
cargo bench -p sift-core --bench grep
```

See [`benches/README.md`](benches/README.md).

## Do NOT

- Break the public API without updating the CLI crate.
- Add `unsafe` outside `index/mmap.rs`.
- Have `grep/` import from concrete index modules such as `index::ngram`. Use `Index` enum only.
- Put stdout or output formatting in core — that belongs in `sift-grep`.
- Add `query/` → `grep/` dependencies; planning stays index-agnostic.

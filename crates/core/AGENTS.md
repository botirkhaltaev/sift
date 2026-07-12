# AGENTS.md -- sift-core

## Responsibility

Core engine for composable indexed code search: index registry, query planning, candidate narrowing, and grep-style matching.

The engine is designed around multiple coexisting configured indexes. `IndexConfig` records configured/persisted identity, `IndexStore` owns build/open/update snapshot transactions, and `Indexes` is the query-time facade (`availability`, `candidates`). Today the default configured index is `IndexConfig::ngram(GramWidth::TRIGRAM)`.

## Public API

Primary search entrypoint (re-exported from `lib.rs`):

- `Grep`, `GrepRequest`, `Searcher`, `Report`, `SearchInputs`, `Inputs`, `Input`
- Index types: `Indexes`, `IndexAvailability`, `Index`, `IndexConfig`, `IndexStore`, `NGramIndex`, `NGramConfig`, `GramWidth`, `Gram`
- Candidate types: `CandidatePlanner`, `CandidatePlan`, `Candidates`, `CandidateQuery`, `CandidateSelection`, `CandidateCoverage`, `CandidateSource`, `CandidateFilter`, `FileWalk`

Internal modules (`pub(crate)`): `corpus/`.

## Source Map

| Module | Responsibility |
|--------|----------------|
| `index/` | `Indexes` registry, `IndexConfig`, `Index` enum, `IndexStore`, snapshot persistence |
| `index/ngram/` | Runtime-width N-gram index: build, load, search, storage |
| `grep/` | Public search API — `Grep::search`, `Grep::stream` |
| `candidates/` | `CandidateSource`, `CandidatePlanner`, `CandidatePlan::resolve`, `Candidates` |
| `grep/input.rs` | `ByteInput`, stream helpers on `Inputs` |
| `search/input.rs` | `Input`, `Inputs`, `InputConversion`, `SearchInputs` |
| `search/searcher.rs` | `Searcher` execution by `SearchBound` |
| `candidates/planner.rs` | Pure `CandidatePlanner::plan` → `CandidatePlan` |
| `candidates/plan.rs` | `CandidatePlan::resolve` I/O boundary |
| `candidates/collection.rs` | `Candidates` (`IntoIterator`, `into_vec`) |
| `corpus/order.rs` | `CandidateOrder` — sort keys for resolved candidates |
| `corpus/` | `Candidate`, `CandidateFilter`, `FileWalk` |

Output formatting lives in `sift-grep/src/format/` (not in core).

## Search Flow

```text
Grep::execute
  1. coverage   ← GrepRequest::candidate_coverage()
  2. plan       ← CandidatePlanner::plan(source, candidate_query, selection, coverage)
  3. candidates ← plan.resolve()
  4. search     ← Searcher::execute(SearchInputs { candidates, streams, conversion }, …)
```

## Error Ownership

`grep/error.rs` defines `grep::Error`. `crate::Error` wraps it as `Error::Search` (re-exported as `GrepError`).

## Invariants

- **Determinism:** parallel search merges hits sorted by `(file, line, text)`.
- **Index file order:** lexicographic relative paths (stable file IDs).
- **Conservative candidates:** index narrowing may over-return candidates but must not under-return.
- **Index independence:** each configured index narrows candidates independently; the registry intersects results.
- **Planning is pure; resolve is I/O:** `CandidatePlanner::plan` makes decisions only; `CandidatePlan::resolve` is the single candidate I/O boundary.

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
- Expose raw index file ids or registry internals outside `index/`.
- Mix planning decisions with I/O in one function.

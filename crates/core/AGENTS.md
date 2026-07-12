# AGENTS.md -- sift-core

## Responsibility

Composable indexed code search: index lifecycle, candidate planning, and grep-style matching.

## Architecture

```
IndexStore (lifecycle)  →  Snapshot::open_current  →  Indexes (search)
CandidatePlanner::plan  →  CandidatePlan::resolve  →  Grep::search
```

- `IndexStore` — build/update/publish only
- `Snapshot` / `Indexes` — read/search (`from_snapshot`, `query`, `hydrate_*`)
- `Grep` — single public entry for resolve + search

Today the default index is `IndexConfig::ngram(GramWidth::TRIGRAM)`.

## Public API

Search (re-exported from `lib.rs`):

- `Grep`, `GrepRequest`, `Grep::resolve_candidates`, `Searcher`, `Report`
- `Indexes`, `Snapshot`, `IndexAvailability`, `IndexedCorpus`
- `Index`, `IndexConfig`, `IndexStore`, `NGramIndex`, `GramWidth`
- `Candidates`, `CandidateSelection`, `CandidateSource`, `CandidateCoverage`

Internal: `CandidatePlanner`, `CandidatePlan`, `CandidateQuery`.

## Source map

| Module | Responsibility |
|--------|----------------|
| `index/search.rs` | `Indexes` search facade |
| `index/snapshot/` | `Snapshot`, persistence |
| `index/store.rs` | `IndexStore` lifecycle |
| `index/ngram/` | N-gram implementation |
| `grep/` | Public search API |
| `candidates/planner.rs` | `CandidatePlanner` (pure planning) |
| `candidates/plan.rs` | `CandidatePlan`, `PlannedDiscovery`, resolve I/O |
| `candidates/candidates.rs` | `Candidates` collection |
| `corpus/` | `Candidate`, filters, walk |

## Search flow

```text
Grep::execute
  1. coverage   ← GrepRequest::candidate_coverage()
  2. plan       ← CandidatePlanner::plan(source, query, selection, coverage)
  3. candidates ← plan.resolve(source)
  4. search     ← Searcher::execute(...)
```

Planning is pure; `CandidatePlan::resolve` is the only candidate I/O boundary.

## Invariants

- Conservative narrowing: indexes may over-return, never under-return.
- Multi-index intersection in `Indexes::query`, not per-caller.
- No free helper functions — logic lives on the owning type.
- No callback/`FnOnce` APIs.

## Testing

```bash
cargo test -p sift-core
```

## Do NOT

- Break public API without updating CLI.
- Add `unsafe` outside `index/mmap.rs`.
- Import `index::ngram` from `grep/`.
- Put stdout formatting in core.
- Expose `Indexes::candidates` or test-only constructors.
- Mix planning with I/O.

# query/

Query description and candidate planning. Owns the logic that turns user patterns into an index-agnostic candidate plan.

## Key Types

- `QuerySpec` — neutral query description (patterns, flags).
- `QueryPlanner` — produces `CandidatePlan` from a `QuerySpec`.
- `CandidatePlan` — opaque plan enum (`FullScan`, `Trigram(...)`).
- `TrigramCandidatePlan` — trigram-specific narrowing plan with arms.

## Conventions

- Query planning is independent of any index implementation.
- Does not depend on `index/` or `grep/`.
- Trigram extraction utilities live in `trigram.rs` for use by both planning and index building.

## Do NOT

- Add index-specific logic (storage, file tables, postings).
- Depend on `grep/` types.

# query/

Query description and candidate planning. Owns the logic that turns user patterns into an index-agnostic candidate plan.

## Key Types

- `QuerySpec` — neutral query description (patterns, flags).
- `QueryPlanner` — produces `should_use_indexes` decision from a `QuerySpec`.

## Internal Types (crate-private)

- `CandidatePlan` — internal enum (`FullScan`, `Trigram(...)`).
- `TrigramCandidatePlan` — trigram-specific narrowing plan with arms.
- `Arm` — one OR branch: every trigram here must appear in a candidate file.

## Conventions

- Query planning is independent of any index implementation.
- Does not depend on `index/` or `grep/`.
- Trigram extraction utilities live in `trigram.rs` for use by both planning and index building.
- `CandidatePlan`, `TrigramCandidatePlan`, and `Arm` are `pub` within the private `query` module, re-exported as `pub(crate)` for use by `index/trigram`.

## Do NOT

- Add index-specific logic (storage, file tables, postings).
- Depend on `grep/` types.

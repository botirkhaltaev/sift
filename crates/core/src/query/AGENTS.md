# AGENTS.md -- query/

## Responsibility

Query planning and candidate resolution. Turns `QuerySpec` into candidates via strategy selection and I/O.

## Key Types

- `QuerySpec`: neutral query description (patterns, flags). Built by `Query::query_spec()` in `grep/pattern/`.
- `QueryPlanner`: `plan(ctx, coverage, walk_on_stale) -> ResolutionPlan` — pure strategy selection.
- `QueryPlanner::resolve`: plan + execute — primary entry for candidate resolution.
- `ResolutionPlan` / `ResolutionStrategy`: planner output; executed by `resolve.rs`.
- `PlanContext`: indexes, filter, store meta, index_capable flag.
- `ResolutionConfig`: per-run coverage, fallback, and candidate order.

## Design

Planning (`planner.rs`, `plan.rs`) is pure — no filesystem or index calls. Execution (`resolve.rs`) performs walks, index lookups, filtering, and ordering. `Query::candidates` delegates here; `grep/` only runs byte scanning.

## Conventions

- `grep/` must not embed planning or candidate I/O logic.
- `QuerySpec` is the interface to the index layer.
- `resolve.rs` may use `corpus::walk` and `Indexes`; `planner.rs` and `plan.rs` may not.

## Do NOT

- Put regex matching or hit collection in this module — that belongs in `grep/engine/`.
- Import from `grep/` (except shared corpus/index types).

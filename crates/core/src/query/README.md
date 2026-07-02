# query/

Query planning and candidate resolution.

## Design

`QuerySpec` is index-agnostic (patterns + flags). `QueryPlanner::plan` chooses a strategy (`UseIndex`, `WalkAll`, `AllIndexed`) from context and per-run policy. `QueryPlanner::resolve` plans and executes candidate resolution in `resolve.rs`.

## Key Types

| Type | Role |
|------|------|
| `QuerySpec` | Neutral query description for index narrowing |
| `QueryPlanner` | Strategy selection; `resolve` plans + executes |
| `QueryPlan` | Planner output |
| `PlanContext` | Persistent inputs: indexes, filter, store meta |
| `ResolvePolicy` | Per-run inputs: coverage, walk_fallback, order |

## Do NOT

- Put regex scanning in this module — that belongs in `grep/pattern/`
- Import from `grep/`

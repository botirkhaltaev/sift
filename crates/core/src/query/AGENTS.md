# AGENTS.md -- query/

## Responsibility

Query description and candidate planning. Turns user patterns into an index-agnostic query specification and orchestrates candidate resolution across the index registry.

## Key Types

- `QuerySpec`: neutral query description (patterns, flags). Consumed by every index kind for candidate narrowing.
- `QueryFlags`: bitflags for fixed strings, case insensitivity, word/line regexp, invert match.
- `QueryPlanner`: orchestrates candidate resolution by consulting the `Indexes` registry and falling back to filesystem walk when no index can narrow.
- `CandidateRequirement`: whether search needs all candidates or only potential matches.

## Design

The query layer is deliberately independent of any index implementation. `QuerySpec` describes what the user is searching for; each index kind decides how to use that specification. As Sift gains additional index types, the planner will evolve to choose among them and compose their results -- but it will never import index-specific logic.

## Conventions

- Query planning is independent of any index implementation.
- Does not depend on `index/` or `grep/`.
- `QuerySpec` is the sole interface between the query layer and the index layer.

## Do NOT

- Add index-specific logic (storage, file tables, postings).
- Depend on `grep/` types.
- Import from `crate::index::trigram` or any concrete index module.

# AGENTS.md -- index/

## Responsibility

Index registry, lifecycle dispatch, and concrete index implementations. Owns the build/open/update lifecycle for all index kinds and the query-time candidate narrowing registry.

## Composable Index Architecture

The index layer is designed for multiple coexisting index types:

- `IndexKind` enum: lifecycle dispatch (build, open, update). One variant per index type.
- `Index` enum: query-time dispatch (candidates, all_files). One variant per index type.
- `Indexes` registry: opens all index kinds from a snapshot and intersects their candidate sets.

Today both enums have one variant: `Trigram`. Future index kinds (AST, dependency graph, vector, etc.) are added as sibling variants and sibling modules to `trigram/`.

## Key Types

- `IndexKind`: tag enum for lifecycle dispatch; drives `build`, `open`, `update`.
- `Index`: opened index instance for query-time dispatch; drives `candidates`, `all_files`.
- `Indexes`: registry that opens all indexes in a `.sift` snapshot and intersects candidate sets.
- `IndexStore`: snapshot-based persistence orchestrator; atomic build/update/publish.
- `StoreMeta`: persistent manifest (`.sift/meta.json`) recording corpus, walk, filter, and index kind configuration.
- `IndexSource` / `IndexDestination`: domain enums for read/write dispatch (directory vs snapshot).
- `FileId`: type-safe file identifier within an index.
- `IndexId`: type-safe index identifier in a multi-index search.
- `Candidate`: single file candidate with `rel_path`, `abs_path`, filtering methods.

## Conventions

- Each index kind lives in its own submodule (sibling of `trigram/`).
- `grep/` only talks to `Index` enum, never to concrete index internals.
- `Index::candidates` may over-return but must not under-return (conservative).
- Each index kind narrows independently; the registry combines results.

## Adding a New Index Kind

1. Add a variant to `IndexKind` and `Index` in `kinds.rs`.
2. Implement `build`, `open`, `update` in the `IndexKind` match arms.
3. Implement `root`, `corpus_kind`, `candidates`, `all_files` in the `Index` match arms.
4. Create a sibling module to `trigram/` with the implementation.

No changes needed to `Indexes`, `IndexStore`, `QueryPlanner`, `search/`, or `grep/`.

## Do NOT

- Add trigram-specific logic outside `trigram/`.
- Make index implementations depend on `grep/` or `query/` internals.
- Change enum signatures without updating all match arms.
- Have one index kind depend on another.

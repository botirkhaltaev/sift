# AGENTS.md -- index/

## Responsibility

Index registry, configured index identity, lifecycle dispatch, and concrete index implementations. Owns build/open/update transactions for all configured indexes and the query-time candidate narrowing registry.

## Composable Index Architecture

The index layer is designed for multiple coexisting index types:

- `IndexConfig` enum: configured/persisted index identity. One variant per index family/configuration.
- `Index` enum: query-time dispatch (candidates, all_files). One variant per index type.
- `Indexes` registry: opens all configured indexes from a snapshot and intersects their candidate sets.

Today the default configured index is `IndexConfig::ngram(GramWidth::TRIGRAM)` / `Index::NGram`, backed by `ngram/`. Future index families (AST, dependency graph, vector, etc.) are added as sibling variants and sibling modules.

## Key Types

- `IndexConfig`: configured identity; drives persisted names, artifact names, and lifecycle routing.
- `Index`: opened index instance for query-time dispatch; drives `candidates`, `all_files`.
- `Indexes`: registry that opens all indexes in a `.sift` snapshot and intersects candidate sets.
- `IndexStore`: snapshot-based persistence orchestrator; atomic build/update/publish.
- `StoreMeta`: persistent manifest (`.sift/meta.json`) recording corpus, walk, filter, and index configuration.
- `IndexSource` / `IndexDestination`: domain enums for read/write dispatch (directory vs snapshot).
- `FileId`: type-safe file identifier within an index.
- `IndexId`: type-safe index identifier in a multi-index search.
- `Candidate`: single file candidate with `rel_path`, `abs_path`, filtering methods.
- `NGramIndex`: runtime-width N-gram implementation opened from persisted storage.

## Conventions

- Each index family lives in its own submodule (for example, `ngram/`).
- `grep/` only talks to `Index` enum, never to concrete index internals.
- `Index::plan` may over-return candidates but must not under-return (conservative).
- Each configured index narrows independently; the registry combines results.
- Keep index-family internals behind the family module; do not leak storage or extraction mechanics into callers.

## Adding a New Index Kind

1. Add a variant to `IndexConfig` and `Index` in `kinds.rs`.
2. Implement configured lifecycle routing in `IndexConfig` and query-time dispatch in `Index`.
3. Implement `root`, `corpus_kind`, `candidates`, `all_files` in the `Index` match arms.
4. Create a sibling module to `ngram/` with the implementation.

No changes needed to `Indexes`, `IndexStore`, `QueryPlanner`, or `grep/`.

## Do NOT

- Add N-gram specialization logic outside `ngram/`.
- Make index implementations depend on `grep/` or `query/` internals.
- Change enum signatures without updating all match arms.
- Have one index kind depend on another.

# AGENTS.md — index/

## Responsibility

Unified index trait (`Index`), shared types (`FileId`, `IndexId`, `IndexBuildConfig`, `IndexMeta`), and concrete index implementations.

## Key Types

- `Index` — unified trait for any indexed corpus (query surface + lifecycle: build, open, update, kind name).
- `FileId` — type-safe file identifier within an index.
- `IndexId` — type-safe index identifier in a multi-index search.
- `Candidate` — single file candidate with `rel_path`, `abs_path`, filtering methods.
- `IndexMeta` — serialized metadata (`sift.meta` JSON) with root path and single-file corpus flag.
- `TrigramIndex` — concrete trigram index implementation (in `trigram/`).
- `TrigramIndexBuilder` — fluent builder for trigram corpus indexing.

## Conventions

- Traits are simple and composable; no trigram-specific details leak through.
- `Index` exposes file/root access and candidate retrieval; each implementation decides how to narrow.
- `grep/` only talks to `Index` trait, never to concrete index internals.
- Future index kinds (symbol, suffix, etc.) are siblings of `trigram/`.

## Do NOT

- Add trigram-specific logic outside `trigram/`.
- Make traits depend on `grep/` or `query/` internals.
- Change trait signatures without updating all implementations.

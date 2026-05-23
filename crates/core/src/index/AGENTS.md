# AGENTS.md — index/

## Responsibility

Generic index trait (`SearchIndex`), shared types (`FileId`, `IndexId`, `FileCandidate`, `IndexMeta`), and concrete index implementations.

## Key Types

- `SearchIndex` — trait for any indexed corpus (file access, candidate lookup, single-file detection).
- `FileId` — type-safe file identifier within an index.
- `IndexId` — type-safe index identifier in a multi-index search.
- `FileCandidate` — resolved file with index_id, file_id, rel_path, abs_path.
- `IndexMeta` — serialized metadata (`sift.meta` JSON) with root path and single-file corpus flag.
- `TrigramIndex` — concrete trigram index implementation (in `trigram/`).
- `TrigramIndexBuilder` — fluent builder for trigram corpus indexing.

## Conventions

- Traits are simple and composable; no trigram-specific details leak through.
- `SearchIndex` exposes file/root access and candidate retrieval; each implementation decides how to narrow.
- `grep/` only talks to `SearchIndex` trait, never to concrete index internals.
- Future index kinds (symbol, suffix, etc.) are siblings of `trigram/`.

## Do NOT

- Add trigram-specific logic outside `trigram/`.
- Make traits depend on `grep/` or `query/` internals.
- Change trait signatures without updating all implementations.

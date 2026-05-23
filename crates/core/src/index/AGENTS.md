# AGENTS.md — index/

## Responsibility

Generic index traits (`Index`, `CandidateSource`), shared types (`FileId`, `CorpusKind`, `IndexMeta`), and concrete index implementations.

## Key Types

- `Index` — trait for any indexed corpus (file access, candidate lookup).
- `CandidateSource<P>` — trait for candidate retrieval given a query plan type.
- `FileId` — type-safe file identifier wrapping `usize`.
- `CorpusKind` — directory or single-file corpus enumeration.
- `IndexMeta` — serialized metadata (`sift.meta` JSON).
- `TrigramIndex` — concrete trigram index implementation (in `trigram/`).
- `TrigramIndexBuilder` — fluent builder for trigram corpus indexing.

## Conventions

- Traits are simple and composable; no trigram-specific details leak through.
- `Index` exposes only file/root access; candidate retrieval is separate via `CandidateSource<P>`.
- `grep/` only talks to these traits, never to concrete index internals.
- Future index kinds (symbol, suffix, etc.) are siblings of `trigram/`.

## Do NOT

- Add trigram-specific logic outside `trigram/`.
- Make traits depend on `grep/` or `query/` internals.
- Change trait signatures without updating all implementations.

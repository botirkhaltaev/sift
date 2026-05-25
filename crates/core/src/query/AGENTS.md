# AGENTS.md -- query/

Query description and candidate planning. Owns the logic that turns user patterns into an index-agnostic query specification.

## Key Types

- `QuerySpec`: neutral query description (patterns, flags).
- `QueryFlags`: bitflags for fixed strings, case insensitivity, word/line regexp, invert match.

## Conventions

- Query planning is independent of any index implementation.
- Does not depend on `index/` or `grep/`.

## Do NOT

- Add index-specific logic (storage, file tables, postings).
- Depend on `grep/` types.

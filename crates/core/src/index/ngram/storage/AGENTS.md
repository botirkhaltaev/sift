# AGENTS.md -- index/ngram/storage/

Binary persistence format for N-gram index tables. Read/write `files.bin`, `lexicon.bin`, `postings.bin`, and `grams.bin` with zero-copy memory-mapped access.

## Key Types

- `LexiconEntry`: gram + postings offset + length.
- `Lexicon`: memory-mapped lexicon with binary-search lookup.
- `Postings`: memory-mapped postings blob.
- `GramSet`: sorted unique grams for one file.
- `GramSets`: memory-mapped per-file gram sets for incremental updates.

## Conventions

- All integers are little-endian.
- Each file starts with an 8-byte magic header for format identification.
- Width-bearing files persist and validate gram width.
- Lexicon entries are sorted by gram ordinal, enabling binary search.
- The only `unsafe` in the index crate lives in `index/mmap.rs` with a documented safety invariant.

## Do NOT

- Add `unsafe` without documenting the safety invariant in `index/mmap.rs`.
- Add backward-compatible reads for removed formats unless a concrete migration requirement exists.

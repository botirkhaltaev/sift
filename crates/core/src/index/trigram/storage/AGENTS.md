# AGENTS.md -- index/trigram/storage/

Binary persistence format for the trigram index. Read/write `files.bin`, `lexicon.bin`, and `postings.bin` with zero-copy memory-mapped access.

## Key Types

- `LexiconEntry`: trigram + postings offset + length.
- `MappedLexicon`: memory-mapped lexicon with binary-search lookup.
- `MappedPostings`: memory-mapped postings blob.
- `MappedFilesView`: memory-mapped file table.

## Conventions

- All integers are little-endian.
- Each file starts with an 8-byte magic header for format identification.
- Lexicon entries are sorted by trigram bytes (enables binary search).
- The only `unsafe` in the crate lives in `mmap.rs` with a documented safety invariant.

## Do NOT

- Change magic bytes without bumping the format version.
- Add `unsafe` without documenting the safety invariant.
- Break backwards compatibility of the binary format without migration support.

# Changelog

All notable changes to this project will be documented in this file.
## [Unreleased]

### Bug Fixes

- eliminate #[allow], split benchmarks per module, add profiling (#22)
- rustfmt import formatting
- remove unused rel_match_context helper, use string literals
- Windows clippy, context prefix formatting, and expanded tests

### Documentation

- mark no-op flags (line-buffered, block-buffered, mmap, no-mmap) explicitly
- rewrite READMEs and add AGENTS.md to all projects and modules (#18)
- update Linux benchsuite snapshot with fresh results and chart
- tighten agent notes — scannable layout, no policy essays
- branch-per-phase workflow before roadmap slices

### Features

- modular public-API-only benchmarks (#26)
- comprehensive unit and integration test coverage for sift-core (#25)
- add --no-config, --unicode/--no-unicode, --colors, --regex-size-limit, --dfa-size-limit, -M/--max-columns, --max-columns-preview flags
- add --no-config, --unicode/--no-unicode, --colors, --regex-size-limit, --dfa-size-limit, -M/--max-columns, --max-columns-preview flags
- add -j/--threads, --line-buffered, --block-buffered, --path-separator, --one-file-system, -U/--multiline, --multiline-dotall, --crlf, --mmap/--no-mmap flags
- add -j/--threads, --line-buffered, --block-buffered, --path-separator, --one-file-system, -U/--multiline, --multiline-dotall, --crlf, --mmap/--no-mmap flags
- add -r/--replace, --trim, -b/--byte-offset, --passthru, --include-zero flags
- add -r/--replace, --trim, -b/--byte-offset, --passthru, --include-zero flags
- add --no-ignore-parent, --no-ignore-global, --no-ignore-exclude, --no-messages, --no-ignore-messages, --no-ignore-files, --ignore-file flags
- add --no-ignore-parent, --no-ignore-global, --no-ignore-exclude, --no-messages, --no-ignore-messages, --no-ignore-files, --ignore-file flags
- add -a/--text and --binary flags for binary file handling
- add -a/--text and --binary flags for binary file handling
- add filter flags for max-depth, max-filesize, types, iglob, files, sort
- add --max-depth, --max-filesize, --iglob, --ignore-file, --files, -t/--type, -T/--type-not, --type-list, --type-add, --type-clear, --sort/--sortr filter flags
- add --context-separator, --no-context-separator, --field-match-separator, --field-context-separator flags (#10)
- add --column, --vimgrep, --pretty, -N/--no-line-number, --version flags (#9)
- implement scope-based path display resolution
- --json JSON Lines output (ripgrep-compatible) (#6)
- bytes searched in SearchStats and --stats (#5)
- elapsed time in SearchStats and --stats output (#4)
- --stats and SearchStats counters (#3)
- --color, --null, grouped output structs (#2)
- context lines (-A/-B/-C) for standard search
- search parity — paths, ignores, follow, filter pipeline

### Miscellaneous

- remove sift-profile binary from core crate (#23)
- fix pre-existing clippy lints (map_or, is_ok_and, byte str literals) (#17)
- remove fff.nvim
- remove useless scripts
- remove unused #[allow(dead_code)] from rel_match_context

### Performance

- sift-profile revamp, matcher/searcher caches, parallel and index tuning

### Refactor

- split search and grep modules, add index intersection planning (#30)
- remove parallel threshold, always use Rayon (#28)
- split grep module into domain folders (#27)
- restructure core into index/, grep/, and query/ modules (#24)
- harden integration test suite with TestProject helper (#21)
- organize CLI into domain-oriented modules (#19)
- replace Option<bool> with ColumnAction enum for max_columns
- avoid needless String allocation and double trim_start()
- add doc comments to ignore-granular structs
- rename parse_filesize to parse_size_suffix for consistency

### Testing

- add 200 inline unit tests, convert CLI to lib+bin layout (#20)

## [0.1.2](https://github.com/botirk38/sift/releases/tag/v0.1.2) — 2026-04-02

### Bug Fixes

- remove double-filtering bug in candidate pipeline
- use line_path for path extraction in glob integration tests
- wire IgnoreSources into SearchFilter with ripgrep defaults
- correct glob filter semantics and add integration tests
- separate quiet from output mode via OutputEmission enum
- reject -m 0 with error exit code (ripgrep-compatible)
- make -m/--max-count per-file (ripgrep-compatible semantics)

### Features

- add --glob-case-insensitive flag
- add --no-filename with ripgrep-compatible semantics
- add --count-matches, fix -c/-o normalization, omit zero-count files
- add -g/--glob path filtering with ignore::overrides
- add -h/--no-filename and --help flags
- add -s/--case-sensitive, -S/--smart-case with ripgrep-compatible precedence

### Performance

- preallocate postings buffer; add perf-baseline script
- parallel filter+prep pipeline, CandidateInfo, P0 bytes fix

### Refactor

- redesign benchmark suite with filter, mode, and output scenarios
- typed SearchFilter abstraction for search-time filtering
- output modes use ripgrep last-flag-wins semantics
- move output mode resolution into run_search, add conflict detection

### Reverted

- restore -h to ripgrep-compatible help, remove broken -g

### Audit

- align planner precedence with verify, add -w/-x combination tests

## [0.1.1](https://github.com/botirk38/sift/releases/tag/v0.1.1) — 2026-03-24

### Bug Fixes

- use serde_json to serialize index metadata in test
- normalize test paths across platforms
- normalize test paths across platforms
- normalize cli path tests and chart from csv
- skip binary files and symlinked files by default
- support single-file corpora with JSON index metadata
- update bench/profile to use .sift layout

### Documentation

- replace remote chart with local asset
- add generated performance chart

### Features

- tune parallel threshold

### Miscellaneous

- bump version to 0.1.1
- initialize benchsuite with uv

### Performance

- refactor search runtime for faster scans
- rewrite planner with Unicode-aware HIR extraction
- add profiling infrastructure (criterion benches, benchsuite upgrades)

### Testing

- add comprehensive integration test suite (28 new tests)

### Index

- migrate storage layout to .sift/.index
- switch to mmap-backed storage with O(1) file lookup

### Search

- use cached paths and id-based candidates
- reduce candidate path and printer overhead
- align execution with ripgrep printer pipeline
- normalize CLI output and migrate scanning to grep stack

## [0.1.0](https://github.com/botirk38/sift/releases/tag/v0.1.0) — 2026-03-24

### Documentation

- README/AGENTS per crate; CI on Linux/macOS/Windows

### Features

- indexed search with prefilter, parallel index build, and clippy-clean profile

### Miscellaneous

- simplify publish workflow — rely on ci.yml, not own validate job
- add sift-core publish workflow on tag push
- remove plan.md
- add skills.sh-installable sift-cli skill under skills/

### Refactor

- move Index into index/ module with IndexBuilder

### Testing

- reorganize CLI integration coverage

### Search

- use regex-automata with explicit cache management
- skip redundant canonicalize in indexed search
- cache parallel scan threshold with OnceLock
- byte-first scanning with regex::bytes::Regex, remove prefilter



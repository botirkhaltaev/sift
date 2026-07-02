# Sift: Implementation Plan (v2)

Architecture-first. Inline over extract. No backward-compat constraints.

---

## Architectural Assessment

### What's good

- **Core domain model:** `Index` enum with static dispatch, `IndexSource`/`IndexDestination`
  lifecycle pattern, `QuerySpec` decoupled from indexes — all solid.
- **Error hierarchy:** `thiserror` in core, `anyhow` in CLI, per-module error types aggregated
  via `From` impls — textbook.
- **CLI two-layer model:** `*Decl` (clap) → resolved domain types. Domain modules never import
  `Cli`. Clean separation.
- **Dependency direction:** CLI → Core, never reverse. Correct.

### What needs work

1. **`lib.rs` flat re-export blast** — `search` is private (`mod search`) but ~40 types are
   dumped into the crate root via individual `pub use` lines. This hides the module hierarchy,
   makes the public API surface look flat, and creates naming pressure (hence `WalkOptions` vs
   `IndexWalkOptions`). Making `search` public and thinning the root re-exports restores
   discoverability.

2. **`Cli::dispatch(&self)` forces unnecessary clones** — Called exactly once before exit, yet
   borrows `self`, forcing every config builder (`pattern_config`, `filter_config`,
   `output_config`, `grep_config`) to `.clone()` every field. Consuming `self` lets fields move.

3. **`SearchQuery` leaks internal state** — `pub patterns`, `pub opts`, `pub matcher` expose
   a lazy-init `OnceLock` and mutable options to any caller. Fields should be private with
   accessors for the immutable parts.

4. **`SearchExecution` owns candidates it only borrows** — `candidates: Vec<Candidate>` forces
   the caller to surrender ownership for a temporary search. Should be `&'a [Candidate]`.

5. **`files_mode: bool`** — Violates the AGENTS.md rule: "Use enums for real alternatives...
   Avoid boolean flags when a named domain type would make intent clearer."

6. **Dead code** — `searcher_cache` field initialized but never read; `SearcherCacheEntry` type
   alias used only by it; no-op `Drop` impl on `CurrentSnapshot`.

7. **Orphan impl** — `Candidate::total_file_bytes` defined in `search/emit/format.rs`, far from
   `candidate.rs` where the type lives.

8. **`WalkOptions` name collision** — Two types with the same name in different modules, papered
   over with a re-export alias.

---

## Phase 1 — Architecture (do first, cascading changes)

### A1. Make `search` module public + restructure `lib.rs` re-exports

**Why:** The ~40-type re-export blast at the crate root hides module structure. Making `search`
public lets consumers use `sift_core::search::SearchMode` while keeping only the primary API
types at the root.

**File:** `crates/core/src/lib.rs`

```rust
// OLD:
mod search;
pub use search::{
    BinaryMode, CandidateFilter, CandidateFilterConfig, CaseMode, ColorChoice, ColumnLimit,
    ColumnOverflow, FilenameMode, GlobConfig, HiddenMode, IgnoreConfig, IgnoreSources,
    LineStyleFlags, LinkTraversal, Match, MatchEmissionMode, OutputEmission, PassthruMode,
    PathDisplay, PatternCompiler, RecordTerminator, SearchCollection, SearchError, SearchLineStyle,
    SearchSearchFlags, SearchMode, SearchOptions, SearchOutcome, SearchOutput, SearchOutputFormat,
    SearchQuery, SearchRecordStyle, SearchSeparators, SearchStats, TypeDef, VisibilityConfig,
    WalkOptions, ZeroCountMode,
};

// NEW:
pub mod search;
pub use search::{SearchError, SearchOutcome, SearchQuery};
```

Only the three primary entry-point types remain at root. Everything else is accessed via
`sift_core::search::*`. This is a **public API change** — all CLI files that import search
types from `sift_core::` root must update to `sift_core::search::`.

**Cascading CLI import updates** (exhaustive list from grep):

| File | Old import | New import |
|------|-----------|------------|
| `grep/run.rs:3` | `sift_core::{CandidateFilter, ..., SearchMode, SearchQuery}` | `sift_core::{SearchQuery, ...}` + `sift_core::search::{CandidateFilter, SearchMode}` |
| `grep/pattern.rs:4` | `sift_core::{BinaryMode, CaseMode, SearchSearchFlags, SearchMode, SearchOptions}` | `sift_core::search::{BinaryMode, CaseMode, SearchSearchFlags, SearchMode, SearchOptions}` |
| `grep/filter.rs:5` | `sift_core::{CandidateFilterConfig, GlobConfig, IgnoreConfig, TypeDef, VisibilityConfig}` | `sift_core::search::{CandidateFilterConfig, GlobConfig, IgnoreConfig, TypeDef, VisibilityConfig}` |
| `grep/output.rs:2-6` | `sift_core::{ColorChoice, FilenameMode, ...}` | `sift_core::search::{ColorChoice, FilenameMode, ...}` |
| `grep/ignore.rs:2` | `sift_core::IgnoreSources` | `sift_core::search::IgnoreSources` |
| `grep/paths.rs:4` | `sift_core::{..., PathDisplay, ...}` | `sift_core::search::PathDisplay` (keep non-search types at root) |
| `index/daemon/mod.rs:884` | `sift_core::VisibilityConfig` | `sift_core::search::VisibilityConfig` |

Also update test/bench files that import search types from `sift_core::` root.

**Core internal callers** — files within `crates/core/` use `crate::search::*` paths already,
so no changes needed there. Only `crates/core/src/lib.rs` tests that use `use super::*` need
checking — they'll still work because `SearchOptions` etc are brought in via `pub mod search`.

---

### A2. Consume `Cli` in `dispatch` — move fields instead of cloning

**Why:** `dispatch(&self)` is called exactly once in `main_entry()`, then the program exits.
Borrowing forces ~15 `.clone()` calls on Decl structs that could just move.

**File:** `crates/cli/src/lib.rs:16-20`

```rust
// OLD:
pub fn main_entry() -> ExitCode {
    let cli = Cli::parse();
    let argv_storage = Argv::from_env();
    let argv = Argv::new(&argv_storage);
    cli.dispatch(&argv)
}

// NEW (same — dispatch signature changes below):
pub fn main_entry() -> ExitCode {
    let cli = Cli::parse();
    let argv_storage = Argv::from_env();
    let argv = Argv::new(&argv_storage);
    cli.dispatch(&argv)
}
```

**File:** `crates/cli/src/cli.rs`

```rust
// OLD:
pub fn dispatch(&self, argv: &Argv<'_>) -> ExitCode { ... }
fn dispatch_route(&self) -> DispatchRoute { ... }
fn index_request(&self) -> IndexRequest { ... }
pub fn grep_config(&self) -> GrepConfig { ... }
pub fn pattern_config(&self) -> PatternConfig { ... }
pub fn filter_config(&self) -> FilterConfig { ... }
fn output_config(&self, ...) -> OutputConfig { ... }
fn exit_grep(&self, ...) -> ExitCode { ... }
fn exit_index(&self, ...) -> ExitCode { ... }

// NEW:
pub fn dispatch(self, argv: &Argv<'_>) -> ExitCode {
    // Inline dispatch_route logic — no separate method needed
    if self.filter_decl.type_list {
        TypeCatalog::from_decl(&self.filter_decl).print_list();
        return ExitCode::SUCCESS;
    }
    match self.command {
        Some(Commands::Update) => Self::exit_update(),
        Some(Commands::Index { command }) => {
            // Inline index_request — removes the expect+unreachable double-guard
            let filter = self.filter_config();
            let req = Self::build_index_request(&command, &self.paths, &self.walker_decl, &filter);
            let daemon = self.paths.daemon();
            match IndexJob::resolve(req) {
                Ok(index) => index.run(daemon.as_ref(), argv),
                Err(e) => { eprintln!("sift: {e}"); ExitCode::from(2) }
            }
        }
        None => {
            let daemon = self.paths.daemon();
            let suppress_errors = SearchFilterCtx::resolve(argv)
                .ignore.msg_flags.contains(MessageFlags::NO_MESSAGES);
            // into_grep_config moves fields instead of cloning
            let grep = Grep::new(self.into_grep_config());
            Self::exit_from_grep(grep.run(argv, daemon.as_ref()), suppress_errors)
        }
    }
}
```

New consuming config builder:
```rust
fn into_grep_config(self) -> GrepConfig {
    let search_paths = self.search_scope.paths;      // moved, not cloned
    let mode = if self.filter_decl.files {
        GrepMode::ListFiles
    } else {
        GrepMode::Search
    };
    GrepConfig {
        pattern: PatternConfig {
            patterns: self.patterns,                   // moved
            search_flags: self.search_flags,           // moved
            regex1: self.regex1,                       // moved
            regex2: self.regex2,                       // moved
            multiline: self.multiline_decl,            // moved
            engine: self.engine_decl,                  // moved
            binary: self.binary_decl,                  // moved
            replace: self.replace_decl,                // moved
            max_count: self.paths.max_count,           // Copy
        },
        filter: FilterConfig {
            decl: self.filter_decl,                    // moved
            glob_patterns: self.glob_flags.glob,       // moved
            follow_links: self.paths.follow,           // Copy
            one_file_system: self.walker_decl.one_file_system, // Copy
        },
        output: OutputConfig {
            column: self.column_decl,                  // moved
            columns: self.columns_decl,                // moved
            extra: self.extra_output,                  // moved
            replace_trim: self.replace_decl.trim,      // ... wait, replace_decl was moved above
        },
        sift_dir: self.paths.sift_dir,                 // moved
        search_paths,
        threads: self.threading.threads,               // Copy
        mode,
    }
}
```

**Note:** `replace_decl.trim` is needed in `OutputConfig` but `replace_decl` is moved into
`PatternConfig`. Solution: read `trim` before the move, or restructure slightly. The `trim`
field is `bool` (Copy), so extract it first:

```rust
fn into_grep_config(self) -> GrepConfig {
    let search_paths = self.search_scope.paths;
    let replace_trim = self.replace_decl.trim;
    let mode = if self.filter_decl.files { GrepMode::ListFiles } else { GrepMode::Search };
    GrepConfig {
        pattern: PatternConfig { replace: self.replace_decl, ... },
        output: OutputConfig { replace_trim, ... },
        ...
    }
}
```

Delete: `dispatch_route`, `index_request`, `grep_config`, `pattern_config`, `output_config`,
`exit_grep`, `exit_index` (all inlined into `dispatch` or `into_grep_config`).

Keep: `filter_config(&self)` (used in both Index and Grep branches, borrows self).

**Adjustment:** Since `filter_config` is called in the Index branch before `self` is fully
consumed, it needs to borrow. For the Grep branch, `into_grep_config` builds the filter inline.
For the Index branch, we build it from references:

```rust
fn build_index_request(
    command: &IndexCommands,
    paths: &PathArgs,
    walker: &WalkerDecl,
    filter: &FilterConfig,
) -> IndexRequest { ... }
```

This is a static method (no `&self`) since it doesn't need the full Cli.

---

### A3. `SearchQuery` — private fields + `opts()` accessor

**File:** `crates/core/src/search/query/mod.rs`

```rust
// OLD:
pub struct SearchQuery {
    pub patterns: Vec<String>,
    pub opts: SearchOptions,
    pub matcher: OnceLock<RegexMatcher>,
    pub searcher_cache: Mutex<Option<SearcherCacheEntry>>,  // dead — removed in B1
}

// NEW (after B1 removes dead fields):
pub struct SearchQuery {
    patterns: Vec<String>,
    opts: SearchOptions,
    matcher: OnceLock<RegexMatcher>,
}
```

Add accessor:
```rust
#[must_use]
pub fn opts(&self) -> &SearchOptions {
    &self.opts
}
```

Existing `patterns()` accessor already exists. `matcher` is internal — no accessor needed.

**Callers that access `query.opts` directly** (all crate-internal):

| File | Line | Change |
|------|------|--------|
| `grep/mod.rs` | 46 | `query.opts.max_results` → `query.opts().max_results` |
| `search/scan/standard.rs` | 368 | `scan.search.opts.max_results` → `scan.search.opts().max_results` |
| `search/scan/standard.rs` | 377 | `scan.search.opts.replace.clone()` → `scan.search.opts().replace.clone()` |
| `search/scan/standard.rs` | 379-380 | `.opts.before_context` / `.after_context` → `.opts().` |
| `search/scan/json.rs` | 40 | `scan.search.opts.max_results` → `scan.search.opts().max_results` |
| `search/scan/summary.rs` | 199 | `scan.search.opts.max_results` → `scan.search.opts().max_results` |
| `search/query/matcher.rs` | 18+ | `self.opts.*` — same module, still direct access |
| test at mod.rs:422 | | `search.opts.case_insensitive()` → `search.opts().case_insensitive()` |

---

### A4. `SearchExecution.candidates`: `Vec<Candidate>` → `&'a [Candidate]`

**File:** `crates/core/src/search/request/mod.rs:31`

```rust
// OLD:
pub struct SearchExecution<'a> {
    pub candidates: Vec<Candidate>,

// NEW:
pub struct SearchExecution<'a> {
    pub candidates: &'a [Candidate],
```

**Callers:**

| File | Change |
|------|--------|
| `grep/mod.rs:78-79` | `candidates,` → `candidates: &candidates,` |
| `search/query/mod.rs:106` | `let candidates = &execution.candidates;` → `let candidates = execution.candidates;` |

---

### A5. `GrepMode` enum replacing `files_mode: bool`

**File:** `crates/cli/src/grep/run.rs`

```rust
// ADD before GrepConfig:
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrepMode {
    Search,
    ListFiles,
}

// CHANGE in GrepConfig:
// OLD: pub files_mode: bool,
// NEW: pub mode: GrepMode,

// CHANGE in Grep::run:
// OLD:
if self.config.files_mode { ... } else { ... }
// NEW:
match self.config.mode {
    GrepMode::ListFiles => self.run_files(argv).map(|found| GrepOutcome::Files { found }),
    GrepMode::Search => self.run_search(argv, daemon).map(|matched| GrepOutcome::Search { matched }),
}
```

Construction site in `cli.rs` changes as part of A2 (`into_grep_config`).

---

### A6. Rename `index::config::WalkOptions` → `IndexWalkConfig`

**File:** `crates/core/src/index/config.rs`

```rust
// OLD: pub struct WalkOptions { ... }
// NEW: pub struct IndexWalkConfig { ... }
```

**File:** `crates/core/src/lib.rs`

```rust
// OLD: pub use index::config::WalkOptions as IndexWalkOptions;
// NEW: pub use index::config::IndexWalkConfig;
```

Update all references (17 sites across core tests, benches, and internal modules):
`IndexWalkOptions` → `IndexWalkConfig` everywhere.

---

## Phase 2 — Dead Code & Type Hygiene

### B1. Remove dead `searcher_cache` field + `SearcherCacheEntry` type alias

**File:** `crates/core/src/search/query/mod.rs`

Delete:
- Line 34: `type SearcherCacheEntry = ...`
- Line 2: `use std::sync::Mutex;`
- Line 10: `use grep_searcher::Searcher;` (only used by the dead type alias; `matcher.rs` has its own import)
- Line 48: `pub searcher_cache: Mutex<Option<SearcherCacheEntry>>,`
- Line 65: `searcher_cache: Mutex::new(None),`

### B2. Remove no-op `Drop` for `CurrentSnapshot`

**File:** `crates/core/src/index/snapshot/mod.rs:34-38`

Delete the entire impl block. `let _ = &mut self.lease` is a no-op — `SnapshotLease::drop`
fires automatically.

### B3. Move `Candidate::total_file_bytes` to `candidate.rs`

**File:** `crates/core/src/search/emit/format.rs` — delete the `impl Candidate` block (lines 7-15)
and the `use crate::Candidate;` import (line 1, if it becomes unused — check if ANSI consts
still need it; they don't, they're standalone).

**File:** `crates/core/src/candidate.rs` — add the method to the existing `impl Candidate` block.

All callers use `crate::Candidate::total_file_bytes(...)` which still resolves.

### B4. Derive `Default` where manual impl is identical

**`LinkTraversal`** (`search/request/mod.rs`):
```rust
// ADD: #[derive(Default)] and #[default] on DoNotFollow
// DELETE: manual Default impl for WalkOptions (20-27)
```

**`WalkOptions`** (`search/request/mod.rs`):
```rust
// ADD: #[derive(Default)]
// DELETE: manual Default impl (19-28)
```

**`RecordTerminator`** (`search/output/style.rs`):
```rust
// ADD: #[derive(Default)] and #[default] on Newline
```

**`SearchRecordStyle`** (`search/output/style.rs`):
```rust
// ADD: #[derive(Default)]
// DELETE: manual Default impl (113-121)
```

**`SearchLineStyle`** (`search/output/style.rs`):
```rust
// ADD: #[derive(Default)]
// DELETE: manual Default impl (69-78)
```

**`SearchOutput`** (`search/output/mod.rs`):
```rust
// ADD: #[derive(Default)]
// DELETE: manual Default impl (30-42)
```

Each derived `Default` produces values identical to the manual impl. Verified by checking
every field type's `#[default]` attribute.

**`SearchOptions`** — CANNOT derive: `unicode: true` differs from `bool::default()` (`false`).
Leave manual impl.

---

## Phase 3 — Idiom Fixes (inline, no helpers extracted)

### C1. `OnceLock::get_or_try_init` replaces manual init

**File:** `crates/core/src/search/query/mod.rs:152-159`

```rust
// OLD:
fn resolve_matcher(&self) -> Result<&RegexMatcher, SearchError> {
    if let Some(m) = self.matcher.get() {
        return Ok(m);
    }
    let m = self.build_matcher()?;
    let _ = self.matcher.set(m);
    Ok(self.matcher.get().expect("just initialised"))
}

// NEW:
fn resolve_matcher(&self) -> Result<&RegexMatcher, SearchError> {
    self.matcher.get_or_try_init(|| self.build_matcher())
}
```

### C2. `bitflags::set()` in `PatternCompiler` builders

**File:** `crates/core/src/search/pattern/mod.rs:25-46`

```rust
// OLD (each method):
pub fn fixed_strings(mut self, on: bool) -> Self {
    if on { self.flags |= SearchSearchFlags::FIXED_STRINGS; }
    self
}

// NEW:
pub fn fixed_strings(mut self, on: bool) -> Self {
    self.flags.set(SearchSearchFlags::FIXED_STRINGS, on);
    self
}
```

Same for `word_regexp` and `line_regexp`. This also **fixes a bug**: the old code silently
ignores `false` (never clears a flag), while `set(flag, false)` correctly removes it.

### C3. Stack-buffer path separator

**File:** `crates/core/src/candidate.rs:56-58`

```rust
// OLD:
let sep_char = sep as char;
raw.replace(std::path::MAIN_SEPARATOR, &sep_char.to_string())

// NEW:
let mut buf = [0u8; 4];
let sep_str = (sep as char).encode_utf8(&mut buf);
raw.replace(std::path::MAIN_SEPARATOR, sep_str)
```

### C4. `swap_remove` instead of clone in `PatternCompiler::compile`

**File:** `crates/core/src/search/pattern/mod.rs:76-78`

```rust
// OLD:
let branches: Vec<String> = patterns.iter().map(|p| self.shape(p)).collect();
let combined = if branches.len() == 1 {
    branches[0].clone()

// NEW:
let mut branches: Vec<String> = patterns.iter().map(|p| self.shape(p)).collect();
let combined = if branches.len() == 1 {
    branches.swap_remove(0)
```

### C5. Storage deserialization read helpers

**Why:** ~25 repetitions of `T::from_le_bytes(bytes[a..b].try_into().unwrap())` across 4 files.
Unlike the small helpers the user wants inlined, these replace 25 identical call sites with a
shared utility — the storage AGENTS.md owns this domain.

**File:** `crates/core/src/index/trigram/storage/mod.rs` — add:

```rust
pub(super) fn read_u32_le(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("slice is exactly 4 bytes"),
    )
}

pub(super) fn read_u64_le(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        bytes[offset..offset + 8]
            .try_into()
            .expect("slice is exactly 8 bytes"),
    )
}

pub(super) fn read_i64_le(bytes: &[u8], offset: usize) -> i64 {
    i64::from_le_bytes(
        bytes[offset..offset + 8]
            .try_into()
            .expect("slice is exactly 8 bytes"),
    )
}
```

Then replace all bare-unwrap integer reads across `lexicon.rs` (~9), `postings.rs` (~1),
`trigram_sets.rs` (~4), `file_table.rs` (~7). `file_table.rs` is a sibling of `storage/`
so it uses `crate::index::trigram::storage::read_u32_le`.

Trigram-byte array conversions (`chunk[..3].try_into().unwrap()`) get contextual expect
messages: `.expect("3-byte trigram")`.

---

## Dropped from previous plan

| Item | Why dropped |
|------|------------|
| Extract error mapping closure in `Indexes::open` | User prefers inlining; duplicated match is only 2 sites and logic is local |
| Double clone fix in `lifecycle.rs:246` merge | Marginal; `.cloned().map(...)` is no clearer than current code |
| Extract `IndexJob::validate_preconditions` | User prefers inlining; duplicated match is 2 sites, helper adds indirection without value |
| Pass `IndexCommands` directly to `index_request` | Subsumed by A2 — `index_request` is eliminated entirely; command is destructured inline |

---

## Implementation Order

```
B1  Remove dead searcher_cache         (no deps)
B2  Remove no-op Drop                  (no deps)
C1  get_or_try_init                    (no deps)
C2  bitflags::set()                    (no deps)
C3  Stack-buffer path separator        (no deps)
C4  swap_remove in compile             (no deps)
B3  Move total_file_bytes              (no deps)
B4  Derive Default (chain: RecordTerminator → SearchRecordStyle → SearchLineStyle → SearchOutput; LinkTraversal → WalkOptions)
A6  Rename WalkOptions → IndexWalkConfig
A3  SearchQuery private fields + opts()
A4  SearchExecution.candidates as borrow
C5  Storage read helpers
A5  GrepMode enum
A1  Make search module public + thin root re-exports
A2  Cli::dispatch(self) + into_grep_config (depends on A5 for GrepMode)
```

Items without dependencies can be done in parallel. A1 and A2 are last because they have the
widest blast radius.

---

## Summary

| Phase | Changes | Nature |
|-------|---------|--------|
| Architecture (A1-A6) | 6 | Structural: public API, ownership, encapsulation, domain types |
| Dead code & type hygiene (B1-B4) | 4 | Cleanup: remove dead code, derive where identical |
| Idiom fixes (C1-C5) | 5 | Inline improvements: no new helpers except storage readers |

**Total: 15 changes. ~30 files touched.**
No new dependencies. No `unsafe`. No `#[allow]`.

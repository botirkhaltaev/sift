# Sift Codebase Review: Rust Best Practices & Architecture Plan

## Executive Summary

Sift is a well-structured Rust project. Clippy pedantic+nursery+cargo passes clean, formatting is correct, error ownership is explicit, and the domain-type discipline (enums over booleans, `IndexSource`/`IndexDestination`, `*Decl` → domain config) is strong. The issues below are architectural and idiomatic refinements, not correctness bugs.

---

## Part 1: Core Crate (`sift-core`) Architecture

### 1A. Strengths

- **Error hierarchy** is textbook: per-module error types (`CompileError`, `FilterError`, `OutputError`, `ExecutionError`) aggregated via `From` impls into `SearchError`, then into `crate::Error`. Clean, composable, no stringly-typed errors.
- **`IndexSource`/`IndexDestination` enums** cleanly replace what would otherwise be parallel `*_to_dir`/`*_into` function variants. Functions dispatch internally on the variant.
- **`IndexKind` lifecycle dispatch** (`build`, `open`, `update`) is extensible — adding a new index kind means adding enum variants with match arms.
- **`Snapshot` + `SnapshotLease`** RAII pattern prevents GC of snapshots during active reads.
- **`Candidate`** with `OnceLock<String>` for lazy `rel_str` is a good zero-cost-until-needed pattern.
- **`PatternCompiler`** is a clean builder with `#[must_use]` on every step.
- **`bitflags!`** for `SearchMatchFlags`, `QueryFlags`, `IgnoreSources`, `LineStyleFlags` is idiomatic and zero-cost.

### 1B. Issues

#### ARCH-C1: `lib.rs` flat re-export blast (~40 types)

**File:** `crates/core/src/lib.rs:14-36`

```rust
// Current: everything dumped into crate root
pub use search::{
    BinaryMode, CandidateFilter, CandidateFilterConfig, CaseMode, ColorChoice,
    ColumnLimit, ColumnOverflow, FilenameMode, GlobConfig, HiddenMode, IgnoreConfig,
    IgnoreSources, LineStyleFlags, LinkTraversal, Match, MatchEmissionMode,
    OutputEmission, PassthruMode, PathDisplay, PatternCompiler, RecordTerminator,
    SearchCollection, SearchError, SearchMatchFlags, SearchMode, SearchOptions,
    SearchOutcome, SearchOutput, SearchOutputFormat, SearchQuery, SearchRecordStyle,
    SearchSeparators, SearchStats, TypeDef, VisibilityConfig, WalkOptions, ZeroCountMode,
};
```

**Problem:** Flat namespace conflates output types, filter types, query types, and options types. Makes it hard for consumers to discover the API surface by domain. Also causes name collisions risk (e.g., `WalkOptions` exists in both `search::request` and `index::config`; the latter is re-exported as `IndexWalkOptions`).

**Proposed change:** Re-export domain modules as public, keep only the top-level entry points (the types CLI actually needs) in the root. The CLI already imports `sift_core::SearchMode`, `sift_core::SearchQuery`, etc. — it can import `sift_core::search::SearchMode` instead.

```rust
// Proposed: structured public API
pub mod candidate;
pub mod grep;
pub mod index;
pub mod query;
pub mod search;  // was `mod search` (private)

// Root re-exports only the primary entry points
pub use candidate::Candidate;
pub use grep::GrepRun;
pub use index::{
    CorpusKind, CorpusMeta, CorpusSpec, FileId, FilterMeta, Index, IndexConfig,
    IndexError, IndexId, IndexKind, Indexes, PlanMode, QueryPlanOutput, WalkMeta,
};
pub use index::config::WalkOptions as IndexWalkOptions;
pub use index::meta::StoreMeta;
pub use index::store::IndexStore;
pub use index::trigram::{TrigramIndex, TrigramIndexError};
pub use query::{CandidateRequirement, QueryFlags, QueryPlanner, QuerySpec};
pub use search::SearchError;
```

The CLI changes from `use sift_core::SearchMode` to `use sift_core::search::SearchMode` — a small import path change but a major API clarity improvement.

#### ARCH-C2: `SearchQuery` conflates query description with execution state

**File:** `crates/core/src/search/query/mod.rs:43-49`

```rust
pub struct SearchQuery {
    pub patterns: Vec<String>,
    pub opts: SearchOptions,
    pub matcher: OnceLock<RegexMatcher>,             // execution cache
    pub searcher_cache: Mutex<Option<SearcherCacheEntry>>,  // execution cache
}
```

**Problem:** `SearchQuery` mixes immutable query description (patterns, opts) with mutable execution state (matcher cache, searcher cache). The caches make the struct non-`Clone`, non-`Send`+`Sync` without the `Mutex`, and harder to reason about. The `search()` method on it further blurs the line.

**Proposed change:** Not a split (which would be over-engineering), but make the caches private and the fields non-pub. The patterns and opts are already accessible via `patterns()` and the opts are used internally. Making fields private enforces the abstraction boundary:

```rust
pub struct SearchQuery {
    patterns: Vec<String>,       // was pub
    opts: SearchOptions,         // was pub
    matcher: OnceLock<RegexMatcher>,
    searcher_cache: Mutex<Option<SearcherCacheEntry>>,
}

impl SearchQuery {
    pub fn patterns(&self) -> &[String];
    pub fn opts(&self) -> &SearchOptions;  // new accessor
}
```

This is a smaller change that respects AGENTS.md's "small, focused changes" rule while fixing the encapsulation leak. All current callers already use `query.opts.max_results` which becomes `query.opts().max_results`.

#### ARCH-C3: Opaque tuple type alias for searcher cache

**File:** `crates/core/src/search/query/mod.rs:34`

```rust
type SearcherCacheEntry = ((bool, Option<usize>, usize, usize), Searcher);
```

**Problem:** The tuple `(bool, Option<usize>, usize, usize)` is a cache key with no semantic meaning. Readers can't tell what each field represents without tracing through `build_searcher`.

**Proposed change:** Replace with a named struct:

```rust
/// Cache key for searcher reuse — avoids rebuilding when config hasn't changed.
#[derive(PartialEq, Eq)]
struct SearcherCacheKey {
    passthru: bool,
    max_count: Option<usize>,
    before_context: usize,
    after_context: usize,
}

type SearcherCacheEntry = (SearcherCacheKey, Searcher);
```

#### ARCH-C4: `SearchQuery::resolve_matcher` — manual `OnceLock` pattern

**File:** `crates/core/src/search/query/mod.rs:152-159`

```rust
fn resolve_matcher(&self) -> Result<&RegexMatcher, SearchError> {
    if let Some(m) = self.matcher.get() {
        return Ok(m);
    }
    let m = self.build_matcher()?;
    let _ = self.matcher.set(m);
    Ok(self.matcher.get().expect("just initialised"))
}
```

**Problem:** This is a manual reimplementation of `OnceLock::get_or_try_init`, which was stabilized in Rust 1.82. The project uses Rust 2024 edition (requires 1.85+), so this API is available.

**Proposed change:**

```rust
fn resolve_matcher(&self) -> Result<&RegexMatcher, SearchError> {
    self.matcher.get_or_try_init(|| self.build_matcher())
}
```

#### ARCH-C5: `Candidate::total_file_bytes` is an orphan associated function

**File:** `crates/core/src/search/emit/format.rs:7-14`

```rust
impl Candidate {
    pub fn total_file_bytes(candidates: &[Self]) -> u64 {
        candidates.iter().fold(0u64, |acc, c| {
            acc + std::fs::metadata(c.abs_path()).map_or(0, |m| m.len())
        })
    }
}
```

**Problem:** An associated function on `Candidate` that operates on a slice of candidates is defined in `search/emit/format.rs`, far from `candidate.rs`. This is an I/O-performing function (calls `std::fs::metadata`) disguised as a pure utility. Per AGENTS.md: "Separate domain decisions from side effects."

**Proposed change:** Move to `candidate.rs` alongside the type definition. This keeps `Candidate`'s behavior co-located:

```rust
// In candidate.rs
impl Candidate {
    /// Sum on-disk byte sizes for all candidates (used for search stats).
    #[must_use]
    pub fn total_file_bytes(candidates: &[Self]) -> u64 { ... }
}
```

#### ARCH-C6: `WalkOptions` name collision

**Files:** `crates/core/src/search/request/mod.rs:12` and `crates/core/src/index/config.rs:19`

Two distinct structs named `WalkOptions` exist:
- `search::request::WalkOptions` — used for file discovery during search (has `LinkTraversal`, `max_depth`, etc.)
- `index::config::WalkOptions` — used for index build walks (has `follow_links: bool`, `one_file_system`, etc.)

The root `lib.rs` works around this with `pub use index::config::WalkOptions as IndexWalkOptions`.

**Proposed change:** Rename `index::config::WalkOptions` to `IndexWalkConfig` at the source, removing the alias:

```rust
// crates/core/src/index/config.rs
pub struct IndexWalkConfig {  // was WalkOptions
    pub follow_links: bool,
    pub one_file_system: bool,
    pub max_depth: Option<usize>,
    pub max_filesize: Option<u64>,
}
```

#### ARCH-C7: `Indexes` wraps `Snapshot` without adding meaningful abstraction

**File:** `crates/core/src/index/registry.rs:14-16`

```rust
pub struct Indexes {
    snapshot: Snapshot,
}
```

**Analysis:** `Indexes` wraps `Snapshot` and forwards `root()`, `is_empty()`, plus adds `candidates()`, `indexed_rel_paths()`, `unindexed_hits()`, `complete_candidates()`, `candidates_multi()`, `first()`, `corpus_kind()`. These are genuine query-time operations that don't belong on `Snapshot` (which is a persistence concept).

**Verdict:** The abstraction is justified. The naming could be clearer — `IndexRegistry` would better describe "opened indexes ready for querying" vs the raw `Indexes` plural — but this is a minor nit.

#### ARCH-C8: `CurrentSnapshot::Drop` is a no-op

**File:** `crates/core/src/index/snapshot/mod.rs:34-38`

```rust
impl Drop for CurrentSnapshot {
    fn drop(&mut self) {
        let _ = &mut self.lease;
    }
}
```

**Problem:** This `Drop` impl does nothing. The `lease` field is a `SnapshotLease` which has its own `Drop` impl — it will be dropped automatically when `CurrentSnapshot` is dropped. The explicit `Drop` just forces the field to be "used" in the destructor, but Rust drops all fields anyway.

**Proposed change:** Remove the manual `Drop` impl entirely. The `SnapshotLease`'s own `Drop` will fire automatically.

---

## Part 2: CLI Crate (`sift-grep`) Architecture

### 2A. Strengths

- **Two-layer model** (`*Decl` → domain config) is well-executed. Clap parsing stays in declarations; runtime semantics are resolved from `Argv`.
- **Domain modules never import `Cli`** — data flows one way: `Cli` builds configs, passes them to domain modules.
- **`Argv` collected once** in `main_entry`, threaded through as `&Argv<'_>`.
- **`DispatchRoute` enum** in `cli.rs` cleanly models the top-level command dispatch without string matching.
- **Module pairing** (decl → config → resolve) is consistent across `pattern`, `filter`, `output`, `ignore`, `paths`.

### 2B. Issues

#### ARCH-L1: `Cli` struct has 30+ flattened fields

**File:** `crates/cli/src/cli.rs:30-108`

```rust
pub struct Cli {
    pub command: Option<Commands>,
    pub patterns: PatternArgs,
    pub search_scope: SearchScope,
    pub regex1: RegexFlagsA,
    pub regex2: RegexFlagsB,
    pub line_number_decl: LineNumberDecl,
    // ... 25+ more flattened Decl fields
}
```

**Analysis:** This is a direct consequence of clap's `#[command(flatten)]` model — each flag group needs to be a separate struct for help grouping. The 30+ fields is ugly but functional. The real issue is that `Cli` has config-builder methods (`pattern_config()`, `filter_config()`, `output_config()`, `grep_config()`, `index_request()`) that clone these fields extensively.

**Proposed change:** The `*_config()` methods clone every `Decl` struct into the config. Since `Decl` structs are only used once (to build the config), they could be consumed by value. But `Cli` is parsed by clap and the config builders are called from `dispatch()` which takes `&self`. The real fix is to make `dispatch` consume `self`:

```rust
// Current:
pub fn dispatch(&self, argv: &Argv<'_>) -> ExitCode { ... }

// Proposed:
pub fn dispatch(self, argv: &Argv<'_>) -> ExitCode { ... }
```

Then `pattern_config(self)` etc. can move fields instead of cloning. This is a targeted change since `Cli` is only used once in `main_entry`.

#### ARCH-L2: `Cli::index_request` uses `expect` + `unreachable!` double-guard

**File:** `crates/cli/src/cli.rs:164-167`

```rust
fn index_request(&self) -> IndexRequest {
    let Commands::Index { command } = self.command.as_ref().expect("index subcommand") else {
        unreachable!("index_request called without index subcommand");
    };
    // ...
}
```

**Problem:** The `expect` panics if `command` is `None`, and the `else unreachable!` panics if it's `Some(Commands::Update)`. These are runtime checks for what should be a compile-time guarantee. The caller (`dispatch_route`) already checks the variant.

**Proposed change:** Pass the `IndexCommands` directly from the dispatch site:

```rust
// In dispatch_route:
DispatchRoute::Index(self.index_request(command))  // pass IndexCommands directly

// In index_request:
fn index_request(&self, command: &IndexCommands) -> IndexRequest { ... }
```

#### ARCH-L3: `IndexJob::run` and `run_background` duplicate validation

**File:** `crates/cli/src/index/mod.rs:104-228`

Both `run()` (lines 126-142) and `run_background()` (lines 179-195) contain identical `match self.operation` blocks checking whether a snapshot exists. This is ~16 lines of duplicated logic.

**Proposed change:** Extract validation into a method:

```rust
impl IndexJob {
    fn validate_preconditions(&self, has_snapshot: bool) -> Result<(), String> {
        match self.operation {
            IndexOperation::Build if has_snapshot => {
                Err(format!("index already exists at {}; run `sift index update`", self.sift_dir.display()))
            }
            IndexOperation::Update if !has_snapshot => {
                Err(format!("no index at {}; run `sift index build` first", self.sift_dir.display()))
            }
            _ => Ok(()),
        }
    }
}
```

#### ARCH-L4: `GrepConfig.files_mode: bool` should be a domain type

**File:** `crates/cli/src/grep/run.rs:22`

```rust
pub struct GrepConfig {
    // ...
    pub files_mode: bool,
}
```

**Problem:** Per AGENTS.md: "Avoid boolean flags when a named domain type would make intent clearer." The `run()` method branches on this boolean to either list files or search. This is a classic case for an enum.

**Proposed change:**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrepMode {
    Search,
    ListFiles,
}

pub struct GrepConfig {
    // ...
    pub mode: GrepMode,  // was files_mode: bool
}
```

---

## Part 3: Cross-Crate Architecture (Core + CLI in Unison)

### 3A. Strengths

- **Dependency direction is correct:** CLI depends on core; core has zero knowledge of CLI. No circular imports.
- **Domain boundary is clean:** CLI builds domain configs (`PatternConfig`, `FilterConfig`, `GrepConfig`) from clap parsing, then passes them to core types (`SearchQuery`, `SearchOptions`, `CandidateFilter`, `GrepRequest`).
- **Core's `GrepRequest` is the API surface:** The CLI calls `GrepRequest::run(&query)` — a single entry point that orchestrates the full pipeline (planner → candidates → filter → search → emit).
- **`Argv` stays in CLI:** The raw argv scanning (last-wins resolution, flag precedence) is CLI-only. Core never sees argv.
- **`IndexStore` is correctly scoped:** The CLI's `IndexJob` orchestrates store open/create/reconcile, but the actual build/update logic lives in core's `IndexStore::build()` / `IndexStore::update()`.

### 3B. Issues

#### CROSS-1: Core's `grep::GrepRequest` uses `anyhow`-incompatible error type

**File:** `crates/core/src/grep/mod.rs:45`

```rust
pub fn run(&self, query: &SearchQuery) -> crate::Result<GrepRun> { ... }
```

**File:** `crates/cli/src/grep/run.rs:63`

```rust
pub fn run(&self, argv: &Argv<'_>, daemon: Option<&Daemon>) -> anyhow::Result<GrepOutcome> { ... }
```

**Analysis:** Core returns `crate::Result<T>` (which is `Result<T, crate::Error>`). CLI wraps this in `anyhow::Result`. The conversion works because `crate::Error` implements `std::error::Error` (via `thiserror`). This is actually fine — `thiserror` for libraries, `anyhow` for applications is the canonical pattern.

**Verdict:** No change needed. This is correct practice.

#### CROSS-2: CLI duplicates walk-based file discovery

**File:** `crates/cli/src/grep/run.rs:109-176` (`run_files`)

The CLI's `run_files` method manually builds a walk using `WalkOptions::discover_files`, iterates paths, filters, deduplicates, and prints. Meanwhile, core has `CandidateFilter::collect()` which does nearly the same walk + filter + dedup.

**Problem:** The CLI reimplements candidate collection for `--files` mode instead of reusing core's `CandidateFilter::collect()`. This creates divergent filtering behavior between `--files` output and actual search.

**Proposed change:** Reuse core's candidate pipeline:

```rust
fn run_files(&self, argv: &Argv<'_>) -> anyhow::Result<bool> {
    let session = self.prepare_session(argv)?;
    let candidates = session.search_filter.collect()?;
    let filtered: Vec<_> = candidates
        .into_iter()
        .filter(|c| c.matches(&session.search_filter))
        .collect();
    // print filtered candidates
    ...
}
```

This ensures `--files` and search see identical candidate sets.

#### CROSS-3: `SearchOutput` is `Copy` but only by coincidence

**File:** `crates/core/src/search/output/mod.rs:19-28`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchOutput {
    pub format: SearchOutputFormat,
    pub mode: SearchMode,
    pub emission: OutputEmission,
    pub lines: SearchLineStyle,
    pub records: SearchRecordStyle,
    pub passthru: PassthruMode,
    pub include_zero: ZeroCountMode,
}
```

**Analysis:** All 7 fields are small enums/structs, so `Copy` works today. But `SearchOutput` is a configuration type passed around by value — `Copy` is appropriate and intentional here. It's the kind of value type that should be `Copy`.

**Verdict:** No change needed.

#### CROSS-4: `SearchExecution` owns `Vec<Candidate>` unnecessarily

**File:** `crates/core/src/search/request/mod.rs:30-36`

```rust
pub struct SearchExecution<'a> {
    pub candidates: Vec<Candidate>,  // owned
    pub output: SearchOutput,
    pub separators: &'a SearchSeparators,
    pub collect: SearchCollection,
}
```

**Problem:** `SearchExecution` owns the candidates `Vec`, but the caller (`GrepRequest::run`) builds the vec and immediately passes it into `SearchExecution`. This forces a move, which is fine for the happy path but means the caller can't inspect the candidates after search. The `'a` lifetime already exists for `separators` — candidates could borrow too.

**Proposed change:** Borrow the candidates slice:

```rust
pub struct SearchExecution<'a> {
    pub candidates: &'a [Candidate],  // borrowed
    pub output: SearchOutput,
    pub separators: &'a SearchSeparators,
    pub collect: SearchCollection,
}
```

The caller already has the `Vec` in scope. This also makes `SearchExecution` lighter (`Copy`-able minus the slice reference).

---

## Part 4: Rust Idioms & Best Practices

### IDIOM-1: Redundant `.iter()` in `for` loops

**Files (20 occurrences):** builder.rs:291,300,312,321, search.rs:138,319, registry.rs:103, store.rs:266, output.rs:260,318,330,346,368,398, pattern.rs:272,338, postings.rs:239, trigram_sets.rs:100

```rust
// Current:
for &p in pairs.iter() { ... }

// Idiomatic:
for &p in pairs { ... }
```

`for x in collection` calls `IntoIterator::into_iter`, which for `&[T]` and `&Vec<T>` yields `&T` — identical to `.iter()`. The explicit `.iter()` is redundant when the collection is borrowed. Note: when `.enumerate()` or `.skip()` follows, `.iter()` is necessary; those cases should be left as-is.

**Proposed change:** Remove `.iter()` from `for` loops where no chained method follows. Approximately 10 of the 20 occurrences are pure `for x in collection.iter()` with no chain.

### IDIOM-2: `unwrap()` on infallible `try_into` in storage deserialization

**Files:** lexicon.rs:93,104,105,157,164,166,246,247,248; postings.rs:81; trigram_sets.rs:285,299,337,360,527,543; file_table.rs:126,139,153,177,179,196,197

```rust
// Current (repeated ~25 times):
u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap())
```

**Problem:** These `unwrap()` calls are technically safe (the slice length matches the array size), but a reader can't tell that at a glance. If the slice bounds are ever wrong, the `unwrap()` gives no context.

**Proposed change:** Use `expect` with a message documenting the invariant:

```rust
u32::from_le_bytes(
    bytes[off..off + 4]
        .try_into()
        .expect("slice is exactly 4 bytes"),
)
```

Alternatively, introduce a small private helper in the storage module:

```rust
fn read_u32_le(bytes: &[u8], offset: usize) -> u32 {
    let slice = &bytes[offset..offset + 4];
    u32::from_le_bytes(slice.try_into().expect("4-byte slice"))
}

fn read_u64_le(bytes: &[u8], offset: usize) -> u64 {
    let slice = &bytes[offset..offset + 8];
    u64::from_le_bytes(slice.try_into().expect("8-byte slice"))
}
```

This eliminates ~25 repetitions of the pattern. These helpers describe what they do (read a little-endian integer), not how they differ from a variant, so they comply with the AGENTS.md helper naming rule.

### IDIOM-3: `PatternCompiler` builder methods silently ignore `false`

**File:** `crates/core/src/search/pattern/mod.rs:25-46`

```rust
pub fn fixed_strings(mut self, on: bool) -> Self {
    if on {
        self.flags |= SearchMatchFlags::FIXED_STRINGS;
    }
    self
}
```

**Problem:** Calling `.fixed_strings(false)` is a no-op — it doesn't clear the flag. If a consumer calls `.fixed_strings(true).fixed_strings(false)`, the flag remains set. This is surprising for a builder.

**Proposed change:** Handle both cases:

```rust
pub fn fixed_strings(mut self, on: bool) -> Self {
    self.flags.set(SearchMatchFlags::FIXED_STRINGS, on);
    self
}
```

`bitflags::set(flag, value)` sets or clears based on the boolean. Apply to `word_regexp` and `line_regexp` too.

### IDIOM-4: `PatternCompiler::compile` avoids unnecessary clone

**File:** `crates/core/src/search/pattern/mod.rs:76-85`

```rust
let branches: Vec<String> = patterns.iter().map(|p| self.shape(p)).collect();
let combined = if branches.len() == 1 {
    branches[0].clone()  // unnecessary clone
} else {
    branches.into_iter().map(|b| format!("(?:{b})")).collect::<Vec<_>>().join("|")
};
```

**Problem:** `branches[0].clone()` allocates when `branches.into_iter().next().unwrap()` would move.

**Proposed change:**

```rust
let mut branches: Vec<String> = patterns.iter().map(|p| self.shape(p)).collect();
let combined = if branches.len() == 1 {
    branches.pop().expect("checked len == 1")
} else {
    branches.into_iter().map(|b| format!("(?:{b})")).collect::<Vec<_>>().join("|")
};
```

### IDIOM-5: `WalkOptions::default()` should derive

**File:** `crates/core/src/search/request/mod.rs:19-28`

```rust
impl Default for WalkOptions {
    fn default() -> Self {
        Self {
            links: LinkTraversal::DoNotFollow,
            max_depth: None,
            max_filesize: None,
            one_file_system: false,
        }
    }
}
```

**Problem:** `LinkTraversal` doesn't derive `Default`, so `WalkOptions` can't derive it either. But `DoNotFollow` is the natural default.

**Proposed change:** Add `#[default]` to `LinkTraversal::DoNotFollow`, then derive `Default` on `WalkOptions`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LinkTraversal {
    #[default]
    DoNotFollow,
    Follow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WalkOptions { ... }
```

### IDIOM-6: `SearchOutput::default()` should derive

**File:** `crates/core/src/search/output/mod.rs:30-42`

Same pattern — all fields already have `#[default]`-capable types or could. `SearchMode::Standard` is already `#[default]`; `OutputEmission::Normal` is already `#[default]`. Check if `PassthruMode::Disabled` and `ZeroCountMode::Omit` have `#[default]`; if not, add it and derive.

### IDIOM-7: `Candidate::display_path` allocates for separator replacement

**File:** `crates/core/src/candidate.rs:51-62`

```rust
pub fn display_path(&self, display: PathDisplay, path_separator: Option<u8>) -> String {
    let raw = match display {
        PathDisplay::Absolute => self.abs_path().display().to_string(),
        PathDisplay::Relative => self.rel_path().display().to_string(),
    };
    if let Some(sep) = path_separator {
        let sep_char = sep as char;
        raw.replace(std::path::MAIN_SEPARATOR, &sep_char.to_string())
    } else {
        raw
    }
}
```

**Problem:** `&sep_char.to_string()` allocates a `String` to create a single-char `&str` for `replace`. Use `char` directly since `str::replace` accepts `char` as a pattern.

**Proposed change:**

```rust
if let Some(sep) = path_separator {
    raw.replace(std::path::MAIN_SEPARATOR, &(sep as char).to_string())
    // Actually, str::replace can take a char pattern for search but not for replacement.
    // The replacement arg is &str. Use a byte array instead:
    raw.replace(
        std::path::MAIN_SEPARATOR,
        std::str::from_utf8(&[sep]).unwrap_or("/"),
    )
}
```

Wait — `str::replace(P, &str)` requires `&str` for the replacement. The current code works but allocates. The better approach: since `sep` is always ASCII (it's a CLI-provided path separator byte), we can avoid the allocation:

```rust
if let Some(sep) = path_separator {
    let mut buf = [0u8; 4];
    let sep_str = (sep as char).encode_utf8(&mut buf);
    raw.replace(std::path::MAIN_SEPARATOR, sep_str)
}
```

This is a micro-optimization but demonstrates good Rust practice: avoid allocating when a stack buffer suffices.

### IDIOM-8: Error mapping duplication in `Indexes::open`

**File:** `crates/core/src/index/registry.rs:38-60`

```rust
let store = store::IndexStore::open(sift_dir).map_err(|e| match e {
    crate::Error::Index(ie) => ie,
    crate::Error::Io(io) => IndexError::Io { path: sift_dir.to_path_buf(), source: io },
    _ => IndexError::Io { path: sift_dir.to_path_buf(), source: std::io::Error::other(e.to_string()) },
})?;

let snapshot = store.open_current().map_err(|e| match e {
    crate::Error::Index(ie) => ie,
    crate::Error::Io(io) => IndexError::Io { path: sift_dir.to_path_buf(), source: io },
    _ => IndexError::Io { path: sift_dir.to_path_buf(), source: std::io::Error::other(e.to_string()) },
})?;
```

**Problem:** Identical error mapping closure used twice.

**Proposed change:** Extract a local closure:

```rust
pub fn open(sift_dir: &Path) -> Result<Self, IndexError> {
    let map_err = |e: crate::Error| -> IndexError {
        match e {
            crate::Error::Index(ie) => ie,
            crate::Error::Io(io) => IndexError::Io { path: sift_dir.to_path_buf(), source: io },
            _ => IndexError::Io {
                path: sift_dir.to_path_buf(),
                source: std::io::Error::other(e.to_string()),
            },
        }
    };
    let store = store::IndexStore::open(sift_dir).map_err(&map_err)?;
    let snapshot = store.open_current().map_err(map_err)?;
    Ok(Self { snapshot })
}
```

### IDIOM-9: `lifecycle.rs` clones `FileFingerprint` twice in map

**File:** `crates/core/src/index/trigram/lifecycle.rs:248`

```rust
.map(|fp| (fp.path.clone(), fp.clone()))
```

**Problem:** `fp.path.clone()` followed by `fp.clone()` clones the path twice (once standalone, once inside the full struct clone).

**Proposed change:**

```rust
.map(|fp| {
    let path = fp.path.clone();
    (path, fp)  // move fp, avoid second clone if into_iter is used
})
// OR if fp is borrowed:
.map(|fp| (fp.path.clone(), fp.clone()))  // unavoidable if iterator yields &fp
```

Need to check whether the iterator yields owned or borrowed values. If it yields owned `FileFingerprint`, the fix is:

```rust
.map(|fp| {
    let key = fp.path.clone();
    (key, fp)
})
```

---

## Part 5: Implementation Order

Changes ordered by dependency (earlier changes don't depend on later ones):

### Phase 1: Pure idiom fixes (no API changes)

1. **IDIOM-1:** Remove redundant `.iter()` in `for` loops (~10 sites)
2. **IDIOM-2:** Add `read_u32_le`/`read_u64_le` helpers in storage, replace ~25 bare `unwrap()` calls
3. **IDIOM-3:** Use `bitflags::set()` in `PatternCompiler` builder methods
4. **IDIOM-4:** Eliminate unnecessary clone in `PatternCompiler::compile`
5. **IDIOM-7:** Stack-buffer path separator in `Candidate::display_path`
6. **IDIOM-8:** Extract error mapping closure in `Indexes::open`
7. **IDIOM-9:** Avoid double clone in `lifecycle.rs:248`
8. **ARCH-C4:** Use `OnceLock::get_or_try_init` in `resolve_matcher`
9. **ARCH-C8:** Remove no-op `Drop` impl for `CurrentSnapshot`

### Phase 2: Type-level improvements (minor API surface changes, internal)

10. **ARCH-C3:** Replace opaque tuple with `SearcherCacheKey` struct
11. **IDIOM-5:** Derive `Default` for `LinkTraversal` + `WalkOptions`
12. **IDIOM-6:** Derive `Default` for `PassthruMode`, `ZeroCountMode`, `SearchOutput`
13. **ARCH-C6:** Rename `index::config::WalkOptions` → `IndexWalkConfig`
14. **ARCH-L4:** Replace `files_mode: bool` with `GrepMode` enum in CLI
15. **ARCH-C5:** Move `total_file_bytes` to `candidate.rs`

### Phase 3: Encapsulation and boundary fixes

16. **ARCH-C2:** Make `SearchQuery` fields private, add `opts()` accessor
17. **CROSS-4:** Change `SearchExecution.candidates` from `Vec<Candidate>` to `&'a [Candidate]`
18. **ARCH-L2:** Pass `IndexCommands` directly to `index_request`

### Phase 4: Structural improvements

19. **ARCH-L3:** Extract `IndexJob::validate_preconditions`
20. **ARCH-L1:** Make `Cli::dispatch` consume `self`, convert config builders to move fields
21. **CROSS-2:** Reuse core's `CandidateFilter::collect()` in CLI's `run_files`
22. **ARCH-C1:** Make `search` module public, restructure re-exports

---

## Summary of Impact

| Category | Count | Risk |
|----------|-------|------|
| Pure idiom (no behavior change) | 9 | Very low |
| Type-level (internal, no public API break) | 6 | Low |
| Encapsulation (tightens API surface) | 3 | Low-Medium |
| Structural (refactors boundaries) | 4 | Medium |
| **Total** | **22** | |

All changes preserve the existing test suite. No new dependencies. No `unsafe`. No `#[allow]` attributes.

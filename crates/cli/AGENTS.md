# AGENTS.md — sift-cli

## Responsibility

Thin CLI binary over `sift-core`. Parses flags with clap, maps them to `SearchOptions`/`SearchMatchFlags`, and dispatches to core.

## Structure

- **`src/lib.rs`** — `Cli` (clap Parser), `build` subcommand, search mode dispatch.
- **`src/main.rs`** — thin binary entrypoint calling `sift_cli::main()`.
- **`tests/common/mod.rs`** — shared test helpers: `TestProject`, assertion helpers, path normalization.
- **`tests/integration_*.rs`** — domain-focused integration tests spawning the real `sift` binary.

## Test Helpers (`tests/common/mod.rs`)

### TestProject
```rust
let p = TestProject::new("my-test");
p.write("a.txt", "content\n");
p.build_index();                   // index "." from project root
let out = p.index_output(["pattern"]);  // search with index
let out = p.walk_output(["pattern"]);   // search without index
p.assert_index_walk_same(["pattern"], "expected\n");
```

### Assertions
- `assert_success(out)` — exit 0 with rich failure message
- `assert_exit_code(out, n)` — specific exit code
- `assert_stdout_eq(out, expected)` — exact stdout match
- `assert_stdout_contains(out, substr)` / `assert_stdout_not_contains`
- `assert_stderr_empty(out)`
- `normalize_stdout(out)` / `normalize_stderr(out)` — cross-platform path/line-end normalization

### Path Helpers
- `rel_match(rel, rest)` — `"file.txt:content"`
- `abs_path(root, rel)` — canonicalized absolute path

## Behavior Notes

- Global options (e.g. `--index`) must appear **before** `build` when indexing.
- Search paths are resolved and must sit under the corpus root in the index metadata.
- Extend flags by threading new `SearchMatchFlags`/`SearchOptions` fields through to `SearchQuery::new` in core — do not duplicate regex logic here.

## Testing

```bash
cargo test -p sift-cli
cargo build --release -p sift-cli
```

## Do NOT

- Duplicate regex or search logic from `sift-core`.
- Add heavy dependencies — this crate should stay thin.
- Change flag semantics without updating integration tests.
- Use `#[allow(…)]` or `#[expect(…)]` — fix the root cause instead. If a helper is only used on certain platforms, gate its import with `#[cfg(…)]`.

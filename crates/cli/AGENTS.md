# AGENTS.md — sift-cli

## Responsibility

Thin CLI binary over `sift-core`. Parses flags with clap, maps them to `SearchOptions`/`SearchMatchFlags`, and dispatches to core.

## Structure

- **`src/main.rs`** — `Cli` (clap Parser), `build` subcommand, search mode dispatch.
- **`tests/integration_*.rs`** — domain-focused integration tests spawning the real `sift` binary.

## Behavior Notes

- Global options (e.g. `--index`) must appear **before** `build` when indexing.
- Search paths are resolved and must sit under the corpus root in the index metadata.
- Extend flags by threading new `SearchMatchFlags`/`SearchOptions` fields through to `CompiledSearch::new` in core — do not duplicate regex logic here.

## Testing

```bash
cargo test -p sift-cli
cargo build --release -p sift-cli
```

## Do NOT

- Duplicate regex or search logic from `sift-core`.
- Add heavy dependencies — this crate should stay thin.
- Change flag semantics without updating integration tests.

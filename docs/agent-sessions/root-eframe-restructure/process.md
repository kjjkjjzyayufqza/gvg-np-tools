# Root Eframe Restructure Process

Hub links:

- Cross-agent hub: `../../../AGENTS.md`
- Cursor rule: `../../../.cursor/rules/gvg-research-tools.mdc`
- TODO: `todo.md`

## User Request

Restructure the project so the Rust app now lives at the repository root instead
of the old nested Rust crate directory. The desired shape should be closer to
`emilk/eframe_template`, with Cargo and source files at the repository root.
Update the AI guidance created in the previous session so it no longer tells
agents to use the old nested crate path, and create a root `README.md`
explaining how to build, run, and debug the project.

Reference provided by the user:

- `https://github.com/emilk/eframe_template/`

## Startup Notes

- Followed `AGENTS.md` startup protocol for this task.
- This repository currently has a dirty worktree with many unrelated deleted and
  untracked research assets; do not revert unrelated changes.
- Existing `.cursor/rules/cursor-rules.mdc` was already deleted before this
  task and should not be restored unless the user asks.

## Context To Fill During Work

- Current Cargo/root structure:
  - `Cargo.toml` and `Cargo.lock` are now at the repository root.
  - `src/` is now at the repository root and contains both the shared library
    and binaries.
  - `src/main.rs` remains the CLI binary source for `gvg_converter`.
  - `src/bin/gvg_modding_tool.rs` remains the native egui/eframe GUI binary.
  - `tests/` is now at the repository root.
- Files moved:
  - Moved Cargo manifest/lockfile, `src/`, and `tests/` from the old nested
    crate directory to the repository root with `git mv`.
  - Removed old generated `target*` directories from the nested crate location
    instead of moving build artifacts.
  - Set `default-run = "gvg_modding_tool"` so root `cargo run` behaves like an
    eframe-style GUI app.
- References updated:
  - Updated `AGENTS.md` to point agents at `src/`, root `target/`, and root-level
    Cargo commands.
  - Updated `docs/MOD_WORKFLOW.md` and `docs/RUST_DAE_PMF2_COMMANDS.md` to use
    `cargo run --bin gvg_converter -- ...` for CLI workflows.
  - Updated stale source path references in `.cursor/plans/`,
    `docs/RX78_RESOURCE_MAPPING_ANALYSIS.md`,
    `docs/stream000_pmf2_byte_analysis.md`, and root `process.md`.
  - Added root `README.md` with build, run, test, and debug instructions.
- Verification:
  - `cargo fmt --check` initially failed on existing Rust formatting after the
    root move exposed the crate at the repository root.
  - Ran `cargo fmt`.
  - Re-ran `cargo fmt --check`: exit 0.
  - Ran `cargo test`: exit 0. Results: 59 library tests passed, 17 integration
    tests passed, 0 failures. The build emitted four existing dead-code warnings
    in `src/pmf2.rs`.
  - Ran `cargo run --bin gvg_converter -- --help`: exit 0 and printed CLI help.
  - `ReadLints` reported no linter errors for the edited docs and Rust paths.

## Rust 2024 Edition Update

- Confirmed from the Rust Edition Guide and Rust 1.85 announcement that Rust
  2024 is the latest stable edition. No newer stable edition is available.
- Checked local toolchain:
  - `rustc 1.90.0 (1159e78c4 2025-09-14)`
  - `cargo 1.90.0 (840b83a10 2025-07-30)`
- Updated `Cargo.toml` from `edition = "2021"` to `edition = "2024"`.
- Updated `README.md` requirements to state Rust 1.85+ because Rust 2024 was
  stabilized in Rust 1.85.
- `cargo fmt --check` initially failed after the edition change because Rust
  2024 style formatting changed import ordering and several line breaks.
- Ran `cargo fmt`.
- Re-ran `cargo fmt --check`: exit 0.
- Ran `cargo test`: exit 0. Results: 59 library tests passed, 17 integration
  tests passed, 0 failures. The same four dead-code warnings in `src/pmf2.rs`
  remain.
- Ran `cargo run --bin gvg_converter -- --help`: exit 0 and printed CLI help.

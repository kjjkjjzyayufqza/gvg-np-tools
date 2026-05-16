# Process

## Context

- UI: `src/gui/inspector.rs` (`show_gim_summary`), state on `GvgModdingApp` in `src/gui.rs`.

## Verification

- `cargo check` — OK (2026-05-16).
- `cargo test` — failed: could not overwrite `target/debug/gvg_modding_tool.exe` (os error 5, likely EXE in use); retry after closing the running app.

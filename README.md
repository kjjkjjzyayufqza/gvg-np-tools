# GVG Next Plus Research Tools

Rust research and modding tools for **Mobile Suit Gundam: Gundam vs. Gundam
Next Plus** (`ガンダムVS.ガンダムNEXT PLUS`).

The project is now a root-level Cargo application, similar in layout to
`emilk/eframe_template`: `Cargo.toml`, `Cargo.lock`, `src/`, and `tests/` live at
the repository root.

## Project Layout

- `Cargo.toml` / `Cargo.lock`: root Cargo package.
- `src/main.rs`: CLI entry point, built as `gvg_converter`.
- `src/bin/gvg_modding_tool.rs`: native egui/eframe GUI entry point.
- `src/lib.rs`: shared library modules used by both CLI and GUI.
- `src/gui.rs` and `src/gui/`: GUI shell, panels, inspectors, and editor windows.
- `src/afs.rs`, `src/pzz.rs`, `src/pmf2.rs`, `src/dae.rs`, `src/texture.rs`:
  format parsers, converters, and rebuilders.
- `src/shaders/`: WGPU shader sources.
- `tests/`: integration tests for shared tooling.
- `docs/`: project research notes, workflows, specs, and AI handoff logs.

## Requirements

- Rust stable toolchain with Cargo. The crate uses Rust 2024 edition, so use
  Rust 1.85 or newer.
- Windows 10/11 is the current primary development environment.
- A GPU/backend supported by `wgpu` through DX12 or Vulkan for the native GUI.
- Original game assets are not included. Place local assets in ignored working
  folders such as `pipeline_out/`, `debug/`, or other project-specific output
  directories.

## Build

Build the default GUI binary:

```powershell
cargo build
```

Build optimized binaries:

```powershell
cargo build --release
```

Build a specific binary:

```powershell
cargo build --bin gvg_modding_tool
cargo build --bin gvg_converter
```

## Run

Run the native GUI. Because `Cargo.toml` sets `gvg_modding_tool` as the default
binary, plain `cargo run` starts the GUI:

```powershell
cargo run
```

Run the optimized GUI:

```powershell
cargo run --release
```

Run the CLI help:

```powershell
cargo run --bin gvg_converter -- --help
```

Example CLI workflow:

```powershell
cargo run --bin gvg_converter -- extract-pzz `
  "E:/research/gvg_np/Z_DATA.BIN" `
  "E:/research/gvg_np/data_bin_inventory/Z_DATA.BIN.inventory.json" `
  --pzz-name pl00.pzz `
  --out "E:/research/gvg_np/pipeline_out/manual_extract/pl00.pzz"

cargo run --bin gvg_converter -- extract-streams `
  "E:/research/gvg_np/pipeline_out/manual_extract/pl00.pzz" `
  --out "E:/research/gvg_np/pipeline_out/manual_extract/streams"

cargo run --bin gvg_converter -- pmf2-to-dae `
  "E:/research/gvg_np/pipeline_out/manual_extract/streams/stream000.pmf2" `
  --out "E:/research/gvg_np/pipeline_out/manual_extract/stream000.dae" `
  --name stream000
```

More focused command notes live in `docs/RUST_DAE_PMF2_COMMANDS.md` and the full
asset workflow lives in `docs/MOD_WORKFLOW.md`.

## Test And Format

Run tests:

```powershell
cargo test
```

Run one integration test target:

```powershell
cargo test --test workspace_tooling
```

Format Rust code:

```powershell
cargo fmt
```

Run Clippy if installed:

```powershell
cargo clippy --all-targets
```

## Debug

Enable Rust backtraces in PowerShell:

```powershell
$env:RUST_BACKTRACE = "1"
```

Debug the GUI with backtraces:

```powershell
$env:RUST_BACKTRACE = "1"
cargo run
```

Debug the CLI:

```powershell
$env:RUST_BACKTRACE = "1"
cargo run --bin gvg_converter -- --help
```

Show test output while debugging:

```powershell
cargo test -- --nocapture
```

Useful investigation docs:

- `docs/PMF2_TODO.md`
- `docs/PMF2_M00_RENDER_ANALYSIS.md`
- `docs/PMF2_SPECIAL_SECTIONS_ANALYSIS.md`
- `docs/GIM_REPLACE_NOTES.md`
- `docs/PPSSPP_OPERATION_ANALYSIS.md`

## AI Agent Notes

AI agents must read `AGENTS.md` before working in this repository, then inspect
the relevant Markdown research notes under `docs/` and maintain a topic-specific
handoff log under `docs/agent-sessions/<topic>/`.

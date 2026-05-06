# Session: GitHub Release on master

## Decisions

- **Trigger:** `push` to branch `master` only.
- **Workflow file:** `.github/workflows/release-master.yml` (replaces prior `release-main.yml`).
- **Runner:** `windows-latest` — MSVC build for `gvg_modding_tool.exe`.
- **Build speed:** shallow `checkout` (`fetch-depth: 1`), `Swatinem/rust-cache@v2` with `shared-key: release-windows`, sparse crates.io protocol env, `codegen-units = 256` in `[profile.release]`.
- **Binary/package size:** `[profile.release]` uses `opt-level = "s"`, `strip = true`, plus `RUSTFLAGS` `-C link-arg=/OPT:REF` and `/OPT:ICF`. Release asset is a **zip** of the exe (`CompressionLevel Optimal`) to reduce download size; artifact matches.
- **Release:** `softprops/action-gh-release@v2`, tag `master-build-${{ github.run_number }}-${{ github.run_id }}`.

## Verification

- `cargo build --release --locked --bin gvg_modding_tool` after profile change.
- On GitHub: workflow runs on `master` push; Release lists one zip asset.

# github-windows-release

## Status

- [x] Workflow: `.github/workflows/release-master.yml` — push to `master` builds release, zips exe, publishes GitHub Release.
- [x] `Cargo.toml` `[profile.release]`: `opt-level = "s"`, `codegen-units = 256`, `strip = true`.

## Notes

- Tag pattern: `master-build-<run_number>-<run_id>`.
- CI sets `RUSTFLAGS` with MSVC `/OPT:REF` and `/OPT:ICF` for a smaller linked binary.

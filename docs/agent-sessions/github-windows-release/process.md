# Session: GitHub Release on main

## Decisions

- **Trigger:** `push` to branch `main` only.
- **Runner:** `windows-latest` — native MSVC build for `gvg_modding_tool.exe`.
- **Release:** `softprops/action-gh-release@v2` with unique `tag_name` per run (`main-build-${{ github.run_number }}-${{ github.run_id }}`) so every push creates a new release without tag collisions.
- **Build:** `cargo build --release --locked --bin gvg_modding_tool`.
- **Artifact:** Also uploaded via `actions/upload-artifact` for CI downloads without opening Releases.

## Verification

- After merge: confirm workflow appears under Actions and a Release is created with attached `gvg_modding_tool.exe`.
- If `cargo build --locked` fails due to an outdated `Cargo.lock`, run `cargo build` locally and commit the lockfile.

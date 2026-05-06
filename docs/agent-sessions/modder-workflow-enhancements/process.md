# Modder Workflow Enhancements — Process Log

## 2026-05-06 Session (Cursor)

### Context
Continuing from previous Claude CLI session (714e0b77) which hit rate limits.
That session only managed: TreeAction enum additions + pan()/reset_view() methods.

### Decisions from grill session
1. Root node: add `afs_filename()` to `ModWorkspace`, show at tree top
2. Save: unified `rebuild_afs()` replaces `patch_entries_bytes()` path
3. Logo: placeholder image, implement in spec order (Phase 1 first)

### Implementation completed

**Phase 1 — Tree View & Export:**
- `workspace.rs`: Added `afs_filename()`, `afs_entry_count()`, `rename_entry()`
- `asset_tree.rs`: AFS root node row, removed `#{:04}` from labels, updated context menus
- `inspector.rs`: Added `InspectorAction` return type, editable name field with 0x20 limit
- `gui.rs`: Wired all new TreeActions (ExportDecryptedPzz, ExportPzzStreams, DumpAfsToFolder, etc.)
- Three new methods: `export_decrypted_pzz()`, `export_pzz_streams()`, `dump_afs_to_folder()`

**Phase 2 — AFS Rebuild:**
- `afs.rs`: Added `AfsRebuildEntry` struct + `rebuild_afs()` function with full name table
- Test: `rebuild_afs_produces_valid_archive_readable_by_scan_inventory` passes

**Phase 3 — 3D Preview:**
- `gui.rs`: Right-click drag → `cam.pan()`, left-click drag → orbit (unchanged)
- `preview.rs`: "Frame" → "Focus Model" + new "Reset View" button
- `gpu_renderer.rs`: `create_ground_grid(device, extent, step)` parametrized, added `update_grid()` and `compute_grid_params()`
- `gui.rs` update_gpu_mesh: dynamic grid update on model change

**Phase 4 — Hex Viewer:**
- `editors.rs`: New `HexViewTarget`, `HexViewerState` structs; `EditorWindows.hex_views: Vec<HexViewerState>`
- Three-column layout: offset | hex (4-byte groups) | ASCII
- Per-byte clickable labels with highlight sync
- Multi-window support (independent per-target)

### Verification
- `cargo check` passes with no warnings
- `cargo test` — 18 tests pass (17 original + 1 new rebuild_afs test)

### Files modified
- `src/afs.rs` — rebuild_afs() + AfsRebuildEntry + test
- `src/workspace.rs` — afs_filename(), afs_entry_count(), rename_entry()
- `src/gui.rs` — entry_name_edit_buf, inspector_action handling, new export methods, pan/grid wiring
- `src/gui/asset_tree.rs` — root node, removed index from labels, updated context menus
- `src/gui/inspector.rs` — InspectorAction, editable name field
- `src/gui/preview.rs` — Focus Model + Reset View buttons
- `src/gui/editors.rs` — hex viewer overhaul (multi-window, 3-col, byte interaction)
- `src/gpu_renderer.rs` — parametrized grid, compute_grid_params(), update_grid()
- `src/render.rs` — pan() + reset_view() (from previous session, kept)

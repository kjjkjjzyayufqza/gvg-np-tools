# PMF2 Performance Optimization — Process Log

## Context Gathered (2026-05-04)

### Root Cause Analysis (IDA + Source)

FPS drops when replacing models with high-poly DAE → PMF2 conversions are caused
by three factors:

1. **GE command buffer hard limit: 256KB per frame**
   - `sub_89BD000` (`0x89BD058`): double-buffered 256KB command lists
   - Overflow → `while(1)` infinite loop crash
   - Each bone draw adds ~200 bytes to the master list

2. **PSP GE vertex throughput bottleneck**
   - The converter previously generated **unindexed TRIANGLES** (PRIM type 3)
   - Each triangle = 3 separate vertices × 16 bytes = 48 bytes per triangle
   - Native game models use **indexed TRIANGLE_STRIP** (PRIM type 4 + IADDR)
   - Indexed drawing shares vertices → ~50-70% less vertex data

3. **VSync staircase**
   - Game calls `sceDisplayWaitVblankStart` at frame end
   - Frame > 16.67ms → 30fps, > 33.33ms → 20fps, etc.

### Native vs Converter GE Command Comparison

| Property | Native PMF2 | Old Converter | New Converter |
|----------|------------|---------------|---------------|
| VTYPE | `0x1142` (indexed) | `0x0142` (no index) | `0x1142` (indexed) |
| Index buffer | IADDR present | No IADDR | IADDR present |
| PRIM type | TRIANGLE_STRIP (4) | TRIANGLES (3) | TRIANGLES (3) |
| Vertex sharing | Yes (indexed) | No (duplicated) | Yes (deduplicated) |

### Key Source Locations

- `src/pmf2.rs:999` — `encode_vertices_i16()` — shared vertex encoding helper
- `src/pmf2.rs:1037` — `serialize_vertex_buf()` — shared vertex buffer serializer
- `src/pmf2.rs:1055` — `build_ge_commands()` — indexed output (production path)
- `src/pmf2.rs:694` — `extract_per_bone_meshes()` — indexed parsing (fixed bounds check)

## Implementation (2026-05-04)

### Changes Made

**`src/pmf2.rs`** — 3 logical changes:

1. **Refactored `build_ge_commands` to produce indexed triangles:**
   - Extracted `encode_vertices_i16()` and `serialize_vertex_buf()` as shared helpers
   - Added HashMap-based vertex deduplication on `EncodedVertex` (i16 tuple, exact match)
   - Generates u16 index buffer alongside deduplicated vertex buffer
   - VTYPE now includes `2 << 11` (16-bit index format) → `0x1142` matches native
   - Emits IADDR (0x02) GE command per chunk pointing at index buffer region
   - Layout: `[GE commands] [unique vertex buffer] [u16 index buffer]`

2. **Fixed `extract_per_bone_meshes` indexed bounds check:**
   - Old code checked `dc.vaddr + vs * count > data.len()` before branching on
     indexed vs non-indexed. For indexed mode, `count` is index count (larger
     than unique vertex count), causing false-positive rejection.
   - Moved the bounds check into the non-indexed branch only. The indexed branch
     already has its own correct check via `max_idx`.

3. **Added 3 new unit tests:**
   - `build_ge_commands_uses_indexed_format` — verifies IADDR present, VTYPE has idx_fmt=2
   - `build_ge_commands_deduplicates_shared_vertices` — 2 identical faces produce 3 unique verts + 6 indices
   - `build_ge_indexed_round_trips_through_extract` — full rebuild → extract round-trip preserves face count

### Commands and Results

```
cargo check                  → clean
cargo test --lib             → 62 passed, 0 failed
```

### Performance Impact Estimate

For a mesh with N triangles and V unique vertices (V ≤ 3N):

| Metric | Old (unindexed) | New (indexed) | Ratio |
|--------|----------------|---------------|-------|
| Vertex buffer | 3N × 16B = 48N bytes | V × 16B | V/3N (typically 0.3-0.5) |
| Index buffer | 0 | 3N × 2B = 6N bytes | n/a |
| Total data | 48N bytes | V×16 + 6N bytes | ~50-70% of original |
| GE vertex reads | 3N | V (cached via index) | V/3N |
| GE cmd overhead | 4 cmds/chunk | 5 cmds/chunk | +1 IADDR per chunk |

Example: 3000-triangle model with 50% vertex sharing (V = 4500):
- Old: 3000 × 48 = 144,000 bytes
- New: 4500 × 16 + 3000 × 6 = 72,000 + 18,000 = 90,000 bytes (62% of original)

### Next Steps

1. **User testing**: Build a modded PZZ with the new indexed output and test in PPSSPP.
2. **Phase 2 (future)**: Triangle strip conversion for additional ~30% reduction.
   This requires a stripify algorithm (e.g., greedy or NvTriStrip-style).
3. **Append path**: `append_mesh_draw_to_template_section` (test-only) still uses
   unindexed format. Update if it becomes production code.

## Follow-up Analysis (2026-05-04)

### User Test Case

Input:

```text
E:\research\gvg_np\game_assets\z_data\testout.dae
```

Output:

```text
E:\research\gvg_np\game_assets\z_data\test.pmf2
```

Read-only statistics showed:

```text
testout.dae size: 9,271,342 bytes
test.pmf2 size:   3,667,760 bytes
DAE geometries:   57
DAE triangles:    77,145 total
```

The imported high-poly pieces dominate the file:

```text
Geometry52 / _052_ms_MeshPart0: 48,792 triangles -> pl0a_m01
Geometry54 / _054_ms_MeshPart1: 15,091 triangles -> pl0a_m01
Geometry53 / _053_ms_MeshPart0:  4,516 triangles -> pl0a_m01
Geometry55 / _055_ms1:           4,448 triangles -> pl0a_m01
Geometry56 / _056_ms1:             608 triangles -> pl0a_m01
```

All five large imported geometries are dominantly weighted to `pl0a_m01`, so the
DAE importer merges them into one `BoneMeshMeta` for section index 1. The
generated `test.pmf2` then contains:

```text
pl0a_m01 section size: 3,526,176 bytes
PRIM calls:            4
PRIM type:             3 (TRIANGLES)
VTYPE:                 0x0142
IADDR:                 absent
PRIM vertex count:     220,365 vertices = 73,455 triangles
```

This explains why the indexed-output change appeared to have little effect for
this file. `build_ge_commands()` currently uses an all-or-nothing indexed path:
it deduplicates the whole bone mesh first, then only emits indexed drawing if the
global unique vertex count fits in a u16 index buffer. For this `m01` mesh:

```text
sequential vertices:       220,365
global unique vertices:     89,390
u16 index limit:            65,535
```

Because `89,390 > 65,535`, the converter falls back to unindexed `TRIANGLES`.

### Optimization Implication

The next useful converter optimization is not just "indexed triangles" globally,
but **chunk-local indexed triangles**:

```text
current 4 chunks:
  65,532 vertices -> 27,718 unique
  65,532 vertices -> 25,797 unique
  65,532 vertices -> 26,237 unique
  23,769 vertices ->  9,651 unique
```

Each chunk fits u16 indexing if it owns its own local vertex buffer and index
buffer. Estimated `m01` data size:

```text
current unindexed vertex bytes:      3,525,840
chunk-local indexed data estimate:   1,871,178
estimated reduction:                 ~47%
```

This should reduce PMF2 size and GE vertex bandwidth significantly, but it does
not change the physical triangle count. The test model still asks PSP/GE to draw
about 73k triangles for one section and 77k total triangles for the character,
which is roughly twenty times the native character budget. A converter-side
chunk-index optimization may improve the result, but polygon reduction/LOD is
still required for reliable 60fps.

## Implementation Follow-up (2026-05-04)

Changed `src/pmf2.rs` production `build_ge_commands()` output from global
all-or-nothing indexed triangles to **chunk-local indexed triangles**:

```text
old behavior:
  flatten all faces
  deduplicate globally
  if global unique vertices <= 65535:
      emit indexed triangles
  else:
      fall back to unindexed TRIANGLES

new behavior:
  flatten all faces
  split into PRIM-sized triangle chunks
  for each chunk:
      deduplicate only that chunk
      emit VADDR + VTYPE(indexed) + IADDR + PRIM
      append that chunk's vertex buffer and u16 index buffer
```

This directly targets the `testout.dae -> test.pmf2` failure mode where
`pl0a_m01` had ~89k global unique encoded vertices but each chunk was under the
u16 index limit.

Added regression:

```text
build_ge_commands_indexes_each_chunk_when_global_unique_vertices_exceed_u16
```

The test constructs a mesh with more than 65,535 globally unique encoded
vertices and verifies every generated PRIM chunk has a matching `IADDR` and
indexed `VTYPE`.

Verification:

```text
cargo test --lib build_ge_commands
  5 passed

cargo test --lib
  63 passed

cargo check
  finished successfully
```

## Stripify Implementation (2026-05-04)

Implemented a conservative/aggressive hybrid stripifier for generated PMF2 mesh
draws:

```text
per chunk:
  deduplicate vertices into a local u16 vertex table
  build local triangle faces
  greedily extend strips through same-winding shared edges
  join profitable disconnected strips with degenerate indices
  if strip output is smaller and <= u16::MAX:
      emit indexed TRIANGLE_STRIP
  else:
      emit indexed TRIANGLES
```

The fallback is important because scattered disconnected triangles can become
larger after degenerate strip joins. In that case the converter keeps the
chunk-local indexed `TRIANGLES` output from Phase 2.

Added tests:

```text
stripify_turns_quad_triangles_into_four_index_strip
stripify_uses_degenerate_indices_to_join_disconnected_strips
stripify_preserves_winding_across_three_triangle_chain
stripify_skips_single_triangle
stripify_skips_when_disconnected_triangles_would_grow
build_ge_commands_emits_triangle_strip_for_connected_mesh
build_ge_commands_uses_degenerate_strip_for_two_disconnected_quads
build_ge_commands_falls_back_to_triangles_when_strip_is_not_smaller
build_ge_strip_output_round_trips_through_extract
```

Verification:

```text
cargo test --lib stripify
  5 passed

cargo test --lib build_ge_commands
  8 passed

cargo test --lib round_trips_through_extract
  2 passed

cargo test --lib
  72 passed

cargo check
  finished successfully
```

## 3D Preview Performance Investigation (2026-05-04)

User reported the GVG tools 3D preview becomes very sluggish with relatively
high face counts (around 10k faces) and asked to compare against
`E:\research\ssbh_editor`, which can display much larger scenes smoothly.

### Evidence Gathered

- Current `cargo run` was a dev/unoptimized GUI run (`target\debug\gvg_modding_tool.exe`).
- Current preview already initializes wgpu and uploads selected PMF2 meshes:
  `GPU mesh uploaded for stream 0: 52/53 bone meshes`.
- User test history in the same terminal shows a patched stream with a dominant
  `pl0a_m01` mesh around `73,455` faces, then repeated GPU mesh reuploads after
  stream replacement.
- Existing 3D preview design doc identified the old CPU renderer bottlenecks:
  per-frame CPU projection, CPU triangle sorting, and egui shape drawing.
  Those old bottlenecks were addressed by adding `src/gpu_renderer.rs`, but new
  GPU-path bottlenecks remain.

### Root-Cause Findings

1. `src/gui.rs::show_3d_preview()` calls `GpuRenderer::render()` on every egui
   repaint while the preview is visible. During camera drag/scroll this means a
   full off-screen render, command encoder creation, and `queue.submit()` every
   UI frame.
2. `GpuRenderer::render()` rebuilds wireframe data every frame when wireframe is
   enabled:
   - allocates a new `Vec<u32>` via `build_wireframe_indices(mesh.index_count)`
   - creates a new GPU index buffer with `device.create_buffer_init`
   - then draws the wireframe pass
   For 10k triangles this creates about 60k line indices per frame; for the
   observed 73k-triangle mesh it creates about 440k line indices per frame.
3. The current wireframe path uses `PrimitiveTopology::LineList` with a derived
   index buffer, while `ssbh_wgpu` uses a persistent mesh index buffer and
   `wgpu::PolygonMode::Line` for wireframe. That avoids per-frame CPU index
   expansion and GPU buffer allocation.
4. GVG stores one flattened `GpuMesh`, but upload still happens synchronously on
   the UI thread after PMF2 extraction. This is acceptable for occasional stream
   changes, but high-poly imports/replacements will visibly stall the UI.
5. GVG uses off-screen texture registration and image presentation. `ssbh_editor`
   uses `egui_wgpu::CallbackTrait` and renders via the eframe/egui render pass
   integration, with long-lived renderer resources in callback resources. That
   avoids extra per-preview `queue.submit()` and keeps most GPU resources
   persistent.

### ssbh_editor / ssbh_wgpu Patterns

- `ssbh_editor` stores renderer state in egui callback resources and inserts an
  `egui_wgpu::Callback` for the viewport.
- `ssbh_wgpu::RenderModel` owns combined, persistent vertex/index buffers and
  draws mesh slices using `RenderMesh.access`.
- Wireframe rendering reuses the same mesh buffers and switches to a
  `PolygonMode::Line` pipeline instead of building a separate line index buffer
  every frame.
- The renderer batches model rendering through `begin_render_models()` and
  `end_render_models()` rather than rebuilding per-frame mesh resources.

### Recommended Optimization Order

1. Cache wireframe indices/buffer inside `GpuMesh`, or preferably switch to a
   `PolygonMode::Line` wireframe pipeline when supported.
2. Add render dirty-state so the off-screen preview only rerenders when camera,
   viewport, mesh, texture, or render toggles change.
3. Move render integration toward `egui_wgpu::CallbackTrait` like
   `ssbh_editor`, so preview commands are recorded into egui's wgpu flow instead
   of submitting immediately from UI code.
4. Add timing instrumentation around PMF2 extraction, mesh upload, wireframe
   buffer creation, and render submission to confirm the dominant cost before
   and after changes.

## 3D Preview Persistent Wireframe Implementation (2026-05-04)

Implemented the first preview optimization in `src/gpu_renderer.rs`:

- Added cached wireframe GPU resources to `GpuMesh`.
- Changed `upload_mesh()` to build wireframe line indices once from the real
  triangle index buffer and upload `mesh_wire_ib` alongside the normal mesh
  buffers.
- Changed `render()` to reuse `mesh.wireframe_index_buffer` instead of
  allocating a `Vec<u32>` and creating a GPU index buffer every frame.
- Fixed the wireframe index derivation to use actual triangle indices instead
  of assuming every triangle references sequential vertex indices.
- Added upload timing/count logging:
  `meshes`, `verts`, `tri_indices`, `wire_indices`, `elapsed_ms`.

Added regression:

```text
gpu_renderer::tests::wireframe_indices_reuse_triangle_indices
```

TDD / verification notes:

```text
cargo test --lib gpu_renderer::tests::wireframe_indices_reuse_triangle_indices
  initially failed with E0308 because build_wireframe_indices still accepted
  a triangle-index count instead of the real triangle index slice.

cargo test --lib gpu_renderer::tests::wireframe_indices_reuse_triangle_indices
  1 passed

cargo fmt && cargo test --lib gpu_renderer && cargo check
  skipped: current PowerShell does not support && command chaining.

cargo fmt; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }; cargo test --lib gpu_renderer; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }; cargo check
  gpu_renderer test passed
  cargo check finished successfully

cargo test --lib
  64 passed
  0 failed
```

## 3D Preview FPS / Debug Instrumentation (2026-05-04)

Added preview-side diagnostics for the reported high-poly case:

- `src/gui/preview.rs`
  - Added a preview perf label that displays:
    `FPS`, vertex count, face count, triangle index count, wireframe index count,
    and the previous frame render time.
  - Added regression:
    `gui::preview::tests::preview_perf_line_includes_fps_and_mesh_counts`.
- `src/gpu_renderer.rs`
  - Added `GpuRenderStats` from `render()` with encode, submit, total render
    CPU time, and viewport size.
  - Added `GpuMesh` count accessors so UI/logging can report the actual uploaded
    mesh counts.
- `src/gui.rs`
  - Stores the most recent preview render stats and shows them in the preview
    panel.
  - Logs one throttled line per second while preview is rendering:
    `fps`, viewport, vertices, faces, triangle indices, wireframe indices,
    `render_ms`, `encode_ms`, `submit_ms`.

For the user-provided PMF2 summary, expected uploaded counts should be close to:

```text
verts ~= 231435
faces ~= 77145
tri_indices ~= 231435
wire_indices ~= 462870
```

Verification:

```text
cargo test --lib gui::preview::tests::preview_perf_line_includes_fps_and_mesh_counts
  initially failed because format_preview_perf_line did not exist yet.

cargo test --lib gui::preview::tests::preview_perf_line_includes_fps_and_mesh_counts
  1 passed

cargo fmt; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }; cargo test --lib gpu_renderer gui::preview; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }; cargo check
  skipped after cargo fmt: cargo test accepts only one filter argument.

cargo test --lib gpu_renderer; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }; cargo test --lib gui::preview; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }; cargo check
  gpu_renderer test passed
  gui::preview test passed
  cargo check finished successfully

cargo test --lib
  65 passed
  0 failed
```

## 3D Preview Low FPS Root Cause Follow-up (2026-05-04)

User provided live diagnostics:

```text
FPS ~= 7.3
verts=231435
faces=77145
tri_indices=231435
wire_indices=462870
render_ms ~= 0.44
encode_ms ~= 0.13
submit_ms ~= 0.31
```

This showed the GPU preview render path was not the bottleneck. The frame only
spent about half a millisecond in `GpuRenderer::render()`, but total UI cadence
was around 7 FPS.

Root cause found in `src/gui/inspector.rs`:

- The right Inspector calls `show_pmf2_summary()` every egui frame.
- `show_pmf2_summary()` called `pmf2::parse_pmf2_sections(data)` and, more
  importantly, `pmf2::extract_per_bone_meshes(data, false)` every frame.
- For the user's selected PMF2 this meant re-extracting about 231k vertices and
  77k faces continuously just to redraw the right-side summary text.

Implemented fix:

- Added `Pmf2SummaryCache` keyed by stream index, PZZ revision, and data identity.
- Added `Pmf2Summary` to store sections, bbox, mesh counts, render policy counts,
  total vertices, and total faces.
- Added one-time logging when the summary cache is rebuilt:
  `[gui] cached PMF2 summary stream=... verts=... faces=... in ...`.
- Added `inspector_pmf2_cache` to `GvgModdingApp` and passed it into the
  inspector alongside the existing GIM preview cache.

Added regression:

```text
gui::inspector::tests::pmf2_summary_cache_key_hits_only_for_same_stream_revision_and_data
```

Verification:

```text
cargo test --lib gui::inspector::tests::pmf2_summary_cache_key_hits_only_for_same_stream_revision_and_data
  initially failed because Pmf2SummaryCache did not implement cache-key behavior.

cargo test --lib gui::inspector::tests::pmf2_summary_cache_key_hits_only_for_same_stream_revision_and_data
  1 passed

cargo fmt; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }; cargo test --lib gui::inspector; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }; cargo check
  inspector test passed
  cargo check finished successfully

cargo test --lib
  66 passed
  0 failed
```

## DAE UV Flip Option + GIM Replace Config Dialog (2026-05-04)

User requested two UX/behavior changes:

1. When running "Replace from DAE" for PMF2, prompt whether UV V should be
   flipped.
2. Remove the top-left global GIM PNG format selector and instead show a config
   dialog after right-click "Replace from PNG", with format selection and a
   PNG preview that fills the dialog area.

### TDD (RED -> GREEN)

Added a new DAE importer test first:

```text
dae::tests::dae_import_can_keep_collada_v_when_uv_flip_disabled
```

Initial RED run:

```text
cargo test --lib dae_import_can_keep_collada_v_when_uv_flip_disabled
  failed (E0425): parse_dae_to_meta_text_with_uv_flip not found
```

### Implementation Summary

#### `src/dae.rs`

- Added `read_dae_to_meta_with_uv_flip(path, model_name, flip_uv_v)`.
- Kept `read_dae_to_meta(...)` as the default behavior wrapper (`flip_uv_v = true`)
  to avoid breaking existing CLI flow.
- Added internal parser path with UV policy:
  `parse_dae_to_meta_text_with_uv_flip(xml, model_name, flip_uv_v)`.
- During import, UV V now resolves as:
  - `1.0 - v` when `flip_uv_v = true` (previous behavior)
  - `v` when `flip_uv_v = false`.

#### `src/gui.rs`

- Removed the top menu `GIM Replace` format combo box.
- Added pending dialog state for DAE replace and GIM replace flows.
- `Replace from DAE` now:
  1) picks DAE file,
  2) opens a config modal with `Flip UV V (v = 1 - v)` checkbox,
  3) applies replacement only after confirmation.
- `Replace from PNG` (right-click stream action) now:
  1) picks PNG file,
  2) opens a config modal with format `ComboBox`,
  3) shows PNG preview using remaining modal area (`fit_to_exact_size`) to fill
     the dialog visually,
  4) applies replacement only after confirmation.
- Added helper `decode_png_preview_color_image(...)` to convert selected PNG into
  egui `ColorImage` for modal preview.

### Added/Updated Tests

- `dae::tests::dae_import_can_keep_collada_v_when_uv_flip_disabled`
- `gui::tests::decode_png_preview_preserves_image_dimensions`

### Verification

```text
cargo test --lib dae_import_can_keep_collada_v_when_uv_flip_disabled
  1 passed

cargo test --lib decode_png_preview_preserves_image_dimensions
  1 passed

cargo test --lib
  68 passed
  0 failed

cargo check
  finished successfully
```

### Remaining Manual Check

- Launch GUI and verify the two new config modals:
  - DAE replace UV flip choice behaves as expected on real model.
  - GIM replace PNG preview layout and format selection UX are acceptable.

## Dialog UX Follow-up Fixes (2026-05-04)

User follow-up requested:

1. DAE UV flip option should be **unchecked by default**.
2. `Replace GIM from PNG` config dialog should not keep growing in height.
3. Config dialogs should be draggable windows, not confirm-style dialogs.

### Changes Made

#### `src/gui.rs`

- Changed DAE pending config default:
  - `flip_uv_v: true` -> `flip_uv_v: false`.
- Replaced both config UIs from `egui::Modal` to draggable `egui::Window`:
  - `show_dae_replace_config_modal`
  - `show_gim_replace_config_modal`
- Updated dialog window behavior:
  - `collapsible(false)`, `resizable(true)`, `default_size(...)`, `min_size(...)`.
  - Supports drag/move out of the box.
- Fixed GIM dialog runaway growth by removing `bottom_up + available_size` pattern
  and rendering preview with a stable remaining-area layout:
  - controls in top row
  - separator
  - preview fills current `ui.available_size()` via `ui.add_sized(...)`.

### Verification

```text
cargo test --lib gui::tests::decode_png_preview_preserves_image_dimensions
  1 passed

cargo test --lib dae_import_can_keep_collada_v_when_uv_flip_disabled
  1 passed

cargo check
  finished successfully
```

## Default Camera Angle Tweak (2026-05-04)

User requested changing the preview camera from "behind the character" to a
"front-facing, slightly elevated, looking downward" initial view.

### Change

Updated `src/render.rs` `PreviewCamera::frame_bounds()` defaults:

- `yaw: 0.35` -> `yaw: PI + 0.35` (rotate to front side)
- `pitch: 0.35` -> `pitch: -0.28` (move camera above and look downward)

This only affects newly framed camera states (`camera: None` -> `frame_bounds`).
If a camera is already active for the current preview, reselect stream or reset
preview state to apply.

### Verification

```text
cargo check
  finished successfully
```

## Unit Name -> plXX Table Extraction (2026-05-05)

Goal: complete the remaining step for a human-readable mapping like
`元祖高达 -> pl00` while keeping the previously recovered numeric formula:

- `z_primary = 1649 + unit_id`
- `z_alt = 1726 + unit_id`
- `w_overlay_base = 6 * unit_id`

### IDA MCP Investigation

Used `ida-pro-mcp` `py_eval` with Shift-JIS scanning and verified that EBOOT has
large inlined machine-name string pools (example region around `0x8A5AA80`).

Key evidence:

- Shift-JIS hit scan for `ガンダム` found 363 hits in EBOOT data section.
- `xrefs_to(0x8A5B608)` shows menu/UI call sites including:
  - `sub_8872A9C` (`0x8872A9C`)
  - `sub_890D614` (`0x890D614`)
- `sub_8872A9C` uses `*(&off_8A5B608 + v3)` as the machine name pointer.
- `sub_8916E68` renders machine names from `*(&off_8A5B0C0 + v22)` (same ID-space family).
- Special-case observed: `v3 == 76` can route to `off_8A1A390` (points to
  `ガンダム試作３号機 ( デンドロビウム )`), so this slot has mode/context-specific naming.

### Output

Generated a full 77-row CSV:

- `docs/agent-sessions/pmf2-perf-optimization/unit-name-plxx-table.csv`

Columns:

- `unit_id`
- `pl_code`
- `name_jp`
- `z_data_primary`
- `z_data_alt`
- `w_data_overlay_base`

Example rows:

- `0, pl00, ガンダム, 1649, 1726, 0`
- `10, pl0a, ニューガンダム, 1659, 1736, 60`
- `65, pl41, ダブルオーライザー, 1714, 1791, 390`

This provides the requested readable mapping foundation from machine name to
`plXX` resource package IDs.

## IDA Mapping Controller Deep Dive (2026-05-05)

Follow-up on "which file actually controls character resource mapping".

### Key Trace Path

1. Battle flow calls `sub_890BD84`, then:
   - `sub_8921F60(byte_8EA5786 & 0x7F, byte_8EA5787, v3[5])`
2. `sub_8921F60` does:
   - `sub_8920B5C(word_8A6B604[30*a1 + 10*a2 + a3], &unk_9B62000, &dword_8EA8BD0)`
   - callback `sub_8920A98(13)`
3. `sub_890C148` waits queue completion and then runs `sub_8922010` (relocates
   pointers in `dword_8EA8BD0` block), then `sub_8910D94`.
4. `sub_8910DE0` consumes `dword_8EA8BD0` and sets core per-side pointers
   (`+28/+32/+36`) used by `sub_89116AC`.
5. `sub_89116AC` / `sub_896AD80` populate `byte_8EA8A78`, `byte_8EA89F4`,
   `byte_8EA89FC`, etc., which are directly consumed by `sub_8922D18`.

### Why This Points To `Z_DATA.BIN`

- In `sub_8883C40`, BIN bootstrap table (`off_8A1BD2C`) has:
  - `Z_DATA.BIN` count `0xA5B`.
- In `sub_8886FD8`, loader branch condition is:
  - `resource_id != 0 && resource_id < 0xA5B`.
- The resource ID tables used by mapping (`word_8A6B604`, `word_8A6BA6C`,
  `word_8A6BC50`, `word_8A6BCF8`, `word_8A6C920`) are in this range
  (examples: `1..42`, `1343..1911`), i.e. all under `0xA5B`.

This is a strong code-level indicator that the mapping is controlled by
`Z_DATA.BIN` indexed entries (numeric IDs), not by plain-text names.

### Overlay Pointer Confirmation

- `sub_8922D18` uses `off_8AFE520` slot pointers (`+1..+6`) to:
  - `pl00ov0.bin`
  - `pl00ov1.bin`
  - `pl00ov2.bin`
  - `pl00ov3.bin`
  - `pl00ov4.bin`
  - `pl00ov5.bin`
- But which slot is selected is still driven by the runtime index arrays above
  (`byte_8EA8A78` etc.), whose source chain goes back to `dword_8EA8BD0`.

### Current Conclusion

- The controlling source is:
  - `Z_DATA.BIN` indexed records loaded through `word_8A6B604 -> dword_8EA8BD0`,
  - then transformed into runtime mapping arrays used by overlay loaders.
- No direct plain-text `rx-78-2` or `pl00l` mapping string was found in this IDB.

### Remaining Gap

- Need one more pass to bind a concrete unit ID (e.g. RX-78-2 internal numeric
  ID) to a specific `word_8A6B604[...]` and final `pl00ov*` slot at runtime.

## Unit -> plXX Table Recovery (2026-05-05)

User goal clarified: recover the practical mapping table such as
`RX-78-2 -> pl00`, and corresponding entries for other units.

### Static Table Results

Using IDA `py_eval` on `word_8A6BA6C`, the first 77 unit rows were decoded as:

- `unit_id = 0..76`
- `z_entry_primary = 1649 + unit_id`
- `z_entry_alt = 1726 + unit_id`

This exactly matches known checkpoints:

- `unit_id=0 -> 1649/1726 -> pl00.pzz / pl00l.pzz`
- `unit_id=10 -> 1659 -> pl0a.pzz` (already verified in prior docs/tests)

So the recovered table is:

- `unit_id n` -> primary `pl{n:02x}.pzz` at `Z_DATA` entry `1649+n`
- `unit_id n` -> alt `pl{n:02x}l.pzz` at `Z_DATA` entry `1726+n`

for `n in [0, 76]` (77 units total in this table).

### Related Overlay Table

For `W_DATA` overlays, `word_8A6B8D4[unit_id]` was decoded for all 77 units and
is exactly `6 * unit_id`.

Therefore:

- overlay base entry = `6 * unit_id`
- slots = `base + 0..5`

`unit_id=0` resolves to `0..5` (`pl00ov0..5.bin`) as expected.

## IDA MCP Resource List Investigation (2026-05-05)

User requested checking whether the game has a resource mapping list like:
`rx-78-2 -> pl00/pl00l`.

### Commands / MCP Calls

- `survey_binary`
- `find(type=string, targets=[pl00, pl00l, rx-78-2, ...])`
- `get_string` on all `pl00` hits
- `xrefs_to` and `xref_query` on discovered addresses
- `get_bytes` around `0x8A1BD14`, `0x9BCF820`, and table regions
- `decompile(0x8883C40)` and `decompile(0x8922D18)`
- `py_eval` scripts to decode table entries and string probes

### Findings

1. Found a definite BIN initialization list at `off_8A1BD2C`, used by
   `sub_8883C40`:
   - entry0: `X_DATA.BIN` -> `0x8B829C0`, `0x391F`
   - entry1: `Y_DATA.BIN` -> `0x8B82840`, `0x16`
   - entry2: `Z_DATA.BIN` -> `0x8B81240`, `0xA5B`
   - entry3: `W_DATA.BIN` -> `0x8B80D40`, `0x1E0`

2. Found another resource pointer table at `off_8AFE520`, used inside
   `sub_8922D18`. Slots `+1..+6` map to six overlay blobs whose internal names
   are:
   - `pl00ov0.bin`
   - `pl00ov1.bin`
   - `pl00ov2.bin`
   - `pl00ov3.bin`
   - `pl00ov4.bin`
   - `pl00ov5.bin`

3. No plain-text hit for:
   - `pl00l`
   - `rx-78-2` / `RX-78-2`
   in the loaded IDB image.

### Interpretation

- The binary clearly has resource list structures, but the specific
  `rx-78-2 -> pl00/pl00l` mapping was not found as plain strings in this IDB.
- Most likely that mapping is encoded/indexed (numeric IDs) in loaded data
  blocks (especially `Z_DATA.BIN`) rather than direct text labels.

### Suggested Next Step

- Continue tracing `Z_DATA.BIN` consumers from the `sub_8883C40` load path and
  identify the parser that converts table rows into per-unit resource handles.

## Inspector Runtime Policy Fold + Mesh Visibility List (2026-05-05)

User requested two inspector UX changes:

1. `Runtime Render Policy` should be collapsible and default to collapsed.
2. Add a `mesh list` with per-mesh visibility checkboxes (checked = visible,
   default all checked/visible).

### Changes Made

#### `src/gui/inspector.rs`

- `show_inspector(...)` now accepts `&mut PreviewVisibility` so inspector toggles
  can directly modify preview visibility state.
- `Runtime Render Policy` moved into:
  - `egui::CollapsingHeader::new("Runtime Render Policy").default_open(false)`
- Added `Mesh Visibility` section:
  - one checkbox per PMF2 section with `has_mesh == true`
  - checked state is driven by `PreviewVisibility::is_bone_visible(...)`
  - changes call `PreviewVisibility::set_bone_visible(...)`
  - convenience `All` / `None` buttons included.

#### `src/render.rs`

- Added `PreviewVisibility::mesh_visibility_key()` to generate a stable key from
  hidden-bone set, used for GPU mesh cache invalidation.

#### `src/gui.rs`

- Added `gpu_mesh_visibility_key` cache field.
- Passed `&mut self.preview_state.visibility` into inspector.
- Extended `gpu_mesh_cache_is_current(...)` to include visibility key.
- `update_gpu_mesh()` now:
  - computes current visibility key,
  - filters extracted PMF2 bone meshes by visibility before GPU upload,
  - triggers reupload when visibility selection changes.

### Verification

```text
cargo test --lib gui::tests::gpu_mesh_cache_invalidates_when_selected_stream_revision_changes
  1 passed

cargo check
  finished successfully
```

## Remove Preview Frame Spam Logs (2026-05-04)

User requested deleting repeated terminal lines like:

```text
[gpu] preview frame: fps=..., viewport=..., verts=..., ...
```

### Change

Updated `src/gui.rs` to remove the periodic preview debug logger path:

- Removed `preview_debug_last_log` field from `GvgModdingApp`.
- Removed call site in `show_3d_preview(...)` that emitted per-second frame logs.
- Removed helper `log_preview_debug_if_due(...)` and its `eprintln!`.

This keeps preview rendering behavior unchanged while stopping terminal spam.

### Verification

```text
cargo check
  finished successfully
```

## Inspector GIM Full-Area Preview (2026-05-04)

User requested that when left-click selecting any GIM stream, the Inspector view
should use the whole remaining area to display the texture, instead of a small thumbnail.

### Change

Updated `src/gui/inspector.rs` `show_gim_summary(...)`:

- Kept compact metadata line (`dimensions | format | swizzled`) at top.
- Replaced fixed thumbnail logic (`max_side = 200`) with full-area rendering.
- Uses current remaining inspector size:
  - `let available = ui.available_size();`
  - `ui.add_sized(available, Image::...fit_to_exact_size(available))`
- Added small fallback message when inspector area is too small.

This makes selected GIM preview fill the inspector content region.

### Verification

```text
cargo check
  finished successfully
```

## Camera Focus Offset Fix (2026-05-04)

User observed default camera framing looked slightly off-center.

### Root Cause

Default framing targeted the AABB center (`(min + max) / 2`). For asymmetric
meshes (weapons/wings/offset geometry), AABB center can drift away from the
visual character center.

### Change

- Added centroid-based focus target on GPU mesh build:
  - `GpuMesh.focus_target: [f32; 3]`
  - computed as average of all uploaded vertex positions.
- Added `PreviewCamera::frame_bounds_with_target(bounds, target)`.
- Switched initial camera framing and `Frame` button to use:
  - bounds for distance/frustum
  - centroid for target.

Updated files:

- `src/gpu_renderer.rs`
- `src/render.rs`
- `src/gui.rs`
- `src/gui/preview.rs`

### Verification

```text
cargo check
  finished successfully
```

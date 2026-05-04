# PMF2 Performance Optimization — TODO

## Goal

Optimize the DAE → PMF2 converter to produce indexed triangle display lists
instead of unindexed triangles, reducing vertex data and improving in-game
rendering performance on PSP hardware.

Additional follow-up: investigate and optimize GVG tools 3D preview
performance for high-face-count PMF2 models, using `ssbh_editor` /
`ssbh_wgpu` as the reference implementation.

## Tasks

### Phase 1: Indexed Triangle Output

- [x] 1.1 Extract shared helpers `encode_vertices_i16` and `serialize_vertex_buf`
      from `build_ge_commands`.
- [x] 1.2 Add vertex deduplication via HashMap on `EncodedVertex` tuples.
- [x] 1.3 Generate IADDR (0x02) GE command before PRIM to point at index buffer.
- [x] 1.4 Update VTYPE to include 16-bit index format (bits [12:11] = 2).
- [x] 1.5 Chunk PRIM commands by index count limit (u16 max via
      `triangle_prim_chunks`).
- [x] 1.6 Layout: commands | unique vertex buffer | u16 index buffer.
- [x] 1.7 Fix `extract_per_bone_meshes` bounds check — was rejecting indexed
      draw calls because it checked `vaddr + vertex_size * index_count` instead
      of deferring to the indexed branch's `max_idx` check.
- [x] 1.8 Add 3 unit tests: indexed format, vertex dedup, extract round-trip.
- [x] 1.9 All 62 tests pass, `cargo check` clean.

### Phase 2: Triangle Strip Conversion (FUTURE)

### Phase 2: Chunk-Local Indexed Output

- [x] 2.1 Change `build_ge_commands()` so meshes with more than 65,535 global
      unique vertices do not fall back to fully unindexed output.
- [x] 2.2 Split triangle lists into chunks first, then deduplicate and index each
      chunk independently.
- [x] 2.3 Emit per-chunk vertex buffers and u16 index buffers, with `IADDR` and
      indexed `VTYPE` for each chunk.
- [x] 2.4 Add a regression for the `testout.dae -> test.pmf2` shape: a bone mesh
      whose global unique vertex count exceeds u16 but whose chunks fit u16.

### Phase 3: Triangle Strip Conversion (FUTURE)

- [x] 3.1 Implement triangle list → triangle strip conversion (stripify).
- [x] 3.2 Handle degenerate triangles for strip joins.
- [x] 3.3 Change PRIM type from TRIANGLES (3) to TRIANGLE_STRIP (4), where safe.
- [x] 3.4 Add unit tests for strip output and degenerate handling.

### Phase 4: Validation

- [x] 4.1 Run `cargo test` — all 62 tests pass for Phase 1.
- [ ] 4.2 End-to-end test with actual game model (user testing).
- [x] 4.3 Document performance expectations in process.md.
- [x] 4.4 Analyze `testout.dae -> test.pmf2`; current output still falls back
      to unindexed `TRIANGLES` for `pl0a_m01` because global unique vertices
      exceed u16.

## Status

- Phase 1: COMPLETE
- Phase 2: COMPLETE
- Phase 3: COMPLETE
- Phase 4: PARTIAL (automated tests done, actual user model analyzed)

## 3D Preview Follow-up

- [x] Read current GVG preview design and GPU renderer code.
- [x] Compare against `ssbh_editor` and `ssbh_wgpu` rendering architecture.
- [x] Identify likely preview-side bottlenecks:
      per-repaint off-screen render, per-frame wireframe index/buffer rebuild,
      synchronous high-poly mesh extraction/upload.
- [x] Implement first optimization: persistent wireframe path
      (cached line index buffer or `PolygonMode::Line` pipeline).
- [x] Add lightweight upload timing/count logs for GPU mesh and cached wireframe indices.
- [x] Add FPS display and throttled render-frame debug logs.
- [x] Cache PMF2 inspector summary to avoid re-extracting 231k vertices every frame.
- [ ] Re-test with the user high-poly PMF2/DAE case in release mode.

## Replace Workflow Follow-up (2026-05-04)

- [x] Add DAE -> PMF2 replace config dialog to choose whether UV V should be flipped.
- [x] Add parser option so DAE import can keep Collada V when UV flip is disabled.
- [x] Remove top menu GIM replace format selector.
- [x] Change right-click GIM "Replace from PNG" flow to open a config dialog after PNG selection.
- [x] Add GIM replace config dialog format picker with full-window PNG preview.
- [x] Add regressions for optional DAE UV flip and PNG preview decode helper.
- [x] Default DAE UV flip checkbox to unchecked.
- [x] Switch DAE/GIM config dialogs from confirm-style modal to draggable windows.
- [x] Fix GIM replace dialog height runaway by replacing bottom-up auto-grow layout.
- [x] Adjust default 3D preview camera to front-facing with slight top-down angle.
- [x] Fix camera focus offset by targeting mesh centroid instead of AABB center.
- [x] Make inspector GIM preview fill the remaining inspector area.
- [x] Remove throttled `[gpu] preview frame ...` runtime spam logs.
- [ ] Manual GUI verification in app: DAE dialog and GIM preview dialog layout/UX.

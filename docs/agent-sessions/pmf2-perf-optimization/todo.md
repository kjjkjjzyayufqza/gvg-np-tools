# PMF2 Performance Optimization — TODO

## Goal

Optimize the DAE → PMF2 converter to produce indexed triangle display lists
instead of unindexed triangles, reducing vertex data and improving in-game
rendering performance on PSP hardware.

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

- [ ] 3.1 Implement triangle list → triangle strip conversion (stripify).
- [ ] 3.2 Handle degenerate triangles for strip joins.
- [ ] 3.3 Change PRIM type from TRIANGLES (3) to TRIANGLE_STRIP (4), where safe.
- [ ] 3.4 Add unit tests for strip output and degenerate handling.

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
- Phase 3: NOT STARTED (deferred — stripification after chunk-local indexing)
- Phase 4: PARTIAL (automated tests done, actual user model analyzed)

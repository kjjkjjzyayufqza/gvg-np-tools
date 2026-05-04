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

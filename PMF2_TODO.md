# PMF2 / DAE Mesh Import TODO

## 1. Investigate `*_m00out` Mesh Not Rendering

- [ ] Verify whether `m00` is a root/control bone that the game intentionally skips for rendering.
  - Latest test: `add_multi_pcube1_bind_m00out` now previews correctly, but the game renders nothing.
  - This strongly suggests the geometry/display-list bytes are parseable, but runtime render traversal or root/control-bone policy skips `m00`.
- [x] Check whether PMF2 has a specific `i32`/flag controlling whether a section is renderable.
  - IDA confirmed PMF2 section access:
    - `sub_88BD214(pmf2, index)` returns `pmf2 + *(pmf2 + 0x20 + index * 4)`, i.e. section pointer.
    - `sub_88BD1D4(section, render_entry)` checks `*(section + 0x70) == 0` before assigning a render/material pointer.
  - IDA corrected the meaning of `section+0x74`:
    - It is used as a material index: `render_entry+4 = material_table + 36 * *(section + 0x74)`.
    - Clearing it means "use material 0", not "clear a no-render flag".
  - Converter currently clears `section+0x74` when enabling a previously no-mesh section so new meshes use material 0.
- [x] Compare `m00` section header bytes against known renderable sections such as `m01`, `m07`, and `m11`.
- [x] Test whether forcing `m00` header flags to match a visible mesh section makes the game render it.
  - Candidate files: `converted_out/debug_tmp/probe_multi_m00_hdr.pmf2`, `probe_multi_m01_hdr.pmf2`, `probe_multi_m07_hdr.pmf2`, `probe_high_m00_hdr.pmf2`.
  - Result: `m01` renders normally with the same PMF2 fixes, while `m00` still renders nothing. Header/material fixes are not sufficient for `m00`.
- [ ] If `m00` is confirmed non-renderable, add a converter warning or remap option to avoid binding new meshes to `m00`.

## 2. Fix Remaining Mesh Import Issues

- [ ] Fix DAE skin import for cases where preview already shows broken geometry.
  - Use `bind_shape_matrix`, `INV_BIND_MATRIX`, and `vertex_weights` instead of only visual-scene node matrices.
  - Re-test result: `add_multi_pcube1_bind_m01out` now previews correctly and renders correctly in game.
  - Keep this item open until a bind-matrix-specific test confirms whether the fix was structural or incidental.
- [ ] Fix game-side black texture/material rendering.
  - `add_multi_pcube1_bind_m07out` previews correctly, including UVs, but renders black in game.
  - Compare generated GE state with native textured draw calls.
  - Prefer cloning a native textured GE state block and replacing only VADDR/VTYPE/PRIM/vertex data.
  - IDA audit note:
    - `section+0x74` selects a 36-byte material entry.
    - Black rendering may mean material index 0 is not the intended textured material for the added mesh, or generated GE state is still missing texture setup.
    - `sub_88BCE3C` stores `section+0x100` as the display-list pointer when `section+0x70 == 0`; the game executes PMF2 display-list bytes directly.
- [ ] Keep large-mesh safety checks.
  - Ensure generated `PRIM` commands never exceed the 16-bit vertex count limit.
  - Converter now splits large generated triangle draws into multiple `PRIM` commands.
  - Keep testing large meshes like `add_high_pcube1_bind_m00out`.
- [x] Keep duplicate-face filtering for Maya-combined meshes.
  - Avoid re-adding faces already present in the template PMF2.
- [ ] Build a small regression matrix:
  - `add_high_pcube1_bind_m00out`
  - `add_multi_pcube1_bind_m00out`: preview OK, game renders nothing.
  - `add_multi_pcube1_bind_m01out`: preview OK, game OK.
  - `add_multi_pcube1_bind_m07out`
  - `add_pcube1_bind_m11out`


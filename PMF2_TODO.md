# PMF2 / DAE Mesh Import TODO

## 1. Investigate `*_m00out` Mesh Not Rendering

- [x] Verify whether `m00` is a root/control bone that the game intentionally skips for rendering.
  - Latest test: `add_multi_pcube1_bind_m00out` now previews correctly, but the game renders nothing.
  - Binary comparison confirmed the `m00` PMF2 section can contain valid `section+0x70 == 0`, `section+0x100` display-list bytes, and a valid `PRIM`, matching visible `m01`/`m07` generated sections.
  - IDA confirmed the real blocker is a runtime draw-mask table: `word_8A17F10[0] == 0x0002` means section index 0 traverses children but does not enqueue draw, while `m01`/`m07`/`m11` entries are `0x0003` and do draw.
  - Full notes: `PMF2_M00_RENDER_ANALYSIS.md`.
  - Related special-section notes, including `pl0a_o05`: `PMF2_SPECIAL_SECTIONS_ANALYSIS.md`.
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
- [x] If `m00` is confirmed non-renderable, add a converter warning or remap option to avoid binding new meshes to `m00`.
  - Converter now warns when patching additional mesh faces onto a root/control-looking `m00` section.
  - Future improvement: add a remap option such as `m00 -> m01` for imported meshes that target the non-drawing root section.

## 2. Fix Remaining Mesh Import Issues

- [x] Fix DAE skin import for cases where preview already shows broken geometry.
  - Use `vertex_weights` to select the target joint, apply `bind_shape_matrix` to skin geometry, and avoid applying `INV_BIND_MATRIX` a second time when converting to PMF2 bone-local vertices.
  - Re-test result: `add_multi_pcube1_bind_m01out` now previews correctly and renders correctly in game.
  - Regression now verifies `bind_shape_matrix` can affect imported geometry while `INV_BIND_MATRIX` is not double-applied to local vertices.
- [x] Fix game-side black texture/material rendering.
  - `add_multi_pcube1_bind_m07out` previews correctly, including UVs, but renders black in game.
  - Compare generated GE state with native textured draw calls.
  - Prefer cloning a native textured GE state block and replacing only VADDR/VTYPE/PRIM/vertex data.
  - IDA audit note:
    - `section+0x74` selects a 36-byte material entry.
    - Black rendering may mean material index 0 is not the intended textured material for the added mesh, or generated GE state is still missing texture setup.
    - `sub_88BCE3C` stores `section+0x100` as the display-list pointer when `section+0x70 == 0`; the game executes PMF2 display-list bytes directly.
  - Probe found appended draws could land after post-PRIM cleanup state; converter now inserts generated draws immediately after the last native `PRIM`, before cleanup/RET.
- [ ] Keep large-mesh safety checks.
  - Ensure generated `PRIM` commands never exceed the 16-bit vertex count limit.
  - Converter now splits large generated triangle draws into multiple `PRIM` commands.
  - Keep testing large meshes like `add_high_pcube1_bind_m00out`.
- [x] Keep duplicate-face filtering for Maya-combined meshes.
  - Avoid re-adding faces already present in the template PMF2.
- [x] Build a small regression matrix:
  - `add_high_pcube1_bind_m00out`
  - `add_multi_pcube1_bind_m00out`: preview OK, game renders nothing.
  - `add_multi_pcube1_bind_m01out`: preview OK, game OK.
  - `add_multi_pcube1_bind_m07out`
  - `add_pcube1_bind_m11out`
  - Automated coverage now includes bind-matrix import, m00 policy detection, large PRIM splitting, and GE insertion before cleanup state.

## 3. Known Special Section Caveats

- [x] Record runtime draw-mask findings for non-standard sections.
  - `word_8A17F10[0] == 0x0002`: `pl0a_m00` traverses children but does not draw.
  - `word_8A17F10[24] == 0x0000`: `pl0a_o05` is not drawn/traversed by the same main render path.
  - `testout.dae -> test.pmf2` showed preview can render appended `pl0a_o05` geometry, but game loading/rendering may fail because `o05` is not a normal drawable target.
  - Repacking this case also warned that stream 0 compressed size exceeded the original chunk (`55653 > 50944`), so PZZ/AFS layout should be checked when game loading hangs.
  - Full notes: `PMF2_SPECIAL_SECTIONS_ANALYSIS.md`.


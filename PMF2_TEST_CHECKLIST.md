# PMF2 Mesh Import Test Checklist

Use this checklist after changing DAE import, PMF2 mesh patching, GE display-list emission, material handling, or save/repack logic.

## Test Inputs

Run the checklist against these outputs:

- `add_high_pcube1_bind_m00out`
- `add_multi_pcube1_bind_m00out`
- `add_multi_pcube1_bind_m01out`
- `add_multi_pcube1_bind_m07out`
- `add_pcube1_bind_m11out`

Also use `testout.dae -> test.pmf2` when validating special-section behavior such as `pl0a_o05`.

## Before Testing

- [ ] Build or run the latest converter binary from the current workspace.
- [ ] Confirm the generated PMF2/PZZ output path for each sample.
- [ ] Keep a known-good original `Z_DATA.BIN` or source PZZ available for comparison.
- [ ] Use a clean emulator/game boot for each in-game test when possible.
- [ ] Record converter console output, especially `[patch-mesh]` warnings.
- [ ] If the output is repacked into a PZZ, record any compressed chunk warning such as `compressed size exceeds original chunk`.

## Common Converter Checks

For each sample:

- [ ] Import the DAE using template PMF2 patch mode with mesh patching enabled.
- [ ] Confirm the converter completes without panic or error.
- [ ] Confirm no `PRIM` count warning or overflow occurs.
- [ ] If the sample adds geometry to an existing section, confirm the log says it appended GE and preserved the template mesh.
- [ ] If the sample enables a previously no-mesh section, confirm the log clearly reports rebuilt GE for that section.
- [ ] Save/repack the output and confirm the output file is written.

## Common Preview Checks

For each sample in the tool preview:

- [ ] The model loads without crashing the preview.
- [ ] The added cube/mesh is visible when its target bone is enabled.
- [ ] Existing template geometry is still visible.
- [ ] The added mesh is attached to the expected bone transform.
- [ ] The mesh is not wildly scaled, mirrored unexpectedly, or offset from the intended bone.
- [ ] Normals look sane under preview lighting.
- [ ] UVs appear present for textured samples.
- [ ] Bone visibility controls still list the expected bones.

## Common Game Checks

For each sample in game:

- [ ] Boot the game with the patched asset.
- [ ] Load the target character/model.
- [ ] Move the camera or character enough to see the target region.
- [ ] Verify the game does not hang or crash during model load.
- [ ] Verify existing model parts still render.
- [ ] Verify animations still run.
- [ ] Compare against the preview result and record any mismatch.
- [ ] If the game hangs during loading, check both PMF2 section renderability and PZZ/AFS repack size warnings before assuming the preview is wrong.

## Sample Matrix

### `add_high_pcube1_bind_m00out`

Purpose: large-mesh safety and root/control `m00` policy.

- [ ] Converter emits the root/control `m00` warning.
- [ ] Generated draw calls are split so no `PRIM` command exceeds the 16-bit vertex count limit.
- [ ] Preview loads and shows the added high-density mesh.
- [ ] Game loads without crashing.
- [ ] Expected game result: mesh bound to `m00` does not render, because game code marks section index 0 as traverse-only (`word_8A17F10[0] == 0x0002`).
- [ ] If it renders, record the exact game state and camera/animation because that contradicts the current IDA-backed root/control policy.

### `add_multi_pcube1_bind_m00out`

Purpose: confirm known `m00` behavior after the parser and GE fixes.

- [ ] Converter emits the root/control `m00` warning.
- [ ] Preview shows the added cube correctly.
- [ ] Game loads without crashing.
- [ ] Expected game result: added cube does not render in game.
- [ ] Confirm this is not a PMF2 binary issue: generated `m00` can still have `section+0x70 == 0`, valid display-list bytes at `section+0x100`, and a valid `PRIM`.
- [ ] Confirm the reason is runtime traversal policy: section index 0 is traverse-only, while renderable sections such as `m01`, `m07`, and `m11` are draw+traverse.
- [ ] Existing model geometry still renders normally.
- [ ] If the cube renders, compare the output against earlier `probe_multi_m00_hdr.pmf2` and update `PMF2_TODO.md` plus `PMF2_M00_RENDER_ANALYSIS.md`.

### `add_multi_pcube1_bind_m01out`

Purpose: positive control for DAE skin import and renderable bone binding.

- [ ] Converter does not emit the root/control `m00` warning.
- [ ] Preview shows the added cube on `m01`.
- [ ] Game shows the added cube on `m01`.
- [ ] Cube position matches preview closely.
- [ ] Cube follows animation or transform changes for the target part.
- [ ] Existing geometry is not duplicated beyond the intended added faces.

### `add_multi_pcube1_bind_m07out`

Purpose: textured/material display-list state regression.

- [ ] Converter appends GE to the existing template section rather than replacing the whole section when possible.
- [ ] Preview shows the added cube and UVs.
- [ ] Game shows the added cube.
- [ ] Expected fixed result: cube is not black if the donor native section has valid textured state.
- [ ] Existing textured geometry in the same section still renders correctly.
- [ ] If the cube is still black, capture the generated PMF2 and compare native commands around the last original `PRIM`, inserted `VADDR`/`VTYPE`/`PRIM`, and cleanup/`RET`.

### `add_pcube1_bind_m11out`

Purpose: another renderable-bone positive control.

- [ ] Converter does not emit the root/control `m00` warning.
- [ ] Preview shows the added cube on `m11`.
- [ ] Game shows the added cube on `m11`.
- [ ] Cube position and orientation match preview closely.
- [ ] Existing model geometry remains intact.
- [ ] No unexpected material or texture regression appears on nearby parts.

### `testout.dae -> test.pmf2`

Purpose: special-section regression for `pl0a_o05` and PZZ size risk.

- [ ] Confirm the input DAE targets `pl0a_o05` or another non-standard ornament/effect section.
- [ ] Preview may show the appended mesh; do not treat preview visibility as proof that the game will render it.
- [ ] Check the target section against `PMF2_SPECIAL_SECTIONS_ANALYSIS.md`.
- [ ] For `pl0a_o05`, expected risk: `word_8A17F10[24] == 0x0000`, so it is not drawn/traversed by the same main render path as `m01`, `m07`, or `m11`.
- [ ] Confirm the generated PMF2 keeps section offsets valid and reaches `RET`/`END` in display-list scans.
- [ ] When repacking to PZZ, record whether stream 0 compressed size exceeds the original chunk.
- [ ] If the game infinitely loads or hangs, test the same added mesh remapped to a known drawable section before debugging PMF2 display-list bytes further.

## Pass Criteria

The regression pass is acceptable when:

- [ ] All non-`m00` samples that preview correctly also render in game.
- [ ] `m00` samples emit a warning and remain non-rendering in game unless `PMF2_M00_RENDER_ANALYSIS.md` is updated with new evidence.
- [ ] Special sections documented in `PMF2_SPECIAL_SECTIONS_ANALYSIS.md` are not counted as normal renderable targets unless their game path is separately proven.
- [ ] `add_multi_pcube1_bind_m07out` no longer renders black when borrowing a valid native textured display-list state.
- [ ] Large mesh output does not exceed PSP GE `PRIM` vertex limits.
- [ ] Existing model geometry and textures are preserved.
- [ ] No game crash, save/repack failure, or preview crash occurs.

## Failure Notes Template

For each failure, record:

- Sample:
- Converter command or GUI steps:
- Converter log:
- Preview result:
- Game result:
- Expected result:
- Output PMF2/PZZ path:
- Screenshot or video path:
- Suspected area: DAE import / mesh binding / GE commands / material index / save-repack / runtime draw-mask policy

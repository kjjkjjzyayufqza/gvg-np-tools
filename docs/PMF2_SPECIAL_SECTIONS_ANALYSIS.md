# PMF2 Special Section Render Policy

## Summary

Not every PMF2 section that contains valid mesh/display-list bytes behaves like a normal body mesh target in game. The game has a hard-coded per-section runtime mask at `word_8A17F10` that controls whether a section is traversed and whether it enqueues draw commands in one confirmed render path.

This means the converter preview can show a mesh that the game may not draw or may load differently, because the preview parses PMF2 data directly while the game applies additional section-index policy and native GE/display-list expectations.

## Runtime Draw Mask

IDA function:

```text
sub_8870BC0(model, section_index, parent_matrix)
  flags = word_8A17F10[section_index]
  if flags & 2:
      compute transform and traverse child/sibling sections
  if flags & 1:
      sub_8981FF8(model, section_index, matrix)  // enqueue draw
```

Observed flag meanings:

```text
0x0000 = neither traverse nor draw in this confirmed render path
0x0002 = traverse only
0x0003 = traverse and draw
```

Important entries:

```text
index 0  / pl0a_m00 = 0x0002  traverse only
index 1  / pl0a_m01 = 0x0003  draw + traverse
index 7  / pl0a_m07 = 0x0003  draw + traverse
index 11 / pl0a_m11 = 0x0003  draw + traverse
index 24 / pl0a_o05 = 0x0000  not drawn/traversed by this path
```

## `m00`

`m00` is the skeleton/root section. It can contain valid PMF2 mesh data, but the game does not enqueue draws for section index `0`.

Expected behavior:

- Preview can show meshes bound to `m00`.
- Game does not render those meshes.
- Header/material/display-list patching is not enough, because the block is the runtime draw mask.

See `PMF2_M00_RENDER_ANALYSIS.md` for the full binary and IDA evidence.

## `o05`

`pl0a_o05` is section index `24`. A new test case used `testout.dae -> test.pmf2` to add geometry under this section.

Observed binary facts:

```text
original pl0a.pzz stream000 / pl0a_o05:
  section index = 24
  size = 8032
  mesh flag = 0
  parent = 2

failing test.pmf2 / pl0a_o05:
  section index = 24
  size = 22160
  mesh flag = 0
  parent = 2
  appended +294 faces
```

A later user test combined the added meshes into one mesh and produced a game-loadable output:

```text
non-hanging test.pmf2 / pl0a_o05:
  section index = 24
  size = 17024
  mesh flag = 0
  parent = 2
  appended draw count = 561 vertices = 187 triangles
```

PMF2 parsing did not show obvious structural damage:

- Section offsets are monotonic.
- Display-list scanning reaches `RET`/`END`.
- `VADDR`/`VTYPE`/`PRIM` are present.
- Preview can render the mesh.

However, IDA shows `word_8A17F10[24] == 0x0000`, so this section is not drawn/traversed by the same confirmed main render traversal used by normal body sections. The later non-hanging result means `o05` should not be classified as absolutely unusable; it is a conditional/special target whose game behavior depends on the exact generated display-list/data shape.

Important algorithm hypothesis:

```text
native o05:
  VTYPE = 0x1142
  uses IADDR + VADDR
  many small indexed TRIANGLE_STRIP PRIM commands

converter appended mesh:
  VTYPE = 0x0142
  unindexed TRIANGLES
  one large PRIM command
```

PSP GE `PRIM` low 16 bits are vertex count, not triangle count:

```text
failing appended draw:
  PRIM param = 0x030372
  type = 3 TRIANGLES
  count = 0x372 = 882 vertices = 294 triangles

non-hanging appended draw:
  PRIM param = 0x030231
  type = 3 TRIANGLES
  count = 0x231 = 561 vertices = 187 triangles
```

This suggests the issue may be the converter's append strategy for native indexed/strip sections, not the section itself. A single large unindexed `TRIANGLES` draw may be too coarse for some native contexts.

Practical conclusion:

- Treat `pl0a_o05` as conditional/special, not as a normal safe body mesh target.
- Preview success on `o05` does not prove game renderability.
- If targeting `o05`, test small/chunked appended PRIM output before assuming the section cannot work.
- Imported meshes targeting `o05` should still offer remapping to a known drawable section unless the user explicitly wants to experiment with this special target.

## Planned Algorithm Experiment

Before making a final policy decision for `o05`, test a converter change:

1. Do not emit one large appended `TRIANGLES` `PRIM` for additional mesh data.
2. Split appended unindexed `TRIANGLES` into smaller `PRIM` batches, for example 60 or 96 vertices per draw.
3. Keep native display-list state preservation unchanged.
4. Generate `testout.dae -> test.pmf2` and test in game.

If the chunked-PRIM version avoids infinite loading, then the root cause is the append draw generation strategy for special/native indexed sections, not `o05` being categorically unusable.

## PZZ Size Risk

The `testout.dae -> test.pmf2` case also increases stream size:

```text
original stream000.pmf2: 130496 bytes
test.pmf2:              144624 bytes
```

When packed into `pl0a.pzz`, the replacement stream exceeded the original compressed chunk:

```text
WARNING: stream 0 compressed size exceeds original chunk (55653 > 50944 bytes)
```

This may be safe only if the save path correctly rebuilds the PZZ descriptor layout and the final AFS entry is accepted by the game. If the game hangs during loading, check both:

- Whether the target section is actually drawable by `word_8A17F10`.
- Whether the repacked PZZ grew beyond assumptions in the game or surrounding AFS entry layout.

## Recommended Converter Policy

- Warn for imports targeting `m00` and conditional/special sections.
- Add a drawable-section validation step using a table derived from `word_8A17F10`.
- Offer remap choices for non-drawable targets:

```text
m00 -> m01
o05 -> m01 / m07 / m11 / user-selected drawable section
```

- Keep preview behavior as-is, but label preview-only or conditional sections so users do not assume game renderability.

## Known Drawable Positive Controls

Use these sections for sanity checks:

- `pl0a_m01`
- `pl0a_m07`
- `pl0a_m11`

These have `word_8A17F10[index] == 0x0003` and have rendered successfully in game after the current PMF2 fixes.

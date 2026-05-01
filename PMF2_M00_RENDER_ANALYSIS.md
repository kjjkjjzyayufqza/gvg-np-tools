# PMF2 `m00` Render Analysis

## Summary

`m00` is not failing because its generated PMF2 mesh bytes are malformed. The generated `m00` section can contain valid mesh metadata and a valid GE display list, but the game runtime treats section index `0` as a root/control section that traverses children without enqueueing its own draw.

Practical result:

- Meshes bound to `m00` can appear correctly in the converter preview.
- The same meshes do not render in game.
- Header/material/display-list patching is not enough to make `m00` render.
- New imported geometry should avoid targeting `m00`; remap to a drawable child such as `m01` when possible.

## PMF2 Binary Evidence

Compared files:

- `converted_out/debug_tmp/probe_multi_m00.pmf2`
- `converted_out/debug_tmp/probe_multi_m00_hdr.pmf2`
- `converted_out/debug_tmp/probe_multi_m01.pmf2`
- `converted_out/debug_tmp/probe_multi_m01_hdr.pmf2`
- `converted_out/debug_tmp/probe_multi_m07.pmf2`
- `converted_out/debug_tmp/probe_multi_m07_hdr.pmf2`
- `converted_out/debug_tmp/probe_high_m00.pmf2`

Important section facts from the generated probes:

```text
probe_multi_m00_hdr.pmf2 / pl0a_m00
index=0 parent=-1 section+0x70=0 section+0x74=0 size=7968
display list: ORIGIN BASE VADDR VTYPE BBOX? PRIM RET
PRIM count: 480

probe_multi_m01_hdr.pmf2 / pl0a_m01
index=1 parent=0 section+0x70=0 section+0x74=0 size=7968
display list: ORIGIN BASE VADDR VTYPE BBOX? PRIM RET
PRIM count: 480

probe_multi_m07_hdr.pmf2 / pl0a_m07
index=7 parent=2 section+0x70=0 section+0x74=0 size=7968
display list: ORIGIN BASE VADDR VTYPE BBOX? PRIM RET
PRIM count: 480
```

The generated `m00` section has the same essential render data shape as renderable generated sections:

- `section+0x70 == 0`, so the PMF2 section is marked as having mesh data.
- `section+0x100` contains display-list bytes.
- The display list includes `VADDR`, `VTYPE`, and `PRIM`.
- The generated `PRIM` count is valid.

This rules out the simple explanations that the converter forgot to set the PMF2 mesh flag, failed to emit display-list bytes, or emitted a malformed basic draw call.

## Header Differences

The meaningful binary/header difference is the role of the section:

```text
pl0a_m00:
  index = 0
  parent = -1
  local matrix = identity
  child/sibling header fields indicate it is the root/control node

pl0a_m01:
  index = 1
  parent = 0
  local matrix has a bone translation

pl0a_m07:
  index = 7
  parent = 2
  local matrix has a bone translation

pl0a_m11:
  index = 11
  parent = 2
  native renderable mesh section
```

`m00` is the root section. It is structurally different from ordinary body sections because it owns the hierarchy root and traversal path.

## IDA Runtime Evidence

IDA database:

```text
D:\PPSSPP\gundam\PSP_GAME\SYSDIR\NPJH50107_gvsgnextpsp.BIN.i64
imagebase: 0x08804000
```

Confirmed PMF2 loading functions:

```text
sub_88BD214(pmf2, index)
  returns pmf2 + *(pmf2 + 0x20 + index * 4)
```

```text
sub_88BD1D4(section, render_entry)
  if (*(section + 0x70) == 0)
      render_entry.material = material_table + 36 * *(section + 0x74)
```

```text
sub_88BCE3C(runtime_model, pmf2, material_table, flags)
  runtime_node.section = section
  runtime_node.mesh_flag = *(section + 0x70)
  runtime_node.parent = *(section + 0x7C)
  if runtime_node.mesh_flag == 0:
      runtime_node.display_list = section + 0x100
```

These functions confirm:

- `section+0x70` controls whether a section has mesh/display-list data.
- `section+0x74` is a material index.
- `section+0x100` is used directly as the runtime display-list pointer.

The decisive render traversal function is:

```text
sub_8870BC0(model, section_index, parent_matrix)
  flags = word_8A17F10[section_index]
  if flags & 2:
      compute child/world transform and traverse child/sibling nodes
  if flags & 1:
      sub_8981FF8(model, section_index, matrix)  // enqueue draw
```

The hard-coded draw mask table contains:

```text
word_8A17F10[0]  = 0x0002  traverse=true, draw=false
word_8A17F10[1]  = 0x0003  traverse=true, draw=true
word_8A17F10[7]  = 0x0003  traverse=true, draw=true
word_8A17F10[11] = 0x0003  traverse=true, draw=true
```

That table explains the observed behavior:

- `m00` at index `0` is traversed so its children can render.
- `m00` is not drawn because bit `1` is clear.
- Renderable sections such as `m01`, `m07`, and `m11` have both traverse and draw bits set.

## Why Header Patching Did Not Work

`probe_multi_m00_hdr.pmf2` forced the generated `m00` section into a renderable-looking state:

- `section+0x70 = 0`
- `section+0x74 = 0`
- valid display-list bytes
- valid `PRIM`

The game still did not render it. IDA explains why: the decision to enqueue a draw is not made only from the PMF2 section header. It is also gated by `word_8A17F10[section_index]`, and index `0` is hard-coded as traverse-only.

## Converter Policy

Current converter behavior:

- Warn when patching additional mesh faces onto a root/control-looking `m00` section.
- Do not keep trying to make `m00` render by changing PMF2 header flags.

Recommended future behavior:

- Add an explicit remap option for imports that target `m00`, for example:

```text
--remap-bone pl0a_m00=pl0a_m01
```

- GUI equivalent: when importing a DAE that binds new geometry to `m00`, offer to remap to the first drawable child section.
- Keep `m00` as the skeleton/root transform node, not a render target.

## Regression Expectations

`add_multi_pcube1_bind_m00out`:

- Preview: added mesh can be visible.
- Game: added mesh should not render.
- Converter: should warn about root/control `m00`.

`add_high_pcube1_bind_m00out`:

- Preview: generated high-density mesh can be visible.
- Game: mesh should not render for the same section-index reason.
- Converter: large mesh `PRIM` splitting should still be valid.

Positive controls:

- `add_multi_pcube1_bind_m01out` should render in game.
- `add_multi_pcube1_bind_m07out` should render in game.
- `add_pcube1_bind_m11out` should render in game.

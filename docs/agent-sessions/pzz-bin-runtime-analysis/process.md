# PZZ BIN Runtime Analysis Process

## Context

Repository: `E:/research/gvg_np`

Target asset folder:

```text
game_assets/z_data/pl00
```

Additional target archive:

```text
W_DATA.BIN
X_DATA.BIN
```

User question:

```text
Use IDA Pro MCP to deeply analyze every BIN inside each model PZZ and determine
how Gundam vs. Gundam Next Plus reads and uses them, including weapon config,
ammo slots, unit stats such as HP/boost/bullet counts, and whether SAD or another
file controls unit logic/input behavior.
```

## Startup Checks

- Read `AGENTS.md`.
- Read `.cursor/rules/gvg-research-tools.mdc`.
- Searched `docs/` for PZZ, BIN, SAD, weapon, ammo, boost, HP, resource, and IDA
  references.
- Read relevant notes:
  - `docs/RX78_RESOURCE_MAPPING_ANALYSIS.md`
  - `docs/EBOOT_ROM_TABLE_IDA_FINDINGS.md`
  - `docs/pl0a_pzz_structure_analysis.md`
  - `docs/PMF2_SPECIAL_SECTIONS_ANALYSIS.md`
  - `docs/PPSSPP_OPERATION_ANALYSIS.md`

## Initial Asset Inventory

`game_assets/z_data/pl00` contains:

```text
stream_000.pmf2 165872
stream_001.gim   66256
stream_002.pmf2  11136
stream_003.gim   17616
stream_004.pmf2    384
stream_005.gim    2320
stream_006.sad  931464
stream_007.bin   19552
stream_008.bin   12800
stream_009.bin     572
stream_010.bin    5008
```

## Working Notes

- Existing notes already confirm that `pl00` maps to Z_DATA entry `1649`, with
  `pl00l` at entry `1726`.
- Existing notes also show six `W_DATA` overlay files for `unit_id = 0`:
  `pl00ov0.bin` through `pl00ov5.bin`.
- The W_DATA overlays are code/data images with magic `MWo3`, so unit-specific
  behavior and input/action logic may be in W_DATA overlays rather than inside
  the model PZZ itself.
- The PZZ `stream_006.sad` is likely animation data. Further checks are needed
  to determine whether it contains only motion timelines or also state/action
  mappings.

## IDA MCP Findings

Active IDA database:

```text
D:/PPSSPP/gundam/PSP_GAME/SYSDIR/NPJH50107_gvsgnextpsp.BIN.i64
```

Confirmed loading chain:

```text
sub_8922D18
  -> sub_89230F0(unit_slot, unit_id, ...)
     -> word_8A6BA6C[2 * unit_id + side_flag]
     -> sub_8886EA4(z_entry, &dword_8D7AF20, 21)

sub_8922D18
  -> word_8A6B8D4[unit_id] + overlay_slot
  -> sub_8886F60(w_data_overlay_entry)
```

For `unit_id = 0`, existing notes and IDA confirm:

```text
Z_DATA 1649 -> pl00.pzz
Z_DATA 1726 -> pl00l.pzz
W_DATA 0..5 -> pl00ov0.bin .. pl00ov5.bin
```

PZZ unpacking:

```text
sub_88866E4 parses the PZZ descriptor count and descriptor table.
sub_88868C4 decompresses descriptor streams and writes stream pointers into
the caller-provided output pointer array.
```

Model setup:

```text
sub_88BCE3C initializes PMF2 render rows from PMF2 sections.
sub_88BD1D4 binds a PMF2 section to a 36-byte texture descriptor by reading
PMF2 section +0x74 as a texture/material index when section +0x70 is zero.
```

Overlay evidence:

```text
LOAD segment 0x9BCF800 starts with MWo3 header for pl00ov0.bin.
Function 0x9BCF880 was defined from overlay code.
It calls sub_89B9DA4 with tables in the overlay data region, writes runtime
unit fields such as a1+30166 and a1+662, then calls sub_88C9C64.

sub_89B9DA4 iterates 24-byte overlay table rows.
sub_88C9C64 consumes a unit runtime pointer at a1+1100 and creates runtime
objects from 48-byte rows plus 4-byte parameter pairs.
```

This proves that W_DATA overlays contain executable unit-specific behavior/data.
They are the best candidate for unit logic, weapon behavior, and input/action
rules.

## `pl00` Stream Classification

```text
stream_000.pmf2: main RX-78-2 model/skeleton. 53 sections.
stream_001.gim : main 256x256 indexed texture atlas.
stream_002.pmf2: effect model set with names plef00_s058..plef00_s066.
stream_003.gim : effect texture.
stream_004.pmf2: small `mn00_mn00` model.
stream_005.gim : small texture paired with stream_004.
stream_006.sad : animation/motion data.
stream_007.bin : pointer/table-heavy integer data, no floats or ASCII.
stream_008.bin : pointer/table-heavy integer data, no floats or ASCII.
stream_009.bin : 13 rows of 44 bytes, all float-heavy tuning data.
stream_010.bin : group table with 31 groups, 61 rows, row size 0x50.
```

`stream_006.sad` header:

```text
magic             = "SAD "
total_size        = 0x000E3680
bone_count        = 0x5D
data_offset       = 0x208
mesh_count/track? = 0x45
table_offset      = 0x37C
file_size         = 0x000E3688
```

`stream_010.bin` structure:

```text
u16 group_count = 31
u16 row_count   = 61
31 entries of { u16 offset, u16 count }
data starts at 0x80
row size = 0x50
```

The group offsets point into the row area and sum to 61 rows. Several groups are
empty. This looks like a material/attachment/effect binding table, not a unit
stat table.

`stream_009.bin` structure:

```text
13 rows * 44 bytes = 572 bytes
row format appears to be 11 little-endian f32 values
```

Rows contain values such as distances, angles, scales, and timing-like values:

```text
-0.18, -0.09, -4.8, 0.03, 0.28, 0.75, 0.9
40, 50, 350
10, 20, 30
360, 120, 560
```

This is more consistent with per-model/effect/camera/lock-on tuning than HP or
ammo counts.

## Current Interpretation

- HP, boost, ammo counts, and input-to-action behavior are not proven to live in
  the PZZ BIN streams.
- The executable explicitly loads six W_DATA overlays per unit. `pl00ov0.bin`
  is executable MIPS code plus dense tables.
- The currently visible overlay code writes runtime unit fields and registers
  action/event rows. This makes W_DATA overlays the most likely location for
  control logic and unit-specific weapon behavior.
- PZZ unknown streams still matter. They are likely companion data consumed by
  model/animation/effect systems:
  - `stream_007.bin` and `stream_008.bin`: integer offset/state/index tables.
  - `stream_009.bin`: float tuning table.
  - `stream_010.bin`: grouped 0x50-byte rows, likely material/attachment/effect
    binding.

## Commands And Outcomes

```text
Get-Content AGENTS.md
Get-Content .cursor/rules/gvg-research-tools.mdc
git grep -n -i -E "pzz|\\.bin|sad|weapon|ammo|boost|hp|pl00|..."
Get-ChildItem game_assets/z_data/pl00 -Recurse -File
```

Outcome: startup rules and relevant docs were read; `pl00` contains 11 streams.

```text
IDA MCP survey_binary
IDA MCP analyze_batch on 0x88866E4, 0x88868C4, 0x88BCE3C, 0x88BD1D4,
0x89230F0, 0x8922D18, 0x8923414, 0x892348C, 0x8923684, 0x8886EA4,
0x8886F60, 0x8922180, 0x8922744, 0x89244F0, 0x89B9DA4, 0x88C9C64
```

Outcome: confirmed Z_DATA model PZZ load path, W_DATA overlay load path, PMF2
render setup path, and executable overlay behavior evidence.

```text
Python binary inspection over game_assets/z_data/pl00 streams
```

Outcome: decoded key structural facts for SAD and BIN streams. No assets were
modified.

```text
python docs/afs_inventory.py W_DATA.BIN --out game_assets/w_data_inventory.json
python docs/extract_afs_all.py W_DATA.BIN game_assets/w_data/all
```

Outcome: extracted 480 W_DATA entries to `game_assets/w_data/all` and wrote an
inventory file to `game_assets/w_data_inventory.json`.

Copied the first six W_DATA entries to:

```text
game_assets/w_data/pl00/pl00ov0.bin
game_assets/w_data/pl00/pl00ov1.bin
game_assets/w_data/pl00/pl00ov2.bin
game_assets/w_data/pl00/pl00ov3.bin
game_assets/w_data/pl00/pl00ov4.bin
game_assets/w_data/pl00/pl00ov5.bin
```

Static overlay scan:

```text
pl00ov0.bin..pl00ov5.bin
  size        0x8D00
  magic       MWo3
  section A   0x40..0x5C78
  section B   0x5C78..0x8D00
  many MIPS prologues, internal pointers, and external jal targets
```

IDA findings from `pl00ov0.bin` loaded at `0x09BCF800`:

```text
0x09BCF880 sub_9BCF880
  registers two 24-byte action/event tables at 0x09BD61B8 and 0x09BD6200.

0x09BCFD18 sub_9BCFD18
  is a PL00 weapon/action dispatcher.
  It calls sub_89B730C before weapon actions and dispatches action/effect
  handlers through sub_89F78A0 and sub_8869794/sub_8869708.

0x09BD0038 sub_9BD0038
  dispatches through dword_9BD6070[*(char *)(unit+2738)+116].

0x09BD0278 sub_9BD0278
  dispatches through dword_9BD6070[*(char *)(unit+2738)+212].
```

Confirmed generic runtime fields:

```text
unit+592  max HP float
unit+596  current HP float
unit+600  previous/display HP float

unit+1616 + slot*20  regular ammo slot array
unit+1936            special/shared ammo slot
unit+2256..2287      state byte banks used by overlay action tables
unit+2738            state dispatch index used by PL00 overlay
```

Confirmed generic helper functions:

```text
sub_88A8844   applies HP damage and clamps current HP to max HP
sub_89B6FE0   initializes a 20-byte ammo slot from a definition row
sub_89B70A8   initializes all regular ammo slots
sub_89B730C   checks and consumes ammo
sub_89B7484   checks whether a slot is full
sub_89B7968   restores or refills ammo
sub_89B75C0   advances ammo reload timers
sub_89BAD98   maps encoded state IDs to unit+2256/2264/2272/2280 banks
```

Detailed findings were written to:

```text
docs/PL00_W_DATA_BEHAVIOR_ANALYSIS.md
```

## X_DATA Audio Pass

The root-level `X_DATA.BIN` archive was inventoried and extracted:

```text
python docs/afs_inventory.py X_DATA.BIN --out game_assets/x_data_inventory.json
python docs/extract_afs_all.py X_DATA.BIN game_assets/x_data/all
```

Outcome:

```text
X_DATA.BIN size       624,109,568 bytes
entry count           14,623
name table offset     623,407,104
name table size       701,904
name table entry size 48
extracted folder      game_assets/x_data/all
```

All extracted entries are `.adx` files. The first archive ranges are:

```text
0000..0055  bgm*.adx
0056..0111  s_bgm*.adx
0112..0353  v00_*.adx
0354..0393  v00d_*.adx
0394..0396  v00p_*.adx
0397..0572  v01_*.adx
```

RX-78-2 maps to `unit_id = 0`, so the high-confidence RX-78-2 X_DATA voice
families are:

```text
v00   242 files, indices 112..353
v00d   40 files, indices 354..393
v00p    3 files, indices 394..396
```

The RX-78-2 audio subset was copied to:

```text
game_assets/x_data/rx78_v00
```

The generated manifest is:

```text
game_assets/x_data/rx78_v00_manifest.csv
```

Representative ADX header checks confirmed mono 22 kHz voice clips:

```text
0112_v00_0000.adx   1.013 seconds
0217_v00_1900.adx   7.747 seconds
0354_v00d_9401.adx  1.556 seconds
0394_v00p_a000.adx  5.888 seconds
```

IDA MCP checks:

```text
find_regex("ADX|adx|Cri|CRI|voice|se_|bgm|X_DATA|snd|sound")
xrefs_to(0x8A1BCFC, 0x8ACDFF8)
analyze_batch(0x8883C40)
analyze_batch(0x89BC0AC)
```

Findings:

```text
off_8A1BD2C registers X_DATA.BIN with count 0x391F.
0x391F equals 14,623, matching the extracted AFS inventory.
sub_8883C40 initializes X/Y/Z/W archive containers from off_8A1BD2C.
sub_89BC0AC is a sound/archive initialization path that waits for X_DATA.BIN.
```

No obvious executable format string such as `v%02d_*.adx` was found. The game
likely plays X_DATA audio by archive index, cue ID, or a table generated from
the AFS name table.

Detailed X_DATA findings were written to:

```text
docs/RX78_X_DATA_AUDIO_MAPPING.md
```

Current X_DATA conclusion:

```text
X_DATA.BIN is audio-only for this task.
RX-78-2 uses v00, v00d, and v00p ADX voice files.
HP, boost, ammo, weapon behavior, and input state machine data are not in
X_DATA.BIN.
```

## Next Reverse-Engineering Steps

1. Define more overlay functions in IDA from `LOAD` and annotate their table
   rows and runtime unit struct offsets.
2. Trace the boost meter from the UI draw routine or PPSSPP runtime memory to
   identify the exact current/max boost fields.
3. Correlate overlay table row IDs with button/action state by tracing calls
   into `sub_89B9D48`, `sub_89B9DA4`, and weapon/effect creation functions.
4. Compare `stream_007.bin`, `stream_008.bin`, and `stream_010.bin` across
   multiple `plXX` units after extracting more model PZZ folders. Cross-unit
   deltas will separate generic schema fields from per-unit weapon parameters.
5. If voice-event labels are needed, trace runtime playback calls from the sound
   system to X_DATA archive indices, then map those indices back to
   `game_assets/x_data/rx78_v00_manifest.csv`.

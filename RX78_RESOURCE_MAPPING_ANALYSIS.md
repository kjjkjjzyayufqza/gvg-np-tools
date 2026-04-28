# RX-78-2 Resource Mapping Analysis

This document records the current findings about how RX-78-2 / `pl00` resources are loaded in Gundam VS Gundam Next Plus.

## Summary

RX-78-2 is not loaded only from `Z_DATA.BIN/pl00.pzz`.

The current IDA analysis shows that unit `0` maps to both:

- `Z_DATA.BIN` entries for the main model package.
- `W_DATA.BIN` entries for six `pl00` overlay resources.

For `unit_id = 0`:

| Container | Index | Name | Notes |
|---|---:|---|---|
| `Z_DATA.BIN` | `1649` | `pl00.pzz` | Primary model package |
| `Z_DATA.BIN` | `1726` | `pl00l.pzz` | Alternate/secondary model package |
| `W_DATA.BIN` | `0` | `pl00ov0.bin` | Overlay slot 0 |
| `W_DATA.BIN` | `1` | `pl00ov1.bin` | Overlay slot 1 |
| `W_DATA.BIN` | `2` | `pl00ov2.bin` | Overlay slot 2 |
| `W_DATA.BIN` | `3` | `pl00ov3.bin` | Overlay slot 3 |
| `W_DATA.BIN` | `4` | `pl00ov4.bin` | Overlay slot 4 |
| `W_DATA.BIN` | `5` | `pl00ov5.bin` | Overlay slot 5 |

The generated mapping table is stored in:

- `unit_resource_mapping.json`
- `unit_resource_mapping_raw.json`

## DATA Container Table

IDA shows a container table around `0x8A1BD2C`.

The four game data containers are:

| Container | Entry Count | Notes |
|---|---:|---|
| `X_DATA.BIN` | `14623` | Mostly `.adx` audio |
| `Y_DATA.BIN` | `22` | Mostly `.sfd` video |
| `Z_DATA.BIN` | `2651` | Models, PZZ, BGM refs, etc. |
| `W_DATA.BIN` | `480` | Unit overlay resources |

The counts match the generated AFS inventories in `_ppsspp_inv/`.

## Z_DATA Unit Mapping

IDA function `sub_89230F0` uses a unit-to-`Z_DATA` table.

Observed expression:

```text
word_8A6BA6C[2 * unit_id + side_flag]
```

For `unit_id = 0`:

```text
word_8A6BA6C[0] = 0x0671 = 1649 = pl00.pzz
word_8A6BA6C[1] = 0x06BE = 1726 = pl00l.pzz
```

This explains why `Z_DATA.BIN` contains both `pl00.pzz` and `pl00l.pzz`.

## W_DATA Overlay Mapping

IDA function `sub_8922D18` uses a unit-to-overlay table.

Observed expression:

```text
word_8A6B8D4[unit_id] + slot
```

For `unit_id = 0`:

```text
word_8A6B8D4[0] = 0
slot 0..5 -> W_DATA entries 0..5
```

Those entries are:

```text
0 -> pl00ov0.bin
1 -> pl00ov1.bin
2 -> pl00ov2.bin
3 -> pl00ov3.bin
4 -> pl00ov4.bin
5 -> pl00ov5.bin
```

The `W_DATA` files have the magic:

```text
MWo3
```

Example extracted files are under:

```text
converted_out/W_pl00ov/
```

## GIM / PZZ Experiments

Several controlled texture experiments were performed on `Z_DATA.BIN/pl00.pzz`.

### `stream001.gim`

Format:

```text
MIG.00.1PSP
format = 5
pixel_order = 1
width = 256
height = 256
palette format = 1
palette size = 256x1
```

Results:

- Full zero/black image caused infinite loading.
- Palette-only all-black also caused infinite loading.
- A clean 1-bit palette modification did not crash.
- A higher-frequency palette entry changed to black did not crash, but no visible change was observed.

Important lesson:

Changing `stream001.gim` is safe only when done carefully, but it does not appear to affect the visible RX-78-2 body in the tested scene.

Later visual inspection confirmed that `converted_out/1649_pl00/stream001.png` is indeed a proper unfolded 256x256 model texture atlas. It contains recognizable RX-78-2 body parts and colors.

Because earlier marker tests did not show a visible change in-game, the current interpretation is more specific:

- `stream001.gim` is a real texture atlas.
- The current in-game RX-78-2 render path may not be sampling this specific atlas.
- Possible reasons:
  - Runtime selected `pl00l.pzz` instead of `pl00.pzz`.
  - Another material/texture slot overrides this atlas.
  - The tested scene uses a different model variant or LOD.
  - PMF2 mesh/material data may bind a different stream/resource for the visible body.

New test written to `Z_DATA.BIN`:

```text
1649_pl00 / stream001.gim
palette index 0: 0x8000 -> 0xFC1F
index usage count: 4834 pixels
PZZ size unchanged: 877072
stream001 chunk units unchanged: 386
old comp_len: 49398
new comp_len: 49399
```

Generated files:

```text
converted_out/1649_pl00_test_stream001_idx0_magenta/stream001.gim
converted_out/1649_pl00_test_stream001_idx0_magenta/stream001_idx0_magenta.png
converted_out/repacked_pl00_stream001_idx0_magenta_inplace.pzz
```

Follow-up combined test:

Both possible RX-78-2 model packages were patched at the same time:

```text
1649 -> pl00.pzz   -> stream001 palette index 0 marked magenta
1726 -> pl00l.pzz  -> stream001 palette index 0 marked magenta
```

This tests the runtime branch where either `pl00.pzz` or `pl00l.pzz` may be selected.

Result:

```text
No visible in-game change.
No crash.
```

After this result, both `pl00.pzz` and `pl00l.pzz` were restored to their original versions in `Z_DATA.BIN`.

This strongly suggests that the visible RX-78-2 render path is not sampling `stream001.gim` from either `pl00.pzz` or `pl00l.pzz`, despite `stream001.png` being a valid unfolded RX-78-2 texture atlas.

## PMF2 Display List Texture Scan

The true PMF2 display lists start at command word:

```text
0x14000000
```

Scanning only those real display lists in:

```text
1649_pl00/stream000.pmf2
1726_pl00l/stream000.pmf2
1649_pl00/stream002.pmf2
1726_pl00l/stream002.pmf2
```

shows draw commands and vertex setup commands:

```text
0x01 VADDR
0x02 IADDR
0x04 PRIM
0x10 BASE
0x12 VERTEXTYPE
0x14 ORIGIN
0x0B RET
0x9B frequent per-primitive command
```

No obvious texture address / CLUT / texture mode commands are present in these PMF2 display lists.

This implies texture binding is probably handled outside the PMF2 display list, likely by the engine before draw submission, using separate per-section/material tables.

The unknown PZZ streams are therefore important:

```text
stream007.bin
stream008.bin
stream009.bin
stream010.bin
```

Current observations:

- `stream009.bin` is mostly float parameters.
- `stream010.bin` begins with a table:

```text
u16 group_count = 31
u16 row_count   = 61
then group_count entries of:
  u16 offset
  u16 count
```

Rows appear to be `0x50` bytes each.

`stream010.bin` is identical between `pl00.pzz` and `pl00l.pzz`, and is a strong candidate for a material/section binding table.

More work is needed to determine which field, if any, selects texture stream or external texture resource.

## Confirmed PMF2 Texture Binding Fields

The PMF2 section structure contains important fields around offsets `0x70..0x7C`.

In model setup function `sub_88BCE3C`, each 144-byte render section row is initialized from one PMF2 section:

```text
render_row + 12  = pmf2_section_ptr
render_row + 128 = section_index
render_row + 136 = *(pmf2_section + 112)
render_row + 130 = *(pmf2_section + 124)
render_row + 132 = -1
render_row + 134 = -1
render_row + 140 = pmf2_section + 256 if *(pmf2_section + 112) == 0
```

Function `sub_88BD1D4` links a render section row to a texture descriptor:

```text
if (*(pmf2_section + 112) == 0) {
    render_row + 4 = model_texture_descriptor_array
                   + 36 * *(pmf2_section + 116)
}
```

For both:

```text
1649_pl00/stream000.pmf2
1726_pl00l/stream000.pmf2
```

all real display-list sections have:

```text
*(section + 112) = 0
*(section + 116) = 0
```

Therefore all visible model sections bind to texture descriptor `0`, which comes from `stream001.gim` in the corresponding PZZ package.

This confirms:

```text
pl00.pzz  stream000.pmf2 -> stream001.gim
pl00l.pzz stream000.pmf2 -> stream001.gim
```

as the intended texture binding.

## UV Sampling Experiment

The PMF2 display list and vertex data were decoded offline.

For each draw call:

- VTYPE was decoded.
- Indexed and non-indexed vertices were resolved.
- UV coordinates were decoded.
- Palette indices were sampled from `stream001.gim`.

Triangle-center sampling showed the following most common sampled palette indices:

```text
1649_pl00:
0, 255, 43, 14, 148, 34, 128, 8, 52, 116,
1, 75, 24, 100, 30, 254, 4, 59, 199, 25,
6, 228, 19, 50, 35, 145, 44, 23, 233, 164

1726_pl00l:
0, 255, 20, 254, 82, 52, 239, 183, 8, 236,
201, 194, 249, 158, 42, 104, 11, 148, 145, 252,
27, 120, 125, 242, 3, 180, 16, 1, 24, 31
```

A strong marker test was then built:

```text
For both pl00.pzz and pl00l.pzz:
  stream001.gim
  top 30 triangle-center sampled palette indices -> 0xFC1F (magenta)
```

The replacement was in-place:

```text
pl00:
  old comp_len = 49398
  new comp_len = 49376
  PZZ size = 877072 unchanged

pl00l:
  old comp_len = 50243
  new comp_len = 50221
  PZZ size = 861968 unchanged
```

Generated files:

```text
converted_out/1649_pl00_test_stream001_top30_magenta/stream001.gim
converted_out/1649_pl00_test_stream001_top30_magenta/stream001_top30_magenta.png
converted_out/repacked_pl00_stream001_top30_magenta_inplace.pzz

converted_out/1726_pl00l_test_stream001_top30_magenta/stream001.gim
converted_out/1726_pl00l_test_stream001_top30_magenta/stream001_top30_magenta.png
converted_out/repacked_pl00l_stream001_top30_magenta_inplace.pzz
```

These two packages were patched into `Z_DATA.BIN` simultaneously for testing.

Result:

```text
No visible in-game change.
No crash.
```

Verification after patching:

```text
D:\PPSSPP\gundam\PSP_GAME\USRDIR\Z_DATA.BIN

entry 1649 pl00.pzz:
  SHA1 in Z_DATA == repacked_pl00_stream001_top30_magenta_inplace.pzz

entry 1726 pl00l.pzz:
  SHA1 in Z_DATA == repacked_pl00l_stream001_top30_magenta_inplace.pzz
```

Therefore the patch was written correctly. The lack of visual change is not caused by a failed `patch-afs` write.

## PPSSPP Extracted ISO Warning

PPSSPP displays:

```text
extracted isos often don't work
```

This does not automatically mean resource changes are ignored, but it is now an important risk factor.

Since the patched `Z_DATA.BIN` was verified on disk, but the game still shows no visible change, possible explanations are:

1. PPSSPP is not actually booting from `D:\PPSSPP\gundam\PSP_GAME\USRDIR\Z_DATA.BIN`.
2. PPSSPP is booting another ISO/CSO copy from recent games or a different path.
3. Extracted-folder mode is causing different file access behavior than a real ISO.
4. The current screen/model is using another resource path despite the `unit_id=0` mapping.

Recommended validation:

Patch a known full-screen image resource such as `pbg000.pzz` or `pbg001.pzz` with an obvious marker and check whether the visible loading/UI image changes.

If a known full-screen image does not change either, the emulator is almost certainly not reading this modified `Z_DATA.BIN`.

If the full-screen image changes, then the file path is correct and the remaining issue is specifically the model texture binding/render path.

## Render Queue / Texture Binding Architecture

The model draw path has now been traced deeper:

```text
PMF2 section
  -> render section row (144 bytes)
  -> draw job (128 bytes)
  -> render queue
  -> GE command emission
```

Important functions:

```text
sub_88BCE3C  initializes render section rows from PMF2 sections
sub_88BD1D4  links render section rows to GIM texture descriptors
sub_8981FF8  creates normal model draw jobs
sub_8982260  creates alternate/model effect draw jobs
sub_8982D10  allocates draw job memory
sub_8982B38  queues draw jobs
sub_8982BB4  consumes queued jobs by top-nibble job type
sub_89841E8  emits normal textured draw GE commands
sub_8984470  emits alternate textured draw GE commands
sub_89BE7D4  emits texture / CLUT GE setup
sub_89BED1C  emits display list call/address command
```

The critical texture setup function is `sub_89BE7D4`.

Its input is a 36-byte GIM texture descriptor created by `sub_88BD230`.

It emits PSP GE commands corresponding to:

```text
texture enable / texture mode
pixel format
texture image address
texture width/height
CLUT mode and CLUT address for indexed textures
```

For model draw jobs, the path is:

```text
sub_8981FF8:
  job + 8 = render_row

sub_89841E8:
  render_row = *(job + 8)
  texture_descriptor = *(render_row + 4)
  display_list = *(render_row + 140)

  if texture_descriptor exists:
      sub_89BE7D4(texture_descriptor, ...)

  sub_89BED1C(display_list)
```

This confirms that if the game is drawing the `pl00/pl00l` PMF2 sections normally, and if the patched `stream001.gim` is what was loaded into texture descriptor 0, the magenta marker should be visible.

## Current Contradiction

We now have three facts that do not fit together:

1. Static resource mapping says RX-78-2 uses:

```text
pl00.pzz  / pl00l.pzz
```

2. PMF2 render sections in both packages bind to texture descriptor `0`, which is `stream001.gim`.

3. The game shows no visible change even after changing the top 30 actually UV-sampled palette indices in both `pl00` and `pl00l`.

The patch was verified on disk:

```text
SHA1 in Z_DATA == SHA1 of repacked marker PZZ
```

So the remaining explanations are:

- The current in-game unit is not using `unit_id = 0` resources despite visually looking like RX-78-2.
- The game is not reading the modified extracted-folder `Z_DATA.BIN` at runtime.
- Runtime code replaces or recolors the texture after loading, using another source.
- The actual visible model is drawn through another render path, not the normal `sub_8981FF8`/`sub_89841E8` path.

The first two explanations are currently the strongest, because the static render path says the marker should have appeared.

## Nu Gundam Correction

The user identified Nu Gundam as:

```text
1736 -> pl0al.pzz
```

This corrects the earlier mistaken focus on `pl00/pl00l`.

Further inspection shows that `pl0al.pzz` is the alternate/`l` package. The matching primary package is:

```text
1659 -> pl0a.pzz
1736 -> pl0al.pzz
```

`pl0al.pzz` stream layout:

```text
stream000.pmf2  94512 bytes
stream001.gim   66256 bytes
stream002.pmf2  23984 bytes
stream003.gim   17616 bytes
stream004.pmf2  384 bytes
stream005.gim   2320 bytes
stream006.sad   755368 bytes
stream007.bin   14048 bytes
stream008.bin   12736 bytes
stream009.bin   572 bytes
stream010.bin   6224 bytes
```

`stream001.gim` is a 256x256 indexed GIM:

```text
format = 5
pixel_order = 1
palette format = 1
palette = 256x1
```

A preview was copied to:

```text
nu_pl0al_stream001.png
```

It visually matches Nu Gundam texture parts.

### Nu Gundam Marker Test

For `pl0al.pzz`, PMF2 display list + UV triangle-center sampling found top sampled palette indices:

```text
0, 255, 35, 1, 25, 62, 212, 21, 43, 15,
31, 5, 237, 27, 174, 34, 41, 3, 238, 76,
44, 182, 69, 138, 64, 188, 184, 11, 181, 190
```

These indices were changed to magenta:

```text
0xFC1F
```

Replacement was in-place:

```text
old comp_len = 53950
new comp_len = 53935
chunk units = 422
chunk capacity = 54016
PZZ size = 609680 unchanged
```

Generated files:

```text
converted_out/1736_pl0al_test_stream001_top30_magenta/stream001.gim
converted_out/1736_pl0al_test_stream001_top30_magenta/stream001_top30_magenta.png
converted_out/repacked_pl0al_stream001_top30_magenta_inplace.pzz
```

Patched into:

```text
D:\PPSSPP\gundam\PSP_GAME\USRDIR\Z_DATA.BIN
entry 1736 -> pl0al.pzz
```

### Nu Gundam Primary + Alternate Test

To avoid the same primary/alternate branch issue seen with `pl00/pl00l`, `pl0a.pzz` was also patched.

`pl0a.pzz` stream layout:

```text
stream000.pmf2  130496 bytes
stream001.gim   66256 bytes
stream002.pmf2  50384 bytes
stream003.gim   17616 bytes
stream004.pmf2  384 bytes
stream005.gim   2320 bytes
stream006.sad   755368 bytes
stream007.bin   14048 bytes
stream008.bin   12736 bytes
stream009.bin   572 bytes
stream010.bin   6224 bytes
```

`pl0a/stream001.gim` visually matches the same Nu Gundam atlas.

PMF2 UV triangle-center sampling for `pl0a` found top palette indices:

```text
0, 19, 22, 7, 255, 15, 68, 76, 212, 40,
37, 27, 33, 78, 3, 62, 196, 44, 215, 51,
65, 5, 1, 171, 45, 11, 180, 254, 200, 203
```

These were changed to magenta `0xFC1F`.

Replacement was in-place:

```text
pl0a:
  old comp_len = 53634
  new comp_len = 53627
  chunk units = 420
  chunk capacity = 53760
  PZZ size = 631184 unchanged

pl0al:
  old comp_len = 53950
  new comp_len = 53935
  chunk units = 422
  chunk capacity = 54016
  PZZ size = 609680 unchanged
```

Both were patched into `Z_DATA.BIN`:

```text
1659 -> pl0a.pzz
1736 -> pl0al.pzz
```

Result:

```text
Infinite loading.
```

Both packages were restored to original afterward.

This shows that the broad top-30 palette marker is too invasive for Nu Gundam. It does not mean the index mapping is wrong; it means the game is sensitive to larger CLUT changes in this model package.

### Nu Gundam Minimal Safety Test

After restoring originals, a minimal test was prepared:

```text
1659 -> pl0a.pzz only
stream001.gim
palette index 203: 0x82DF -> 0x82DE
```

This changes only one bit in one palette entry.

Replacement is in-place:

```text
old comp_len = 53634
new comp_len = 53634
chunk units = 420
PZZ size = 631184 unchanged
```

`pl0al.pzz` was restored to original for this test.

Generated file:

```text
converted_out/repacked_pl0a_stream001_idx203_1bit_inplace.pzz
```

Patched into:

```text
D:\PPSSPP\gundam\PSP_GAME\USRDIR\Z_DATA.BIN
entry 1659 -> pl0a.pzz
```

Result:

```text
Infinite loading.
```

After this test, both Nu Gundam packages were restored to original:

```text
1659 -> pl0a.pzz
1736 -> pl0al.pzz
```

## Current State For Next Agent

As of the latest state, the game files have been restored for the Nu Gundam packages:

```text
D:\PPSSPP\gundam\PSP_GAME\USRDIR\Z_DATA.BIN

1659 -> pl0a.pzz   restored from converted_out/1659_pl0a.pzz
1736 -> pl0al.pzz  restored from converted_out/1736_pl0al.pzz
```

The previous `pl00/pl00l` RX-78-2 experiments were also restored earlier:

```text
1649 -> pl00.pzz   restored
1726 -> pl00l.pzz  restored
```

The important generated experiment files remain on disk for analysis:

```text
converted_out/repacked_pl0a_stream001_top30_magenta_inplace.pzz
converted_out/repacked_pl0al_stream001_top30_magenta_inplace.pzz
converted_out/repacked_pl0a_stream001_idx203_1bit_inplace.pzz

converted_out/1659_pl0a_test_stream001_top30_magenta/
converted_out/1736_pl0al_test_stream001_top30_magenta/
converted_out/1659_pl0a_test_stream001_1bit_safe/
```

## Critical Finding: PZZ Tail

For actual loaded Nu Gundam resources, even a one-bit change in `pl0a/stream001.gim` caused infinite loading despite:

```text
PZZ size unchanged
stream chunk units unchanged
stream comp_len unchanged
AFS entry index verified correct
AFS entry content SHA1 verified against generated repacked PZZ
stream001.gim bytes verified against marker GIM
```

This strongly suggests that the issue is not AFS index selection or PZZ stream index selection.

Each relevant PZZ has a 16-byte tail after all stream chunks:

```text
pl0a.pzz:
  tail = 39 be cb 9c d2 3c b6 4a 5c ba ec 27 a1 df 48 b5

pl0al.pzz:
  tail = 54 18 21 6c 2e 20 b6 4a 99 56 d4 39 14 9f 49 b5

pl00.pzz:
  tail = 08 63 19 c3 1b b7 69 a2 eb 15 8d 4c d2 ab 97 5d
```

Current Rust repacking/in-place replacement preserves this tail unchanged.

The tail does not match these simple hashes of the decrypted PZZ body:

```text
MD5
SHA1 first 16 bytes
CRC32
Adler32
```

But its length and behavior strongly suggest it is some kind of PZZ integrity/signature/checksum/footer metadata.

The best next step is to reverse-engineer how this 16-byte tail is generated or checked.

## Recommended Next Investigation

Focus on IDA/PZZ loader code, not texture editing.

Suggested targets:

1. Locate PZZ decompression / stream extraction code in IDA.
2. Search for reads of the final 16 bytes of a PZZ buffer.
3. Search for comparison/memcmp-like logic involving a 16-byte block after zlib chunks.
4. Search for code that processes descriptor entries with flags `0x40000000` and raw tail chunk descriptor `0x399` / `0x30B` style values.
5. Identify whether the 16-byte tail is:
   - checksum over decrypted body,
   - checksum over encrypted body,
   - keyed hash using PZZ key,
   - random/cached metadata,
   - or a relocation/table pointer unrelated to integrity.

Useful facts for the next agent:

```text
pl0a.pzz:
  entry_count = 12
  key = 0x4AB70B80
  stream001 old comp_len = 53634
  one-bit marker comp_len = 53634
  PZZ size = 631184
  tail length = 16

pl0al.pzz:
  entry_count = 12
  key = 0x4AB70B80
  stream001 old comp_len = 53950
  top30 marker comp_len = 53935
  PZZ size = 609680
  tail length = 16
```

Validation command used previously:

```text
Read Z_DATA entry 1659/1736 by AFS table offset+size,
compare SHA1 with generated repacked PZZ,
then decrypt and decompress stream001,
compare stream001 SHA1 with generated marker GIM.
```

Result of that validation:

```text
AFS/PZZ/stream indices were correct.
The modified stream was actually inside Z_DATA.
The game still infinite-loaded when the modified resource was loaded.
```

Therefore the next agent should assume the edit pipeline reaches the correct resource, but the PZZ package is internally invalid after data modification because an unknown tail/check mechanism is not updated.

### `stream003.gim`

Format:

```text
MIG.00.1PSP
format = 5
pixel_order = 1
width = 128
height = 128
palette format = 3
palette size = 256x1
```

Results:

- A high-frequency palette entry was changed to magenta.
- No crash.
- No visible red/magenta change.
- Full palette changed to black.
- No crash.
- No visible black change.

Current inference:

`stream003.gim` is probably not the visible RX-78-2 body texture in the tested scene.

## PZZ Packing Lessons

The original `pl00.pzz` size is:

```text
877072 bytes
```

During testing, one oversized repack temporarily changed the AFS entry size to `877456`. This was later normalized back to `877072`.

For safer experiments:

- Prefer in-place replacement inside the original PZZ layout.
- Keep stream chunks within their original descriptor capacity.
- Avoid expanding the AFS entry unless all related size records are understood.
- Do not assume the game only reads the AFS table size. There may be additional runtime or resource table assumptions.

## Current Working Hypothesis

The visible RX-78-2 body texture is likely not in the tested `pl00.pzz` GIM streams, or those streams are not used for the active visible material state.

The stronger lead is:

```text
W_DATA.BIN/pl00ov0.bin .. pl00ov5.bin
```

These resources are explicitly loaded by the unit resource path for `unit_id = 0`.

## Next Steps

1. Analyze the `MWo3` format used by `pl00ov*.bin`.
2. Identify whether `MWo3` contains texture, material, color, or overlay data.
3. Build a small parser for `MWo3` headers and internal pointers.
4. Try a minimal reversible modification in `pl00ov0.bin`.
5. Patch `W_DATA.BIN` entry `0` only, then test visibility and stability.
6. Continue expanding the mapping table with known unit names if needed.

## Important Files

```text
unit_resource_mapping.json
unit_resource_mapping_raw.json
_ppsspp_inv/X_DATA.BIN.inventory.json
_ppsspp_inv/Y_DATA.BIN.inventory.json
_ppsspp_inv/Z_DATA.BIN.inventory.json
_ppsspp_inv/W_DATA.BIN.inventory.json
converted_out/W_pl00ov/
```

## MWo3 Overlay Findings

The six `pl00ov*.bin` files from `W_DATA.BIN` were extracted to:

```text
converted_out/W_pl00ov/
```

All six files have the same size:

```text
0x8D00 bytes = 36096 bytes
```

The common header layout currently appears to be:

```text
0x00 u32 magic          "MWo3"
0x04 u32 overlay_id     1, 0x51, 0xA1, 0xF1, 0x141, 0x191
0x08 u32 load_base      runtime address of this overlay image
0x0C u32 section1_size  0x5C38
0x10 u32 section2_size  0x3088
0x14 u32 zero
0x18 u32 end_addr       load_base + 0x8D00
0x1C u32 end_addr       load_base + 0x8D00
0x20 char name[]        "pl00ovN.bin"
0x40 section1
0x5C78 section2
0x8D00 end
```

For `pl00ov0.bin`, this is:

```text
load_base      = 0x09BCF800
section1       = file 0x40..0x5C77
section2       = file 0x5C78..0x8CFF
end_addr       = 0x09BD8500
```

Important structural observation:

`section1` contains MIPS code. At file offset `0x80`, the bytes decode as a normal MIPS function prologue:

```text
27bdfff0
afbf000c
afb00008
```

So `MWo3` is not a texture container. It is an overlay memory image containing code plus data.

`section2` looks like dense 16-bit/32-bit data tables, not standard image data. No `MIG.`, `.GIM`, `PMF2`, or `SAD ` magic was found inside the overlay files.

Each overlay contains many absolute pointers back into its own `load_base..load_base+0x8D00` range. For `pl00ov0.bin`, 284 local pointers were detected. This reinforces that `MWo3` is a relocatable or pre-linked runtime overlay blob.

### Code Similarity

The six overlays are structurally almost identical. They differ in only 869 bytes out of 36096.

The differences include:

- Header fields such as overlay id, load base, end address, file name.
- Small scattered data/code immediates.
- Some slot-specific constants.

This suggests the six files are variants/slots of the same unit overlay logic rather than independent texture files.

### Calls From Overlay Code

Scanning MIPS `jal` instructions in `pl00ov*.bin` shows many calls into the main executable and some calls to internal overlay functions.

Common frequently called functions include:

```text
0x89759F0
0x89717AC
0x8869794
0x886BA00
0x8981FF8
0x89B327C
0x89B2540
0x891DBA0
0x886C98C
```

The decompiled functions checked so far look like unit/animation/math/behavior support, not direct texture loading.

### Current Interpretation

`pl00ov0.bin..pl00ov5.bin` are very likely unit-specific runtime overlays for RX-78-2 behavior/state/animation/effect logic.

They are not the visible body texture themselves.

However, they may still reference or request additional resources indirectly. A naive 16-bit scan finds values that numerically match `pbg*` and other `Z_DATA` indices, but many are probably false positives from MIPS immediates or address halves. Proper resource-reference recovery needs instruction-level analysis, not raw halfword scanning.

### Additional Important Finding

`pl00.pzz` is not the only Z package for RX-78-2.

The unit mapping also includes:

```text
1726 -> pl00l.pzz
```

`pl00l.pzz` contains the same stream layout as `pl00.pzz`:

```text
stream000.pmf2
stream001.gim
stream002.pmf2
stream003.gim
stream004.pmf2
stream005.gim
stream006.sad
stream007.bin
stream008.bin
stream009.bin
stream010.bin
```

But its `stream001.gim` and `stream003.gim` differ from those in `pl00.pzz`.

Since the runtime table chooses between `pl00.pzz` and `pl00l.pzz` via a flag, the currently visible RX-78-2 may be using `pl00l.pzz` rather than `pl00.pzz`.

This is now a stronger texture lead than `W_DATA/pl00ov*.bin`.

## PBG Resource Finding

While scanning `pl00ov*.bin`, several values matched `Z_DATA` entries named `pbg*.pzz`, especially:

```text
2499 -> pbg000.pzz
2500 -> pbg001.pzz
2540 -> pbg029.pzz
2508 -> pbg009.pzz
```

The earlier extractor failed on `pbg*.pzz` because those PZZ files use:

```text
entry_count = 1
```

The Rust key finder previously searched from `2..200`, so it skipped valid single-stream PZZ files. This has been fixed in `rust_converter/src/pzz.rs` by starting at `1`.

`pbg000.pzz` and `pbg001.pzz` both extract as a single GIM stream:

```text
pbg000.pzz:
  stream000.gim
  size = 131792
  image = 480x272
  format = 5
  pixel_order = 1
  palette format = 3
  palette = 256x1

pbg001.pzz:
  stream000.gim
  size = 131792
  image = 480x272
  format = 5
  pixel_order = 1
  palette format = 3
  palette = 256x1
```

Converted previews are:

```text
converted_out/2499_pbg000_test/stream000.png
converted_out/2500_pbg001_test/stream000.png
```

Because these images are 480x272, they are more likely full-screen UI/background/cut-in resources than model body textures.

However, the repeated references from `pl00ov5.bin` make them worth testing in game if the visible target is a UI/cut-in/background element.

## PZZ Tail: Complete Reverse Engineering

### Root Cause of Infinite Loading

The 16-byte PZZ tail is a **custom integrity checksum** computed during XOR decryption. The game verifies this checksum at load time. If it does not match, the resource load fails silently, causing infinite loading.

This is why even a 1-bit change to `pl0a.pzz/stream001.gim` caused infinite loading: the stream data changed, but the 16-byte tail checksum was not recomputed.

### Call Chain

```text
sub_88867AC          top-level resource loader
  sub_88858F0        looks up file body size from ROM table
  sub_8886260        reads file from AFS / save cache
  sub_8885A84        XOR decrypt + hash verify
    sub_880A338        memcpy: copies last 16 bytes (raw tail)
    sub_8887910        submits decrypt+hash job (command type 1)
      → worker thread → sub_88879E0
        → sub_88BD520    hash wrapper
          sub_88BD728    XOR decrypt in-place + compute 16-byte hash
          sub_880A2A4    memcmp: compare computed hash vs stored tail
  sub_88866E4        parse decrypted PZZ header (entry count, descriptors)
  sub_8885B78        decompress PZZ streams (command type 2)
    → worker thread → sub_88868C4  zlib inflate per-stream
```

### Conditional Hash Check

The hash check is gated by a flag:

```text
if ( (*(_BYTE *)(state + 1) & 1) != 0 )
    → perform decrypt + hash verify
else
    → skip (file is not XOR-encrypted)
```

All tested `pl*.pzz` files have this flag set.

### File Layout

```text
Offset 0          .. body_size-1   : XOR-encrypted body
Offset body_size  .. body_size+15  : raw checksum (NOT encrypted)
```

Total file size = body_size + 16.

The body size for each Z_DATA entry is stored in a ROM table at `0x8A56160`:

```text
dword_8A56160[z_data_entry_index] = body_size
```

`body_size + 16` equals the AFS entry file size.

### XOR Key Derivation

The XOR decryption key is derived from `body_size` using two 256-byte substitution tables.

Step 1 — extract nibble from body_size:

```text
shift = 3
while shift < 30:
    nibble = (body_size >> shift) & 0xF
    if nibble != 0: break
    shift += 3
```

Step 2 — derive secondary index:

```text
derived = (byte)(13 * (nibble + 3))
```

Step 3 — table lookups:

```text
key1 = table_lookup(nibble,  step=3, table1_at_0x8A0E038)
key2 = table_lookup(derived, step=2, table2_at_0x8A0E138)
xor_key = key1 ^ key2
```

Where `table_lookup(idx, step, table)` reads 4 bytes:

```text
b0 = table[idx]
b1 = table[(idx + step) & 0xFF]
b2 = table[(idx + 2*step) & 0xFF]
b3 = table[(idx + 3*step) & 0xFF]
return (b0 << 24) | (b1 << 16) | (b2 << 8) | b3
```

### Verified Key Derivations

```text
pl0a.pzz  (entry 1659):
  body_size = 631168 = 0x9A180
  nibble = 6  (at shift=6)
  derived = 0x75
  key1 = 0x6C07313D
  key2 = 0x26B03ABD
  xor_key = 0x4AB70B80  ✓ matches find_pzz_key result

pl0al.pzz (entry 1736):
  body_size = 609664 = 0x94D80
  nibble = 6  (at shift=6)
  derived = 0x75
  key1 = 0x6C07313D
  key2 = 0x26B03ABD
  xor_key = 0x4AB70B80  ✓ same key (same nibble)

pl00l.pzz (entry 1726):
  body_size = 861952 = 0xD2700
  nibble = 12  (at shift=6)
  derived = 0xC3
  xor_key = 0x1CD56D68  ✓ matches first_word ^ 12
```

### Substitution Tables (256 bytes each)

Table 1 at `0x8A0E038`:

```text
1e 65 c2 22 20 c5 6c f1 b7 07 73 2a 31 43 48 3d
75 30 1b 78 09 2d c7 ad 0a f6 3c ac 5a 7e dd 0d
5b 25 00 fd 9b 02 bd 52 08 93 8b 9d 46 11 34 b2
bb cd d0 c4 84 c6 d4 28 6e cf 32 9e 19 eb e2 40
ca c9 c1 a0 1c 60 e0 39 4c 56 45 69 e3 3e 9f 05
35 cc b0 13 0f da f8 26 fe 99 54 d8 ae 92 29 e4
72 2b c0 04 23 15 37 a3 f4 49 a5 5d bf 7c 38 c8
06 89 be db b1 a1 27 74 4d 4b 03 51 16 01 77 f0
55 5e 97 d3 0e 50 ed 63 6d d5 c3 4f 82 bc 91 80
a9 ce 3b 36 ec 79 1d 5c 24 98 8e df e8 4a aa f9
e1 ef fc 9c e9 17 ee b8 a4 f2 af 83 1f fa 58 18
a8 6f 71 8c 95 e6 85 f7 64 f5 b5 33 d7 12 e7 7f
ff 86 5f 9a 62 8f 2f 68 d6 a6 b4 53 3a 76 d1 7a
7d ab 21 90 6a ba ea fb 44 59 b6 87 e5 0b 1a 67
8d b3 14 a7 a2 3f d9 0c 8a 81 10 7b dc cb f3 66
2e 57 b9 47 61 4e 2c 42 de 96 6b 41 94 88 70 d2
```

Table 2 at `0x8A0E138`:

```text
74 97 27 1e 65 fe f5 09 71 78 1d 54 7b d3 16 98
87 4c e9 33 b9 82 8f 6c 3e 5d 24 55 23 7e ee d9
32 e2 eb 94 2f 9c 31 7a 02 10 2b cf 56 a7 ce 6b
c6 67 06 4e b8 b4 cc ae 8e d1 e5 c8 59 cd 8c 49
51 03 bf 89 4f 95 07 25 4b c4 e7 d2 fd 44 96 91
66 05 80 c3 19 0d ff 20 a8 2a d8 79 d5 5b 84 9d
c0 36 6a 9e a0 9f 3b a4 e0 21 2c 5f 53 be 11 81
28 47 a1 88 12 26 39 b0 fb 3a 50 bd 5a f4 bc ab
40 04 b3 f6 9b cb e1 3f bb 1c de 73 0f 08 01 15
13 42 72 4d 0c 1a b7 7d f7 ec ac 48 62 34 fa ba
a6 df 7c 92 8a ad b5 75 64 69 c2 5c da 90 68 43
a5 aa f9 e6 41 63 57 6d 14 93 6e 61 83 c5 17 52
4a 30 f2 2d 22 e8 35 76 d7 45 f1 dc b6 c7 ca db
ed d4 f3 d0 af 60 b2 18 38 c1 ea a3 dd a2 3c 0e
8b 9a 3d 1f a9 00 0b ef e3 5e 46 b1 99 29 85 1b
f8 86 37 58 c9 77 fc 8d 2e f0 0a d6 e4 70 7f 6f
```

### Checksum Algorithm

Function: `sub_88BD728` at `0x88BD728`

This function both XOR-decrypts the body in-place and computes the 16-byte checksum in a single pass.

**Initialization:**

```text
sum_lo  = (body_size * 7) >> 1     (u32, truncated)
sum_hi  = 0
xor_lo  = 0xFFFFFFFF
xor_hi  = 0xFFFFFFFF
```

**Per-word processing** (for each little-endian u32 `word` in the encrypted body):

```text
decrypted = word ^ xor_key
*ptr      = decrypted                       (decrypt in-place)

new_sum   = ((u64)sum_hi << 32 | sum_lo) + (u64)decrypted
sum_lo    = (u32)(new_sum & 0xFFFFFFFF)
sum_hi    = (u32)(new_sum >> 32)

xor_lo   ^= sum_lo
xor_hi   ^= sum_hi
```

**Remainder bytes** (if `body_size % 4 != 0`):

The last 1-3 bytes are read as a partial u32 (little-endian, zero-padded), XOR'd with `xor_key & mask`, stored back masked, and added to the running sum.

Masks: `{1: 0x000000FF, 2: 0x0000FFFF, 3: 0x00FFFFFF}`

**Output:**

```text
tail[0..3]   = sum_lo   (u32 LE)
tail[4..7]   = sum_hi   (u32 LE)
tail[8..11]  = xor_lo   (u32 LE)
tail[12..15] = xor_hi   (u32 LE)
```

### How to Repack PZZ Correctly

When modifying a PZZ stream and repacking:

1. Build the new decrypted PZZ body (descriptor table + all chunks, same layout as original).
2. Derive `xor_key` from `body_size` using the table lookup algorithm above.
3. XOR-encrypt the body: `encrypted[i] ^= xor_key_bytes[i % 4]` for all bytes.
4. Compute the 16-byte checksum over the **encrypted** body by running `sub_88BD728` logic:
   - Initialize `sum_lo`, `sum_hi`, `xor_lo`, `xor_hi` from `body_size`.
   - For each u32: decrypt (XOR), accumulate sum, accumulate XOR chain.
   - Alternatively: run the same algorithm on the **decrypted** body directly, since the hash is over decrypted values.
5. Append the raw 16-byte checksum after the encrypted body.

The checksum can be computed equivalently from decrypted data:

```text
Given: decrypted_body (all bytes before the tail)
body_size = len(decrypted_body)

sum_lo  = (body_size * 7) >> 1
sum_hi  = 0
xor_lo  = 0xFFFFFFFF
xor_hi  = 0xFFFFFFFF

for each u32 word in decrypted_body (LE):
    sum64    = ((u64)sum_hi << 32 | sum_lo) + word
    sum_lo   = (u32)(sum64)
    sum_hi   = (u32)(sum64 >> 32)
    xor_lo  ^= sum_lo
    xor_hi  ^= sum_hi

checksum = sum_lo || sum_hi || xor_lo || xor_hi   (16 bytes LE)
```

Then the final PZZ file is:

```text
xor_encrypt(decrypted_body, xor_key) || checksum
```

### IDA Function Reference

```text
0x88BD728  pzz_decrypt_and_hash     XOR decrypt body + compute 16-byte checksum
0x88BD520  pzz_verify_hash          calls hash, then memcmp with stored tail
0x88BD560  pzz_derive_key_params    extract nibble + derived from body_size
0x88BD5B4  pzz_table_lookup         4-byte key from substitution table
0x8885A84  pzz_decrypt_verify       orchestrates tail copy + decrypt+hash job
0x88868C4  pzz_decompress_streams   iterates descriptors, zlib inflate per stream
0x88866E4  pzz_parse_header         reads entry_count and descriptor pointers
0x8862888  zlib_inflate_wrapper      zlib decompression (calls inflate loop)
0x880A2A4  memcmp                   byte comparison
0x880A338  memcpy                   byte copy
0x8A0E038  pzz_sbox_table1          256-byte substitution table 1
0x8A0E138  pzz_sbox_table2          256-byte substitution table 2
0x8A56160  z_data_body_size_table   body_size per Z_DATA entry index
```

## PMF2 Vertex Coordinate System & Rendering Pipeline

### Confirmed via IDA Reverse Engineering

The PMF2 header bbox at offset `0x10-0x18` is **not** a metadata field — it is a **runtime rendering scale factor** used by the GE (Graphics Engine) hardware pipeline.

### Rendering Call Chain

```text
sub_88BCE3C    initialize render sections from PMF2 data
  → reads PMF2 header bbox [0x10, 0x14, 0x18]
  → stores bbox as 3 floats into render_section + 96

sub_89841E8    emit normal textured draw GE commands
  → calls sub_886BBDC to build scaled world matrix
  → calls sub_89BEC88 to upload world matrix to GE
  → calls sub_89BED1C to execute display list (CALL command)

sub_886BBDC    VFPU matrix scale (the critical function)
  → lv.q  C100.q, (render_section + 96)    ; load bbox [sx, sy, sz, ?]
  → lv.q  C000-C030, (bone_world_matrix)   ; load 4x4 world matrix
  → vscl.q  row0 *= sx                      ; scale X-axis row by bbox[0]
  → vscl.q  row1 *= sy                      ; scale Y-axis row by bbox[1]
  → vscl.q  row2 *= sz                      ; scale Z-axis row by bbox[2]
  → row3 unchanged (translation)
```

### Coordinate System

PMF2 vertices are stored as **signed 16-bit integers** (i16, range [-32768, 32767]).

The game converts them to world-space positions via:

```text
world_pos = GE_ModelViewProj × (bone_world_matrix × diag(bbox)) × i16_vertex
```

Where:

```text
diag(bbox) = | bbox[0]  0       0       0 |
             | 0        bbox[1] 0       0 |
             | 0        0       bbox[2] 0 |
             | 0        0       0       1 |
```

The bbox values define the **maximum representable extent** in each axis. An i16 value of 32767 maps to exactly `bbox[axis]` in bone-local space.

### DAE ↔ PMF2 Coordinate Mapping

Export (PMF2 → DAE):

```text
dae_local_pos = i16_pos × bbox / 32768.0
```

Import (DAE → PMF2):

```text
i16_pos = clamp(round(dae_local_pos × 32768.0 / bbox), -32768, 32767)
```

### PMF2 Section Header Layout (256 bytes = 0x100)

```text
0x00-0x3F   4×4 local bone matrix (16 × f32, row-major)
0x40-0x4F   bounding center + radius (4 × f32)
0x50-0x5F   color scale / material params (4 × f32, typically [1.0, 1.0, 1.0, 0.0])
0x60-0x6F   bone name (null-terminated ASCII, 16 bytes max)
0x70        has_mesh flag (u32): 0 = has display list, 1 = no display list
0x74        auxiliary data (u32): 0 for mesh sections, varies for non-mesh
0x78        reserved (u32)
0x7C        parent section index (u32, 0xFFFFFFFF = root)
0x80        sibling link (u32, 0xFFFFFFFF = none)
0x84-0xBF   additional links (unused slots = 0xFFFFFFFF)
0xC0-0xFF   children section indices (up to 16 slots, 0xFFFFFFFF = empty)
```

### Display List Structure (at section_offset + 0x100)

```text
0x14000000   ORIGIN (reset address base)
0x10000000   BASE (set base address = 0)
0x02xxxxxx   IADDR (index buffer offset from base)
0x01xxxxxx   VADDR (vertex buffer offset from base)
0x12xxxxxx   VERTEXTYPE (vertex format descriptor)
0x9Bxxxxxx   CMD_9B (material/texture state, per draw call)
0x04TTNNNN   PRIM (TT=primitive type 04=triangles, NNNN=vertex count)
...          (multiple PRIM+CMD_9B pairs)
0x0B000000   RET (end of display list)
```

### Vertex Format (VERTEXTYPE = 0x001142)

```text
bits [1:0]  = 2  → texture coords: 16-bit signed
bits [6:5]  = 2  → normals: 16-bit signed
bits [8:7]  = 2  → positions: 16-bit signed
```

Per-vertex stride = 16 bytes:

```text
offset 0:  tu (i16)   texture U
offset 2:  tv (i16)   texture V
offset 4:  nx (i16)   normal X
offset 6:  ny (i16)   normal Y
offset 8:  nz (i16)   normal Z
offset 10: px (i16)   position X
offset 12: py (i16)   position Y
offset 14: pz (i16)   position Z
```

Texture UV mapping: `uv_float = i16_value / 32768.0`

Normal mapping: `normal_float = i16_value / 32767.0`

Position mapping: `local_pos = i16_value × bbox[axis] / 32768.0`

### has_mesh Flag (Section +0x70)

This field controls whether the game attempts to render a display list for this section:

```text
*(pmf2_section + 0x70) == 0  → game sets display_list = section + 256, renders mesh
*(pmf2_section + 0x70) != 0  → game skips rendering this section
```

When removing a mesh from a section, this field MUST be set to 1. Leaving it as 0 with an empty/zeroed display list causes the GE state machine to process invalid commands, corrupting rendering for all subsequent draw calls (other models, stage objects, etc.).

### Size Constraints for New Meshes

All sections in a single PMF2 file share the same bbox. The maximum vertex extent in DAE local space is:

```text
X: ±bbox[0]   (for pl0a: ±2.676)
Y: ±bbox[1]   (for pl0a: ±12.560)
Z: ±bbox[2]   (for pl0a: ±12.766)
```

Vertices exceeding these ranges are clamped to i16 extremes, causing visible distortion.

To support larger meshes, a full vertex re-encoding is required:

1. Decode all original sections' vertices using old bbox.
2. Compute new bbox from all vertices (original + new).
3. Re-encode ALL sections' vertex data with the new bbox.
4. Update the PMF2 header bbox at 0x10-0x18.
5. Regenerate all display lists with the re-encoded vertex data.

### Verified Experiment Results

| Test | Result |
|---|---|
| Zero all PMF2+GIM streams in pl0a/pl0al | Nu Gundam completely invisible, other objects affected |
| Remove m11 mesh without setting +0x70=1 | Model partially invisible, GPU state corruption, stage objects missing |
| Remove m11 mesh with +0x70=1 | Only head (m11) cleanly removed, everything else normal |
| Add new meshes within bbox range | New geometry renders correctly |
| Add new meshes exceeding bbox range | New geometry distorted (axis clamping) |
| Expand bbox without re-encoding old vertices | New meshes correct, old meshes shrunk/distorted |

### IDA Function Reference (PMF2 Rendering)

```text
0x88BCE3C  pmf2_init_render_sections    reads bbox from header, populates render rows
0x886BBDC  vfpu_scale_world_matrix      scales world matrix rows by bbox (VFPU vscl.q)
0x89841E8  emit_normal_draw             sets up texture, matrix, calls display list
0x89BEC88  upload_world_matrix          emits GE WORLD_MATRIX commands (0x3A/0x3B)
0x89BED1C  emit_display_list_call       emits GE CALL command to display list
0x89BE630  emit_ambient_color           emits GE AMBIENT_COLOR command (0x55)
0x89BE7D4  emit_texture_setup           emits texture/CLUT GE commands
```


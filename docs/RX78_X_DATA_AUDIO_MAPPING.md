# RX-78-2 X_DATA Audio Mapping

This note records the current findings for `X_DATA.BIN` and the RX-78-2
(`unit_id = 0`, `pl00`) resources inside it.

## Summary

`X_DATA.BIN` is an AFS archive of CRI ADX audio files. It does not contain the
RX-78-2 HP, boost, ammo counts, weapon behavior, or input state machine data.

For RX-78-2, the relevant `X_DATA.BIN` entries are the `v00` voice families:

```text
v00   -> primary RX-78-2 voice cue group
v00d  -> auxiliary RX-78-2 voice cue group
v00p  -> small RX-78-2 partner/special voice cue group
```

The extracted files are available at:

```text
game_assets/x_data/all/
game_assets/x_data/rx78_v00/
game_assets/x_data/rx78_v00_manifest.csv
game_assets/x_data_inventory.json
```

## Archive Inventory

The source archive is the root-level file:

```text
X_DATA.BIN
```

Inventory result:

```text
archive size      624,109,568 bytes
entry count       14,623
name table offset 623,407,104
name table size   701,904
name entry size   48
file extension    .adx for all entries
```

The archive starts with BGM entries and then unit voice groups:

```text
0000..0055  bgm*.adx
0056..0111  s_bgm*.adx
0112..0353  v00_*.adx
0354..0393  v00d_*.adx
0394..0396  v00p_*.adx
0397..0572  v01_*.adx
```

## RX-78-2 Entries

RX-78-2 uses the `v00` voice namespace. The copied RX-78-2 subset contains
285 ADX files:

```text
group  count  index range
v00    242    112..353
v00d    40    354..393
v00p     3    394..396
```

Examples:

```text
0112_v00_0000.adx
0113_v00_0001.adx
0217_v00_1900.adx
0354_v00d_9401.adx
0394_v00p_a000.adx
0395_v00p_a001.adx
0396_v00p_a002.adx
```

Representative ADX header checks:

```text
0112_v00_0000.adx
  channels  1
  rate      22000 Hz
  samples   22285
  duration  1.013 seconds

0217_v00_1900.adx
  channels  1
  rate      22000 Hz
  duration  7.747 seconds

0354_v00d_9401.adx
  channels  1
  rate      22000 Hz
  duration  1.556 seconds

0394_v00p_a000.adx
  channels  1
  rate      22000 Hz
  duration  5.888 seconds
```

These headers match voice clips, not parameter tables or executable overlays.

## Cue Prefixes

The primary `v00` group has many cue prefix families. Larger families include:

```text
0x00  15 clips
0x12  10 clips
0x19   6 clips
0x27  11 clips
0x3b  14 clips
0x84  22 clips
0x8a   9 clips
```

The `v00d` group uses higher cue prefixes:

```text
0x90   2 clips
0x94  10 clips
0x98   6 clips
0x99   5 clips
0x9b   1 clip
0x9c   8 clips
0x9d   6 clips
0x9e   2 clips
```

The `v00p` group uses:

```text
0xa0   3 clips
```

The exact semantic label for each cue prefix still needs runtime playback
tracing or manual audio audition. The file names alone prove unit ownership and
cue grouping, but not the in-game event label.

## IDA Findings

Known container registration:

```text
off_8A1BD2C
  X_DATA.BIN -> container pointer 0x08B829C0, count 0x391F
  Y_DATA.BIN
  Z_DATA.BIN
  W_DATA.BIN
```

`sub_8883C40` initializes the four main archives through this table. For
`X_DATA.BIN`, the configured count is `0x391F`, which is 14,623 entries and
matches the extracted AFS inventory.

Sound initialization also references `X_DATA.BIN`:

```text
sub_89BC0AC
  references "disc0:/PSP_GAME/USRDIR/"
  references "disc0:"
  references "FLIST.DIR"
  references "X_DATA.BIN"
  waits for sub_8829FD0("X_DATA.BIN")
  calls sub_8825724 with sound/archive setup data
```

No obvious executable format string such as `v%02d_*.adx` was found. The game
likely plays X_DATA audio by archive index, cue ID, or a table generated from
the AFS name table rather than formatting ADX file names directly.

## Relationship To HP, Boost, Ammo, Weapons, And Input

The data requested earlier is not in `X_DATA.BIN`:

```text
HP fields              runtime unit structure, accessed by generic code
Ammo slots            runtime unit structure, initialized/consumed by helpers
Weapon behavior       W_DATA pl00ov*.bin overlays
Input/action state    W_DATA pl00ov*.bin overlays plus generic state helpers
Boost                 still unresolved, likely generic runtime fields/UI trace
```

The confirmed RX-78-2 gameplay files remain:

```text
game_assets/w_data/pl00/pl00ov0.bin
game_assets/w_data/pl00/pl00ov1.bin
game_assets/w_data/pl00/pl00ov2.bin
game_assets/w_data/pl00/pl00ov3.bin
game_assets/w_data/pl00/pl00ov4.bin
game_assets/w_data/pl00/pl00ov5.bin
```

See `docs/PL00_W_DATA_BEHAVIOR_ANALYSIS.md` for the confirmed HP, ammo,
weapon/action dispatcher, and input/action state findings.

## Current Conclusion

`X_DATA.BIN` contributes RX-78-2 audio presentation only. The RX-78-2 X_DATA
files are `v00`, `v00d`, and `v00p` ADX voice clips. They should be useful for
voice replacement or cue-event mapping, but they are not the source of
mechanical data such as HP, boost, bullet counts, weapon behavior, or the input
state machine.

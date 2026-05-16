# PZZ BIN Runtime Analysis TODO

## Goal

Analyze how the game loads and uses model PZZ streams, especially the unknown
BIN streams, SAD animation data, weapon configuration, ammo slots, unit stats,
and possible unit logic/control data.

## Tasks

- [x] Read repository agent rules and relevant research notes.
- [x] Create the session folder and tracking files.
- [x] Inventory `game_assets/z_data/pl00` stream files and byte-level structure.
- [x] Use IDA MCP to trace resource loading and known runtime functions.
- [x] Correlate `pl00` stream layout with existing PMF2/GIM/SAD/BIN findings.
- [x] Identify likely locations for unit stats, weapon/ammo data, and input logic.
- [x] Extract root `W_DATA.BIN` into `game_assets/w_data/all`.
- [x] Copy `pl00ov0.bin..pl00ov5.bin` into `game_assets/w_data/pl00`.
- [x] Analyze `pl00ov0.bin` overlay code and data tables in IDA.
- [x] Confirm HP runtime fields and ammo slot runtime fields.
- [x] Extract root `X_DATA.BIN` into `game_assets/x_data/all`.
- [x] Copy RX-78-2 `v00`, `v00d`, and `v00p` ADX files into
  `game_assets/x_data/rx78_v00`.
- [x] Create `game_assets/x_data/rx78_v00_manifest.csv`.
- [x] Confirm that `X_DATA.BIN` is audio-only for RX-78-2 and not the source of
  HP, boost, ammo, weapon behavior, or input state machine data.
- [x] Record confirmed findings, hypotheses, and next reverse-engineering steps.

## Current Status

Third pass complete. `W_DATA.BIN` was extracted and the six `pl00ov*.bin`
overlays were analyzed. `X_DATA.BIN` was also extracted and confirmed to be an
ADX audio archive. RX-78-2 audio is in the `v00`, `v00d`, and `v00p` groups.
Weapon behavior and action state mapping are in W_DATA `MWo3` overlays, while
HP and ammo are generic runtime unit fields accessed by those overlays. Boost
remains the main unresolved field and needs a focused UI/runtime trace.

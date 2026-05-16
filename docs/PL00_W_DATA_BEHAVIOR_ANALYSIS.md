# PL00 W_DATA Behavior Analysis

## Scope

This note tracks the `pl00` behavior/runtime files for Mobile Suit Gundam:
Gundam vs. Gundam Next Plus. It focuses on HP, boost-related runtime fields,
ammo counts, weapon behavior, and input/action state dispatch.

The target source archive is root-level `W_DATA.BIN`. It was extracted to:

```text
game_assets/w_data/all/
game_assets/w_data/pl00/
```

The `pl00` W_DATA entries are:

```text
0000_pl00ov0.bin -> game_assets/w_data/pl00/pl00ov0.bin
0001_pl00ov1.bin -> game_assets/w_data/pl00/pl00ov1.bin
0002_pl00ov2.bin -> game_assets/w_data/pl00/pl00ov2.bin
0003_pl00ov3.bin -> game_assets/w_data/pl00/pl00ov3.bin
0004_pl00ov4.bin -> game_assets/w_data/pl00/pl00ov4.bin
0005_pl00ov5.bin -> game_assets/w_data/pl00/pl00ov5.bin
```

## Overlay Layout

All six `pl00ov*.bin` files are `MWo3` runtime overlays.

```text
size       = 0x8D00
magic      = MWo3
section A  = 0x40..0x5C78
section B  = 0x5C78..0x8D00
```

Each overlay contains MIPS code and local pointers. `pl00ov0.bin` is loaded at
`0x09BCF800` in the active IDA database. The first overlay code begins at
`0x09BCF880`.

Overlay base addresses from the MWo3 headers:

```text
pl00ov0.bin base 0x09BCF800 end 0x09BD8500
pl00ov1.bin base 0x09BE3600 end 0x09BEC300
pl00ov2.bin base 0x09BF7400 end 0x09C00100
pl00ov3.bin base 0x09C0B200 end 0x09C13F00
pl00ov4.bin base 0x09C1F000 end 0x09C27D00
pl00ov5.bin base 0x09C32E00 end 0x09C3BB00
```

The six overlays are near-identical slot variants. They differ by roughly
784-869 bytes against `pl00ov0.bin`.

## Runtime Loading

IDA confirms this mapping for `unit_id = 0`:

```text
W_DATA 0..5 -> pl00ov0.bin .. pl00ov5.bin
Z_DATA 1649 -> pl00.pzz
Z_DATA 1726 -> pl00l.pzz
```

The relevant loader chain is:

```text
sub_8922D18
  -> word_8A6B8D4[unit_id] + overlay_slot
  -> W_DATA overlay entry

sub_89230F0
  -> word_8A6BA6C[2 * unit_id + side_flag]
  -> Z_DATA model PZZ entry
```

The important separation is:

- `Z_DATA/pl00.pzz` supplies model, texture, SAD animation, and companion
  model/effect parameter streams.
- `W_DATA/pl00ov*.bin` supplies executable unit-specific behavior code and
  behavior tables.
- Several core runtime fields such as HP and generic ammo slot state live in
  the main executable's unit structure, not inside the PZZ stream files.

## Confirmed HP Fields

The runtime HP fields are generic unit fields:

```text
unit + 0x250 / 592 : max HP as float
unit + 0x254 / 596 : current HP as float
unit + 0x258 / 600 : previous/display HP as float
```

Key evidence:

```text
sub_88A8844
  reads unit+596
  subtracts incoming damage from unit+596
  clamps unit+596 to unit+592
  sets unit+596 to 0 when dead

sub_895BC64
  computes damage/HP ratios using unit+592 and unit+596

sub_88FA6F4
  checks unit+596 against absolute and ratio thresholds:
    current HP <= 0.333 * max HP
    current HP <= 200.0
    current HP <= 0.5 * max HP
```

`pl00ov0` also reads HP in a unit-specific action branch:

```text
sub_9BCFD18 case 9:
  if (*(float *)(unit + 596) < 101.0)
    sub_89B18EC(unit, 6)
    sub_89B342C(unit, 1)
```

This proves that `pl00` overlay behavior can branch on HP, but the HP storage
and damage application are generic runtime systems.

## Confirmed Ammo Slot Structure

The generic weapon ammo slot array starts at:

```text
unit + 0x650 / 1616
```

Each regular weapon slot is 20 bytes:

```text
slot_base = unit + 1616 + slot_index * 20

slot + 0x00 u8    reload/ammo mode
slot + 0x01 u8    sub-timer/countdown byte
slot + 0x02 i16   dirty/reload flag
slot + 0x04 i16   current ammo
slot + 0x06 i16   previous ammo/display ammo
slot + 0x08 f32   scalar, initialized to 1.0
slot + 0x0C f32   reload/progress timer
slot + 0x10 f32   cooldown/ready timer
```

The primary ammo definition pointer array is read from:

```text
*(unit + 1004) + 200
```

Each slot definition begins with small signed halfwords:

```text
def + 0x00 i16 max ammo or negative max ammo
def + 0x02 low nibble reload/ammo mode
def + 0x04 i16 reload step/time
def + 0x06 i16 cooldown/ready time
def + 0x08 u8  flags
```

Key functions:

```text
sub_89B6FE0(slot, def)
  initializes one 20-byte ammo slot from the definition row.

sub_89B70A8(unit)
  initializes all regular ammo slots from *(unit+1004)+200.

sub_89B730C(unit, slot, amount, flags)
  checks whether the slot has enough ammo.
  if flags bit 0 is set, consumes ammo and updates timers.

sub_89B7484(unit, slot)
  checks whether the slot is full.

sub_89B7968(unit, slot, amount)
  restores ammo, or refills the slot when amount is negative.

sub_89B75C0(unit)
  advances ammo reload timers for regular slots and the special slot.
```

`sub_89B730C` is the strongest confirmed ammo consumption entry point:

```text
if current_ammo < amount:
  return 0

if consume_flag:
  unit+2724 += amount
  unit+2727 = 1
  unit+2831 = 0
  previous_ammo = current_ammo
  current_ammo -= amount
  cooldown_timer = def[3]
  update reload/progress timer by mode
```

There is also a special/shared ammo slot at:

```text
unit + 0x790 / 1936
```

It is driven from:

```text
*(unit + 1004) + 196
```

The special-slot consume function is:

```text
sub_89B71EC(unit, amount, flags)
```

## PL00 Weapon Action Dispatcher

The strongest `pl00` weapon behavior function found so far is:

```text
0x09BCFD18 sub_9BCFD18(unit, action_case, ...)
```

It dispatches action cases and calls the generic ammo checker before spawning
animations/effects/action transitions.

Confirmed cases:

```text
case 0:
  sub_89B730C(unit, 0, 1, 1)
  sub_89F78A0(unit, 0, unit+2192)
  sub_8869794(unit, 16, 0, 0)

case 1:
  sub_89B730C(unit, 2, 1, 1)
  sub_89F78A0(unit, 2, unit+2192)
  sub_8869794(unit, 18, 0, 0)

case 2:
  sub_89B730C(unit, -1, 1, 1)
  sub_89F78A0(unit, 1, unit+2192)
  sub_8869708(unit, 40, 0, 0)

case 3:
  sub_9BD2618(unit, unit+2192)
  sub_8869708(unit, 40, 0, 0)
  unit+2676 |= 1

case 4:
  sub_89B730C(unit, -1, 2, 1)
  sub_89F78A0(unit, 4, unit+2192)
  sub_8869794(unit, 16, 0, 0)
  sub_8998628(unit, 0)

case 5:
  sub_89B730C(unit, -1, 1, 1)
  sub_89F78A0(unit, 8, unit+2192)
  sub_8869794(unit, 16, 0, 0)

case 9:
  HP threshold branch at unit+596 < 101.0
```

This is the concrete bridge between `pl00` unit-specific weapon behavior and
the generic ammo slot system.

`sub_89F78A0(unit, action_index, ...)` calls function pointers from:

```text
*(unit + 92) + action_index * 0x80
```

That makes `sub_9BCFD18` a high-level weapon/action trigger, not the final
projectile or effect implementation.

## PL00 Input And Action State Tables

`pl00ov0` initializes and registers action/event rows in:

```text
sub_9BCF880(unit)
```

It registers two 24-byte row tables:

```text
0x09BD61B8
0x09BD6200
```

via:

```text
sub_89B9DA4(unit, table)
  iterates 24-byte rows until row[0] == 0xFF
  calls sub_89B9D48(unit, row)

sub_89B9D48(unit, row)
  creates a runtime object
  stores row pointer at object+76
  installs callback sub_89B9DFC
```

`sub_89BAD98(unit, encoded_state_id)` maps encoded state IDs to four state byte
banks:

```text
0x00..0x3F -> unit + 2256
0x40..0x7F -> unit + 2272
0x80..0xBF -> unit + 2264
0xC0..0xFF -> unit + 2280
```

The `pl00` overlay directly manipulates these bytes in:

```text
sub_9BCF900
sub_9BCFC90
sub_9BD1238
sub_9BD3040
sub_9BD30E8
sub_9BD3578
```

The main action state dispatch table is at:

```text
0x09BD6240
```

`sub_9BD0038(unit)` dispatches through:

```text
dword_9BD6070[*(char *)(unit + 2738) + 116]
```

At `pl00ov0` this resolves to function pointers:

```text
0x09BD0080
0x09BD00A0
0x09BD00C0
0x09BD00E8
0x09BD0110
0x09BD0130
0x09BD0158
```

A second related dispatch path is:

```text
sub_9BD0278(unit)
  if unit+2738 != 6:
    dword_9BD6070[*(char *)(unit+2738) + 212](unit)
```

This is currently the best confirmed input/action state machine lead for
`pl00`. The actual raw button read is still in generic input code; the overlay
contains the unit-specific state mapping and action handlers.

## Boost Status

Boost is not yet as conclusively mapped as HP and ammo.

Confirmed facts:

- No `pl00.pzz` BIN stream showed a clear HP/boost/ammo table.
- The `pl00ov0` weapon/action dispatcher does not directly read or write an
  obvious "boost gauge" field in the same way it reads HP and ammo.
- Generic movement/action functions around `0x89A17C0..0x89AB324` heavily use
  runtime timers such as `unit+2040`, `unit+2044`, `unit+2048`, and speed or
  movement fields such as `unit+320`, `unit+324`, `unit+332`.
- These fields are action timers and movement parameters, not a proven boost
  gauge by themselves.

The next reliable boost step is a dynamic trace in PPSSPP or an IDA xref trace
from the UI boost meter draw routine to the runtime unit field it reads.
Static IDA analysis alone has not yet produced a single boost field with the
same confidence level as:

```text
HP:    unit+592 / unit+596
Ammo:  unit+1616 + slot*20
State: unit+2256..2287 and unit+2738
```

## File-Level Conclusion

The relevant `pl00` files are:

```text
game_assets/w_data/pl00/pl00ov0.bin
game_assets/w_data/pl00/pl00ov1.bin
game_assets/w_data/pl00/pl00ov2.bin
game_assets/w_data/pl00/pl00ov3.bin
game_assets/w_data/pl00/pl00ov4.bin
game_assets/w_data/pl00/pl00ov5.bin
```

The relevant generic executable systems are:

```text
sub_88A8844   HP damage/clamp
sub_89B6FE0   ammo slot initialization
sub_89B70A8   regular ammo slot array initialization
sub_89B730C   ammo availability and consumption
sub_89B7484   ammo full check
sub_89B7968   ammo refill
sub_89B75C0   ammo reload timer update
sub_89B9DA4   overlay event/action table registration
sub_89BAD98   encoded state-byte resolver
```

The relevant `pl00ov0` functions are:

```text
0x09BCF880  overlay init, table registration
0x09BCF900  unit-specific action/state update
0x09BCFC90  unit-specific state reset
0x09BCFD18  weapon/action dispatcher with ammo checks
0x09BD0038  state dispatch through unit+2738
0x09BD0278  secondary state dispatch through unit+2738
0x09BD1238  timed action/animation transition
```

Therefore:

- `pl00.pzz` is not the main location for HP, ammo count, weapon behavior, or
  input state machine logic.
- `pl00.sad` is animation/motion data, not the main input logic file.
- `W_DATA/pl00ov*.bin` is the correct file family for unit-specific weapon
  behavior and action state mapping.
- HP and ammo storage are generic runtime fields in the executable unit struct.
  `pl00ov` reads/updates them through generic helper functions.

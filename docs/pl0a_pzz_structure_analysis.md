# pl0a.pzz Binary Structure Analysis

## Output Files

- Source: `E:\research\gvg_np\pl0a.pzz`
- Decrypted: `E:\research\gvg_np\pl0a_de.pzz`
- Raw analysis data: `E:\research\gvg_np\pl0a_de.analysis.json`

## Decrypt Result

- `xor_key = 0x4AB70B80`
- `file_size = 631184`
- `body_size = 631168`
- `tail_size = 16`
- `entry_count = 12`
- `data_start = 0x800` (2048)

`pl0a_de.pzz` layout:

- `0x00000000 .. 0x00099FFF` : decrypted body
- `0x00099FE0 .. 0x00099FEF` : 16-byte decrypted tail footer

## Binary Example: Encrypted vs Decrypted Header

Encrypted `pl0a.pzz` first 64 bytes:

```text
8C 0B B7 4A 0E 0A B7 0A 24 0A B7 0A 10 0B B7 0A
A8 0B B7 0A 82 0B B7 0A 85 0B B7 0A 04 00 B7 0A
90 0B B7 0A 8C 0B B7 0A 83 0B B7 0A 86 0B B7 0A
19 08 B7 4A 80 0B B7 4A 80 0B B7 4A 80 0B B7 4A
```

Decrypted `pl0a_de.pzz` first 64 bytes:

```text
0C 00 00 00 8E 01 00 40 A4 01 00 40 90 00 00 40
28 00 00 40 02 00 00 40 05 00 00 40 84 0B 00 40
10 00 00 40 0C 00 00 40 03 00 00 40 06 00 00 40
99 03 00 00 00 00 00 00 00 00 00 00 00 00 00 00
```

Interpretation:

- `0x00000000`: `0x0000000C` -> descriptor count is 12
- `0x00000004..`: 12 descriptor entries (`u32`, little-endian)

## Descriptor and Chunk Layout

Each descriptor:

- bit31 (`0x40000000`) = stream flag
- low 30 bits = `units`
- `chunk_size = units * 128`

Decoded descriptors:

- idx 0: `0x4000018E`, units=398, offset=`0x00000800`, size=50944, stream=true
- idx 1: `0x400001A4`, units=420, offset=`0x0000CF00`, size=53760, stream=true
- idx 2: `0x40000090`, units=144, offset=`0x0001A100`, size=18432, stream=true
- idx 3: `0x40000028`, units=40, offset=`0x0001E900`, size=5120, stream=true
- idx 4: `0x40000002`, units=2, offset=`0x0001FD00`, size=256, stream=true
- idx 5: `0x40000005`, units=5, offset=`0x0001FE00`, size=640, stream=true
- idx 6: `0x40000B84`, units=2948, offset=`0x00020080`, size=377344, stream=true
- idx 7: `0x40000010`, units=16, offset=`0x0007C280`, size=2048, stream=true
- idx 8: `0x4000000C`, units=12, offset=`0x0007CA80`, size=1536, stream=true
- idx 9: `0x40000003`, units=3, offset=`0x0007D080`, size=384, stream=true
- idx10: `0x40000006`, units=6, offset=`0x0007D200`, size=768, stream=true
- idx11: `0x00000399`, units=921, offset=`0x0007D500`, size=117888, stream=false

## Stream Chunk Format Example

Chunk 0 first 32 bytes:

```text
00 00 C6 E2 00 01 FD C0 78 9C CC BD 07 54 53 5B
F3 07 BA 0F 24 04 92 D0 42 EF BD 89 74 E9 D2 14
```

Interpretation:

- `00 00 C6 E2` -> `comp_len = 0x0000C6E2 = 50914` (big-endian)
- `00 01 FD C0` -> `raw_len = 0x0001FDC0 = 130496` (big-endian)
- next bytes start with zlib header (`78 9C ...`)

## Stream Classification (Decoded)

- idx 0 -> `pmf2`, decoded_len=130496
- idx 1 -> `gim`, decoded_len=66256
- idx 2 -> `pmf2`, decoded_len=50384
- idx 3 -> `gim`, decoded_len=17616
- idx 4 -> `pmf2`, decoded_len=384
- idx 5 -> `gim`, decoded_len=2320
- idx 6 -> `sad`, decoded_len=755368
- idx 7 -> `unknown`, decoded_len=14048
- idx 8 -> `unknown`, decoded_len=12736
- idx 9 -> `unknown`, decoded_len=572
- idx10 -> `unknown`, decoded_len=6224
- idx11 -> non-stream raw chunk (117888 bytes)

## Tail Footer

Tail bytes in decrypted file (16 bytes):

```text
39 BE CB 9C D2 3C B6 4A 5C BA EC 27 A1 DF 48 B5
```

Tail bytes in original encrypted file:

```text
B9 B5 7C D6 52 37 01 00 DC B1 5B 6D 21 D4 FF FF
```

This footer is stored after the encrypted/decrypted body and is not part of descriptor chunks.

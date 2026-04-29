# pl0al.pmf2 First Sections Guide

這份文件重新以 `E:\research\gvg_np\pl0al.pmf2` 為來源，只看開頭三個 section：`pl0al_m00`、`pl0al_m01`、`pl0al_m02`。

重點先放在架構，不再做逐 byte 大表。你現在要先學會三件事：

1. PMF2 header 告訴你 section 數量與 bbox scale。
2. section offset table 告訴你每個 section 從哪裡開始。
3. 每個 section 前 `0x100` bytes 是節點 metadata；超過 `0x100` 的部分通常是 mesh/display-list payload。

## 1. File-Level Layout

| Range | Meaning |
|---|---|
| `0x00000000..0x0000001F` | PMF2 header |
| `0x00000020..0x0000010B` | Section offset table，共 59 筆，每筆 4 bytes |
| `0x0000010C..0x0000010F` | padding/alignment |
| `0x00000110...` | 第一個 section 開始 |

Header 重要欄位：

| Offset | Bytes | Meaning |
|---:|---|---|
| `0x00` | `50 4D 46 32` | ASCII `PMF2` |
| `0x04` | `3B 00 00 00` | section count = `59` |
| `0x08` | `20 00 00 00` | header marker = `0x20` |
| `0x10` | `40 1B 7F 40` | bbox scale X = `3.986038` |
| `0x14` | `BE F6 48 41` | bbox scale Y = `12.560240` |
| `0x18` | `32 43 4C 41` | bbox scale Z = `12.766405` |

PMF2 裡的多 byte 數值目前都按 little-endian 讀。比如 `3B 00 00 00` 代表 `0x0000003B`，也就是十進位 `59`。

## 2. Section Metadata Layout

每個 section 的前 `0x100` bytes 可以先這樣看：

| Relative Offset | Size | Meaning |
|---:|---:|---|
| `+0x00..+0x3F` | 64 bytes | local matrix，16 個 `f32` |
| `+0x40..+0x5F` | 32 bytes | auxiliary section metadata，下面會重點討論 |
| `+0x60..+0x6F` | 16 bytes | section name，C-string |
| `+0x70..+0x73` | 4 bytes | no-mesh flag，`1` 常表示沒有 mesh，`0` 常表示有 mesh/payload |
| `+0x74..+0x7B` | 8 bytes | unknown header area |
| `+0x7C..+0x7F` | 4 bytes | parent index；超出 section count 通常當成 `-1` root |
| `+0x80..+0xBF` | 64 bytes | unknown header area |
| `+0xC0..+0xFF` | 64 bytes | padding/sentinel area |
| `+0x100...` | variable | optional mesh/display-list payload |

## 3. First Three Sections

| Index | Name | Range | Size | Parent | no-mesh flag | Role |
|---:|---|---:|---:|---:|---:|---|
| 0 | `pl0al_m00` | `0x00000110..0x00000210` | `256` (`0x100`) | `-1` | `1` | root/control node |
| 1 | `pl0al_m01` | `0x00000210..0x00000310` | `256` (`0x100`) | `0` | `1` | child transform/control node |
| 2 | `pl0al_m02` | `0x00000310..0x00001890` | `5504` (`0x1580`) | `1` | `0` | first mesh-bearing node |

可以先把它看成一棵小樹：

```text
pl0al_m00  (root/control, no mesh)
└── pl0al_m01  (transform/control, no mesh)
    └── pl0al_m02  (mesh node)
```

## 4. The `+0x40..+0x5F` Block

你特別問的這段，在前三個 section 裡很關鍵，因為它不是一眼就能命名的固定欄位。

這 32 bytes 可以拆成 8 個 4-byte 欄位。問題是：同一段 bytes，在不同 section 類型下，看起來像不同資料。

### `pl0al_m00` auxiliary block

Raw bytes:

```text
00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 E0 FD 13 00 08 00 00 00 04 00 00 00 03 00 00 00
```

As `u32 little-endian`:

```text
0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x0013FDE0, 0x00000008, 0x00000004, 0x00000003
```

As `f32 little-endian`:

```text
0.0, 0.0, 0.0, 0.0, tiny, tiny, tiny, tiny
```

後 16 bytes 如果當 `f32` 看只會得到極小且沒有直觀意義的數字；當 `u32` 看則比較像有意義的整數 tuple。

### `pl0al_m01` auxiliary block

Raw bytes:

```text
00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 E0 FD 13 00 08 00 00 00 04 00 00 00 03 00 00 00
```

As `u32 little-endian`:

```text
0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x0013FDE0, 0x00000008, 0x00000004, 0x00000003
```

As `f32 little-endian`:

```text
0.0, 0.0, 0.0, 0.0, tiny, tiny, tiny, tiny
```

它和 `pl0al_m00` 完全一樣，這表示 `m00/m01` 可能共享某種 root/control 類型 metadata。

### `pl0al_m02` auxiliary block

Raw bytes:

```text
00 00 80 25 4C 16 F2 3F 3D 87 57 3F C4 4A 65 40 00 00 80 3F 00 00 80 3F 00 00 80 3F 00 00 00 00
```

As `u32 little-endian`:

```text
0x25800000, 0x3FF2164C, 0x3F57873D, 0x40654AC4, 0x3F800000, 0x3F800000, 0x3F800000, 0x00000000
```

As `f32 little-endian`:

```text
0.0, 1.891305, 0.841907, 3.582688, 1.0, 1.0, 1.0, 0.0
```

這組明顯更像 float metadata。尤其尾段是：

```text
1.0, 1.0, 1.0, 0.0
```

這很像 scale/padding 或某種 render/bounds 相關資料。

## 5. What Might `+0x40..+0x5F` Be?

目前最保守的說法：它是 auxiliary section metadata，也就是「section 額外資訊」。

對 `pl0al_m00` / `pl0al_m01` 這類 no-mesh control node：

- 前 16 bytes 是 0。
- 後 16 bytes 用 `u32` 看比較自然。
- 這不像一般 float matrix，也不像 bbox。
- 可能是 control/root 類 section 的未知整數 tuple、flag、地址、索引或 runtime metadata。

對 `pl0al_m02` 這類 mesh-bearing section：

- 8 個 4-byte 欄位用 `f32` 看更自然。
- 尾段是 `1.0, 1.0, 1.0, 0.0`，很像 scale/padding。
- 前段可能是 mesh local bounds、center/radius 或其他 render/culling metadata。

所以目前不要把它硬叫成單一固定欄位。更好的暫名是：

```text
+0x40..+0x5F = auxiliary section metadata
```

再細分假說：

```text
for no-mesh/control sections:
    likely integer metadata tuple

for mesh sections:
    likely float render/bounds metadata
```

## 6. `pl0al_m00`

`pl0al_m00` 位於：

```text
start = 0x00000110
end   = 0x00000210
size  = 0x100
```

它剛好只有 `0x100` bytes，所以它只有 section metadata，沒有 mesh payload。

重要欄位：

| Absolute Offset | Relative Offset | Meaning | Value |
|---:|---:|---|---|
| `0x00000110` | `+0x00` | local matrix | identity matrix |
| `0x00000150` | `+0x40` | auxiliary metadata | see section 4 |
| `0x00000170` | `+0x60` | name | `pl0al_m00` |
| `0x00000180` | `+0x70` | no-mesh flag | `1` |
| `0x0000018C` | `+0x7C` | parent | `-1` |

## 7. `pl0al_m01`

`pl0al_m01` 位於：

```text
start = 0x00000210
end   = 0x00000310
size  = 0x100
```

它也是純 transform/control node，沒有 mesh payload。

Local matrix translation：

```text
x = 0.0
y = 15.769179
z = 0.0
```

重要欄位：

| Absolute Offset | Relative Offset | Meaning | Value |
|---:|---:|---|---|
| `0x00000210` | `+0x00` | local matrix | identity + translation |
| `0x00000250` | `+0x40` | auxiliary metadata | see section 4 |
| `0x00000270` | `+0x60` | name | `pl0al_m01` |
| `0x00000280` | `+0x70` | no-mesh flag | `1` |
| `0x0000028C` | `+0x7C` | parent | `0` |

## 8. `pl0al_m02`

`pl0al_m02` 位於：

```text
start = 0x00000310
end   = 0x00001890
size  = 0x1580
```

它比 `0x100` 大，所以它分成兩部分：

| Range | Meaning |
|---:|---|
| `0x00000310..0x0000040F` | section metadata header |
| `0x00000410..0x0000188F` | GE/display-list payload |

重要欄位：

| Absolute Offset | Relative Offset | Meaning | Value |
|---:|---:|---|---|
| `0x00000310` | `+0x00` | local matrix | section transform |
| `0x00000350` | `+0x40` | auxiliary metadata | likely float render/bounds metadata |
| `0x00000370` | `+0x60` | name | `pl0al_m02` |
| `0x00000380` | `+0x70` | no-mesh flag | `0` |
| `0x0000038C` | `+0x7C` | parent | `1` |
| `0x00000410` | `+0x100` | payload start | PSP GE/display-list data |

## 9. First GE Words In `pl0al_m02`

`pl0al_m02` 的 payload 從 `0x00000410` 開始。parser 的讀法是：

```text
word = little-endian u32
cmd = word >> 24
param = word & 0x00FFFFFF
```

| Offset | Bytes | Word | Command | Param | Meaning |
|---:|---|---|---|---:|---|
| `0x00000410` | `00 00 00 14` | `0x14000000` | `0x14 ORIGIN` | `0` | display-list origin |
| `0x00000414` | `00 00 00 10` | `0x10000000` | `0x10 BASE` | `0` | address base setup |
| `0x00000418` | `54 12 00 02` | `0x02001254` | `0x02 IADDR` | `4692` | index buffer address |
| `0x0000041C` | `14 02 00 01` | `0x01000214` | `0x01 VADDR` | `532` | vertex buffer address |
| `0x00000420` | `42 11 00 12` | `0x12001142` | `0x12 VERTEXTYPE` | `4418` | vertex format |
| `0x00000424` | `00 00 00 9B` | `0x9B000000` | `0x9B ?` | `0` | unknown GE command |
| `0x00000428` | `07 00 04 04` | `0x04040007` | `0x04 PRIM` | `262151` | draw primitive |
| `0x0000042C` | `00 00 00 9B` | `0x9B000000` | `0x9B ?` | `0` | unknown GE command |
| `0x00000430` | `08 00 04 04` | `0x04040008` | `0x04 PRIM` | `262152` | draw primitive |
| `0x00000434` | `01 00 00 9B` | `0x9B000001` | `0x9B ?` | `1` | unknown GE command |

看到這裡先不用理解每個 GE command 的所有細節。你只要先知道：

- `VADDR` 指向 vertex buffer。
- `IADDR` 指向 index buffer。
- `VERTEXTYPE` 告訴你每個 vertex 怎麼解碼。
- `PRIM` 是一次 draw call。

## 10. Beginner Reading Order

建議順序：

1. 先看 offset table：找到每個 section 的 start/end。
2. 看 `+0x60`：知道 section name。
3. 看 `+0x7C`：知道 parent，畫出骨架樹。
4. 看 `+0x70` 和 section size：判斷有沒有 mesh。
5. 看 `+0x00..+0x3F`：理解 local transform。
6. 看 `+0x40..+0x5F`：先標成 auxiliary metadata，不要過早命名。
7. 如果 size 大於 `0x100`，再看 `+0x100` 後面的 GE/display-list。

對前三個 section，最重要的一句話是：

```text
pl0al_m00 是 root/control 節點。
pl0al_m01 是 pl0al_m00 下的 transform/control 節點。
pl0al_m02 是掛在 pl0al_m01 下、真正帶 mesh payload 的節點。
```
# pl0al.pmf2 First Sections Guide

??????? `E:\research\gvg_np\pl0al.pmf2` ?????????? section?`pl0al_m00`?`pl0al_m01`?`pl0al_m02`?

???????????? byte ??????????????

1. PMF2 header ??? section ??? bbox scale?
2. section offset table ????? section ??????
3. ?? section ? `0x100` bytes ??? metadata??? `0x100` ?????? mesh/display-list payload?

## 1. File-Level Layout

| Range | Meaning |
|---|---|
| `0x00000000..0x0000001F` | PMF2 header |
| `0x00000020..0x0000010B` | Section offset table?? 59 ???? 4 bytes |
| `0x0000010C..0x0000010F` | padding/alignment |
| `0x00000110...` | ??? section ?? |

Header ?????

| Offset | Bytes | Meaning |
|---:|---|---|
| `0x00` | `50 4D 46 32` | ASCII `PMF2` |
| `0x04` | `3B 00 00 00` | section count = `59` |
| `0x08` | `20 00 00 00` | header marker = `0x20` |
| `0x10` | `40 1B 7F 40` | bbox scale X = `3.986038` |
| `0x14` | `BE F6 48 41` | bbox scale Y = `12.56024` |
| `0x18` | `32 43 4C 41` | bbox scale Z = `12.766405` |

PMF2 ??? byte ?????? little-endian ???? `3B 00 00 00` ?? `0x0000003B`??????? `59`?

## 2. Section Metadata Layout

?? section ?? `0x100` bytes ???????

| Relative Offset | Size | Meaning |
|---:|---:|---|
| `+0x00..+0x3F` | 64 bytes | local matrix?16 ? `f32` |
| `+0x40..+0x5F` | 32 bytes | auxiliary section metadata???????????? |
| `+0x60..+0x6F` | 16 bytes | section name?C-string |
| `+0x70..+0x73` | 4 bytes | no-mesh flag?`1` ????? mesh?`0` ???? mesh/payload |
| `+0x74..+0x7B` | 8 bytes | unknown header area |
| `+0x7C..+0x7F` | 4 bytes | parent index??? section count ???? `-1` root |
| `+0x80..+0xBF` | 64 bytes | unknown header area |
| `+0xC0..+0xFF` | 64 bytes | padding/sentinel area |
| `+0x100...` | variable | optional mesh/display-list payload |

## 3. First Three Sections

| Index | Name | Range | Size | Parent | no-mesh flag | Role |
|---:|---|---:|---:|---:|---:|---|
| 0 | `pl0al_m00` | `0x00000110..0x00000210` | `256` (`0x100`) | `-1` | `1` | root/control node |
| 1 | `pl0al_m01` | `0x00000210..0x00000310` | `256` (`0x100`) | `0` | `1` | child transform/control node |
| 2 | `pl0al_m02` | `0x00000310..0x00001890` | `5504` (`0x1580`) | `1` | `0` | first mesh-bearing node |

????????????

```text
pl0al_m00  (root/control, no mesh)
??? pl0al_m01  (transform/control, no mesh)
    ??? pl0al_m02  (mesh node)
```

## 4. The `+0x40..+0x5F` Block

??????????? section ??????????????????????

? 32 bytes ???? 8 ? 4-byte ?????????? bytes???? section ?????????????

### `pl0al_m00` auxiliary block

Raw bytes:

```text
00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 E0 FD 13 00 08 00 00 00 04 00 00 00 03 00 00 00
```

As `u32 little-endian`:

```text
0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x0013FDE0, 0x00000008, 0x00000004, 0x00000003
```

As `f32 little-endian`:

```text
0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0
```

### `pl0al_m01` auxiliary block

Raw bytes:

```text
00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 E0 FD 13 00 08 00 00 00 04 00 00 00 03 00 00 00
```

As `u32 little-endian`:

```text
0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x0013FDE0, 0x00000008, 0x00000004, 0x00000003
```

As `f32 little-endian`:

```text
0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0
```

### `pl0al_m02` auxiliary block

Raw bytes:

```text
00 00 80 25 4C 16 F2 3F 3D 87 57 3F C4 4A 65 40 00 00 80 3F 00 00 80 3F 00 00 80 3F 00 00 00 00
```

As `u32 little-endian`:

```text
0x25800000, 0x3FF2164C, 0x3F57873D, 0x40654AC4, 0x3F800000, 0x3F800000, 0x3F800000, 0x00000000
```

As `f32 little-endian`:

```text
0.0, 1.891305, 0.841907, 3.582688, 1, 1, 1, 0.0
```

## 5. What Might `+0x40..+0x5F` Be?

??????????? auxiliary section metadata?????section ??????

? `pl0al_m00` / `pl0al_m01` ?? no-mesh control node?

- ? 16 bytes ? 0?
- ? 16 bytes ? `u32` ??????
- ????? float matrix???? bbox?
- ??? control/root ? section ????? tuple?flag??????? runtime metadata?

? `pl0al_m02` ?? mesh-bearing section?

- 8 ? 4-byte ??? `f32` ?????
- ?????? `1.0, 1.0, 1.0, 0.0`??? scale/padding?
- ????? mesh local bounds?center/radius ??? render/culling metadata?

?????????????????????????

```text
+0x40..+0x5F = auxiliary section metadata
```

??????

```text
for no-mesh/control sections:
    likely integer metadata tuple

for mesh sections:
    likely float render/bounds metadata
```

## 6. `pl0al_m00`

`pl0al_m00` ???

```text
start = 0x00000110
end   = 0x00000210
size  = 0x100
```

????? `0x100` bytes?????? section metadata??? mesh payload?

?????

| Absolute Offset | Relative Offset | Meaning | Value |
|---:|---:|---|---|
| `0x00000110` | `+0x00` | local matrix | identity matrix |
| `0x00000150` | `+0x40` | auxiliary metadata | see section 4 |
| `0x00000170` | `+0x60` | name | `pl0al_m00` |
| `0x00000180` | `+0x70` | no-mesh flag | `1` |
| `0x0000018C` | `+0x7C` | parent | `-1` |

## 7. `pl0al_m01`

`pl0al_m01` ???

```text
start = 0x00000210
end   = 0x00000310
size  = 0x100
```

???? transform/control node??? mesh payload?

Local matrix translation?

```text
x = 0.0
y = 15.769179
z = 0.0
```

?????

| Absolute Offset | Relative Offset | Meaning | Value |
|---:|---:|---|---|
| `0x00000210` | `+0x00` | local matrix | identity + translation |
| `0x00000250` | `+0x40` | auxiliary metadata | see section 4 |
| `0x00000270` | `+0x60` | name | `pl0al_m01` |
| `0x00000280` | `+0x70` | no-mesh flag | `1` |
| `0x0000028C` | `+0x7C` | parent | `0` |

## 8. `pl0al_m02`

`pl0al_m02` ???

```text
start = 0x00000310
end   = 0x00001890
size  = 0x1580
```

?? `0x100` ???????????

| Range | Meaning |
|---:|---|
| `0x00000310..0x0000040F` | section metadata header |
| `0x00000410..0x0000188F` | GE/display-list payload |

?????

| Absolute Offset | Relative Offset | Meaning | Value |
|---:|---:|---|---|
| `0x00000310` | `+0x00` | local matrix | section transform |
| `0x00000350` | `+0x40` | auxiliary metadata | likely float render/bounds metadata |
| `0x00000370` | `+0x60` | name | `pl0al_m02` |
| `0x00000380` | `+0x70` | no-mesh flag | `0` |
| `0x0000038C` | `+0x7C` | parent | `1` |
| `0x00000410` | `+0x100` | payload start | PSP GE/display-list data |

## 9. First GE Words In `pl0al_m02`

`pl0al_m02` ? payload ? `0x00000410` ???parser ?????

```text
word = little-endian u32
cmd = word >> 24
param = word & 0x00FFFFFF
```

| Offset | Bytes | Word | Command | Param | Meaning |
|---:|---|---|---|---:|---|
| `0x00000410` | `00 00 00 14` | `0x14000000` | `0x14 ORIGIN` | `0` | GE/display-list word |
| `0x00000414` | `00 00 00 10` | `0x10000000` | `0x10 BASE` | `0` | GE/display-list word |
| `0x00000418` | `54 12 00 02` | `0x02001254` | `0x02 IADDR` | `4692` | GE/display-list word |
| `0x0000041C` | `14 02 00 01` | `0x01000214` | `0x01 VADDR` | `532` | GE/display-list word |
| `0x00000420` | `42 11 00 12` | `0x12001142` | `0x12 VERTEXTYPE` | `4418` | GE/display-list word |
| `0x00000424` | `00 00 00 9B` | `0x9B000000` | `0x9B ?` | `0` | GE/display-list word |
| `0x00000428` | `07 00 04 04` | `0x04040007` | `0x04 PRIM` | `262151` | GE/display-list word |
| `0x0000042C` | `00 00 00 9B` | `0x9B000000` | `0x9B ?` | `0` | GE/display-list word |
| `0x00000430` | `08 00 04 04` | `0x04040008` | `0x04 PRIM` | `262152` | GE/display-list word |
| `0x00000434` | `01 00 00 9B` | `0x9B000001` | `0x9B ?` | `1` | GE/display-list word |


??????????? GE command ?????????????

- `VADDR` ?? vertex buffer?
- `IADDR` ?? index buffer?
- `VERTEXTYPE` ????? vertex ?????
- `PRIM` ??? draw call?

## 10. Beginner Reading Order

?????

1. ?? offset table????? section ? start/end?
2. ? `+0x60`??? section name?
3. ? `+0x7C`??? parent???????
4. ? `+0x70` ? section size?????? mesh?
5. ? `+0x00..+0x3F`??? local transform?
6. ? `+0x40..+0x5F`???? auxiliary metadata????????
7. ?? size ?? `0x100`??? `+0x100` ??? GE/display-list?

???? section??????????

```text
pl0al_m00 ? root/control ???
pl0al_m01 ? pl0al_m00 ?? transform/control ???
pl0al_m02 ??? pl0al_m01 ????? mesh payload ????
```

# EBOOT ROM Table Verification (IDA MCP)

## Goal

Verify whether the `0x8A56160` table is hardcoded in executable code/data (EBOOT) and whether it is the root cause of infinite loading when rebuilt `pl0a.pzz` changes body size.

## Scope

- Binary analysis via `ida-pro-mcp` on the currently opened IDA database.
- Focus on:
  - `dword_8A56160`
  - body-size lookup functions
  - checksum verification call path
  - cross-references proving where the table is read/written

## Key Findings

1. `dword_8A56160` is inside the executable image, not inside AFS payload files.
   - Address: `0x8A56160`
   - Symbol: `dword_8A56160`
   - Segment: `.data`
   - Segment range: `0x8A01EA4 .. 0x8AFE8B0`
   - Segment permissions reported by IDA: `perm=6` (read/write data segment)

2. The resource body size is directly read from this table.
   - `sub_8885934`:
     - `return dword_8A56160[a1 & 0x7FFF] + 16;`
   - `sub_8885954` uses alternate table:
     - `return dword_8A58ACC[a1];`
   - `sub_88858F0` selects one of the two tables, then stores result to `*(a1 + 8)`.

3. Cross-reference evidence shows only direct read use-sites for these tables.
   - `xref_query(dword_8A56160)` -> single data xref at `0x888593C` in `sub_8885934`.
   - `xref_query(dword_8A58ACC)` -> single data xref at `0x888595C` in `sub_8885954`.
   - No additional direct xrefs indicating runtime population from an external file.

4. Table values match known PZZ body sizes from your prior analysis.
   - `entry 1659` (`pl0a.pzz`) address = `0x8A57B4C`
     - `u32le = 631168` (`0x9A180`)
   - `entry 1736` (`pl0al.pzz`) address = `0x8A57C80`
     - `u32le = 609664` (`0x94D80`)

5. Tail checksum verification exists in the executable and is tied to loaded size/input.
   - `sub_88BD520(a1, a2, a3)`:
     - computes 16-byte digest via `sub_88BD728(...)`
     - compares against expected 16-byte tail with `sub_880A2A4(...) == 0`
   - `sub_88879E0` calls `sub_88BD520(a1[1], a1[2], a1[3])`.

## Full Call Chain Traced (Re-verified 2026-04-30)

The following call chain was traced instruction-by-instruction in IDA to confirm exactly
how the ROM table value controls both I/O size and tail verification.

### Step 1: File open (`sub_8885734`)

```
sub_8886D58(entry_index, &type=2, &index)  // type=2 means Z_DATA.BIN
dword_8BD0F88 = sub_8822C78(type, index)   // open AFS entry by index -> fd
```

AFS entry is opened by index. Physical offset/size come from AFS header.

### Step 2: Size lookup (`sub_88858F0`)

```
*(a1 + 8) = sub_8885934(entry_index)
           = dword_8A56160[entry_index & 0x7FFF] + 16
```

The return value is `body_size + 16 = full PZZ file size`.
This value is **hardcoded** in the ROM table, not read from AFS.

### Step 3: Sector read (`sub_8886404`)

```
sector_count = sub_888596C(*(a1+8))   // ceil(full_size / 2048)
*(a1+12) = sector_count << 11         // byte count to read
sub_882306C(fd, sector_count, buf)    // read that many sectors from disc
```

**The read size is derived entirely from the ROM table value, not from AFS entry size.**

### Step 4: Tail extraction (`sub_8885A84`)

```
body_size = *(a1 + 8) - 16           // table_value + 16 - 16 = table_value
sub_880A338(tail_buf, buf + body_size, 16)  // copy last 16 bytes as tail
```

The tail is extracted at offset = `body_size` from the loaded buffer.
If the actual PZZ file is larger than the table value, this reads from the
**wrong position** (middle of data, not the real tail).

### Step 5: Checksum verification (`sub_88BD520`)

```
sub_88BD728(buf, body_size, computed_tail)  // compute expected 16-byte digest
result = sub_880A2A4(file_tail, computed_tail, 16)  // memcmp
return result == 0;  // true if match
```

`sub_88BD728` computes a custom 16-byte digest over `body_size` bytes of data.
`sub_880A2A4` is a standard `memcmp` implementation.

If body_size is wrong (from stale ROM table), both the tail extraction position
AND the digest computation scope are wrong, causing guaranteed mismatch.

### Step 6: Failure path -> infinite loading

```
// In sub_8885A84:
v11 = dword_8BD2C5C;  // = sub_88BD520 return value
if (v11 != 0) return 1;  // success
// else v4 = 2 -> return 2

// In sub_8886404:
v2 = sub_8885988(...)  // which calls sub_8885A84
if (v2 == 1) sub_8885C34(...)  // only proceed if checksum passed
if (v2 == 2) return 1;  // return 1 = "still loading, retry"
```

Return value 1 means "continue polling" to the caller.
The upper-level state machine keeps re-dispatching the load task,
which keeps failing the same checksum, creating an **infinite loop**.

## Table Value Verification

| Entry | Name | Table Address | Stored Value | sub_8885934 Returns |
|-------|------|---------------|-------------|-------------------|
| 1659 | pl0a.pzz | `0x8A57B4C` | 631168 | 631184 (= 631168 + 16) |
| 1736 | pl0al.pzz | `0x8A57C80` | 609664 | 609680 (= 609664 + 16) |

Original `pl0a.pzz` file is 631184 bytes -> matches exactly.

## Conclusion (Re-verified)

**Confirmed: the ROM table IS the root cause of infinite loading.**

The evidence chain is:

1. `sub_8885934` reads a hardcoded body_size from `dword_8A56160` in EBOOT `.data` segment.
2. This value controls BOTH:
   - How many bytes are read from disc (sector count calculation)
   - Where the 16-byte tail is extracted from (offset = body_size)
   - The scope of the checksum digest computation (body_size bytes)
3. When a modded PZZ has a different body_size:
   - The tail is read from the wrong offset (reads data, not the actual tail)
   - The checksum digest is computed over the wrong byte range
   - `memcmp` fails -> verification returns false
   - Caller returns "retry" -> infinite loading loop
4. No runtime mechanism exists to update this table from AFS or any external file.

## Critical Discovery: sub_88BD728 Is the XOR Decryptor (2026-04-30)

**`sub_88BD520` is NOT just a "tail check" — it contains the XOR decryption pipeline.**

### sub_88BD520 actual flow

```
sub_88BD728(data_ptr, body_size, digest_out)   // XOR decrypt IN-PLACE + compute digest
sub_880A2A4(tail_ptr, digest_out, 16)          // memcmp(tail, digest, 16)
return (memcmp_result == 0)                    // true if digest matches tail
```

### sub_88BD728 internals

```
sub_88BD560(body_size, &key_byte, &key_table, ...)  // derive XOR key from body_size
// XOR-decrypt body data in-place using derived key
// simultaneously compute 16-byte digest as side-effect
```

### Why body_size MUST be exact

The XOR decryption key is derived from `body_size` via `sub_88BD560`.
If body_size is wrong:

- **Wrong key** → entire PZZ body decrypted to garbage
- Not just the tail — ALL data is corrupted
- Game may partially render (garbage data sometimes parses) then crash

This was confirmed experimentally: setting body_size to max value (917488) caused
the game to crash during battle loading because all PZZ data was mis-decrypted.

### Failed patch attempts and lessons learned

| Attempt | What | Why it failed |
|---------|------|---------------|
| NOP `sub_8885A84` entirely | Skip tail check by returning 1 immediately | **Skipped decryption entirely** — game got encrypted data |
| Hook `sub_8885934` to read fd | Read AFS size dynamically | 10 callers, many without fd → crash |
| Hook `sub_88858F0` to read fd | Read AFS size dynamically | Called at startup before fd init → crash |
| Set table to max (917488) | One-size-fits-all | **Wrong XOR key** → all data mis-decrypted → crash |
| Patch `sub_88BD520` return + exact body_size | Force verify pass, keep decryption | **SUCCESS** |

## Working CWCheat Patch (Verified 2026-04-30)

### Patch 1: Force tail verification pass (permanent, apply once)

Modifies `sub_88BD520` to keep decryption (`sub_88BD728`) but skip `memcmp` result check.
Patches the last 2 instructions before `jr $ra`:

```
Address 0x88BD550: original "xor v0, zero"  → "li v0, 1" (0x24020001)
Address 0x88BD554: original "sltiu v0, 1"   → "nop"      (0x00000000)
```

CWCheat (address = PSP_VA - 0x08800000):

```
_C1 PZZ Modding - Force verify pass (keep decryption)
_L 0x200BD550 0x24020001
_L 0x200BD554 0x00000000
```

### Patch 2: Set correct body_size per modified PZZ entry

Must match the actual PZZ file: `body_size = file_size - 16`.

Example for pl0a.pzz (entry 1659, table address `0x8A57B4C`):

```
_C1 PZZ Modding - pl0a body_size override
_L 0x20257B4C 0x0009C580
```

Where `0x0009C580` = 640384 = actual modded PZZ file size (640400) minus 16.

### CWCheat address formula

```
cwcheat_address = PSP_virtual_address - 0x08800000
```

PPSSPP `GetAddress` implementation: `(value + 0x08800000) & 0x3FFFFFFF`

### Per-entry table address formula

```
table_address = 0x8A56160 + entry_index * 4
cwcheat_address = table_address - 0x08800000
value = new_pzz_file_size - 16
```

## Dynamic Hook Analysis: Why It's Not Feasible (2026-05-01)

### Goal

Eliminate per-mod CWCheat maintenance by hooking `sub_8885934` to read actual
PZZ file size from AFS at runtime, instead of the hardcoded ROM table.

### Attempts

| Hook target | Result | Reason |
|-------------|--------|--------|
| `sub_8885934` (read AFS fd) | Crash at startup | 10 callers; save data functions call it before AFS is initialized |
| `sub_88858F0` (read AFS fd) | Crash at startup | Called during boot before fd is available |
| `sub_8885934` (read AFS partition struct) | Crash at startup | Same 10-caller problem; `dword_9BBB704[2]` not initialized yet |
| `sub_88858F0` (inline AFS struct lookup) | Infinite loading | AFS partition struct uses v0 format (u16 sector counts), not exact byte sizes |

### Root Cause: AFS Partition Struct Cannot Provide Exact body_size

Verified at runtime via PPSSPP Memory Viewer:

```
dword_9BBB704[2] = 0x08B81240  (Z_DATA partition struct pointer)
*(struct + 15)   = 0x00         (format flag = v0 = u16 sector count array)
*(struct + 8)    = 0x00000A5B   (entry count = 2651, matches Z_DATA.BIN)
```

For v0 format, the partition struct stores:
- `u16` array at `struct + 282`, indexed by entry_index
- Each value = **sector count** (ceil(file_size / 2048))
- `sector_count << 11` = sector-aligned size ≠ exact file_size

Example for pl0a.pzz:
- Exact file_size: 631184 bytes
- Sector count: 309 → 309 * 2048 = 632832 (difference: +1648 bytes)

**XOR decryption key is derived from exact body_size via `sub_88BD560`.**
Off by even 1 byte → completely wrong key → entire PZZ decrypted to garbage.

### Conclusion

**Dynamic hook is not feasible.** The only source of exact body_size at runtime
is the ROM table (`dword_8A56160`). This table must contain the correct value
for each modified PZZ.

## Two Available Patching Routes

### Route 1: Modify Decrypted EBOOT Binary (Recommended)

Directly patch the decrypted `NPJH50107_gvsgnextpsp.BIN`, rename to `EBOOT.BIN`,
let PPSSPP load it instead of the encrypted version.

**Patches:**

1. Verify-pass (permanent, one-time):
   - File offset `0x0BD504`: write `0x24020001` (li v0, 1)
   - File offset `0x0BD508`: write `0x00000000` (nop)

2. Per-modified-PZZ table value:
   - File offset = `0x085544 + entry_index * 4`
   - Value = `new_pzz_file_size - 16` (u32 little-endian)

**Pros:** No cheat file needed; PPSSPP loads the patched EBOOT directly.
**Cons:** Cannot add NEW PZZ entries beyond the original 2651 count.
Modding tool must update the EBOOT every time a PZZ body_size changes.

### Route 2: CWCheat Runtime Patch

Keep the original EBOOT; use CWCheat `.ini` file for patches.

**Patches:**

1. Verify-pass (permanent):
   ```
   _C1 PZZ Modding - Force verify pass
   _L 0x200BD550 0x24020001
   _L 0x200BD554 0x00000000
   ```

2. Per-modified-PZZ table value:
   ```
   _C1 PZZ Modding - pl0a body_size
   _L 0x20257B4C 0x0009C580
   ```
   Address formula: `0x20000000 | ((0x8A56160 + entry_index * 4) - 0x08800000)`
   Value: `new_pzz_file_size - 16`

**Pros:** Does not modify any game files; easy to toggle on/off.
**Cons:** CWCheat file must be updated every time a PZZ body_size changes.
Cannot add NEW PZZ entries beyond the original 2651 count.

### Shared Limitation

Both routes can only modify **existing** PZZ entries (indices 0-2650).
Adding entirely new PZZ entries to Z_DATA.BIN would require additional
EBOOT modifications (entry count, partition init, ROM table expansion)
which is a significantly larger reverse-engineering effort.

## What This Is NOT

- This is NOT an AFS size constraint (AFS header stores its own offset/size independently).
- This is NOT just a checksum issue — body_size controls XOR **decryption key derivation**.
- This is NOT a PPSSPP emulator issue (PPSSPP faithfully executes the game's own EBOOT code).
- Dynamic AFS-based hook is NOT feasible (partition struct only stores sector counts, not exact byte sizes).

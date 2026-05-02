# EBOOT Hook Patch Strategy for PZZ Size Overrides

## Root Cause Recap

- `sub_8885934` returns `dword_8A56160[idx] + 0x10`.
- That return value is treated as the expected full PZZ size (`body_size + 16`).
- When a modded PZZ grows, EBOOT still uses the old hardcoded value, causing wrong XOR/keying/checksum flow and infinite loading.

## Hook Design

### What gets hooked

- Function: `sub_8885934` at `0x08885934`
- Old behavior: read **table A** (`0x08A56160`) and add `+0x10`.
- New behavior: read **table B** (`0x08A58ACC`) directly.

### Why this design

- Keep the original table A untouched.
- Pre-fill table B from table A (`table_b[i] = table_a[i] + 0x10`) so baseline behavior stays identical.
- Apply per-entry overrides only in table B for modified resources.

This gives a stable “hooked lookup path” without touching unrelated loader logic.

## Implemented Tool

- Script: `eboot_pzz_hook_patch.py`
- It performs:
  1. ELF32 PT_LOAD VA->file mapping
  2. `sub_8885934` instruction patch
  3. table B initialization from table A (`+16`)
  4. optional entry overrides (`entry -> new final size`)

## Hooked Instruction Stream

Patched function bytes at `0x08885934`:

```text
0x30827FFF  andi  v0, a0, 0x7FFF
0x00021880  sll   v1, v0, 2
0x3C0208A5  lui   v0, 0x08A5
0x24428ACC  addiu v0, v0, 0x8ACC
0x00431021  addu  v0, v0, v1
0x8C420000  lw    v0, 0(v0)
0x03E00008  jr    ra
0x00000000  nop
```

## Usage

### 1) Hook install only (no overrides)

```bash
python eboot_pzz_hook_patch.py \
  --eboot EBOOT.BIN \
  --out EBOOT_hooked.BIN
```

### 2) Hook + override by final PZZ file size

```bash
python eboot_pzz_hook_patch.py \
  --eboot EBOOT.BIN \
  --out EBOOT_hooked.BIN \
  --override-size 1659=631184
```

### 3) Hook + override by body size (`final = body + 16`)

```bash
python eboot_pzz_hook_patch.py \
  --eboot EBOOT.BIN \
  --out EBOOT_hooked.BIN \
  --override-body 1659=631168
```

### 4) Hook + override using modded file size

```bash
python eboot_pzz_hook_patch.py \
  --eboot EBOOT.BIN \
  --out EBOOT_hooked.BIN \
  --override-file 1659=pl0a_mod.pzz
```

## Validation Checklist

1. Confirm script output reports `status: ok`.
2. Confirm `override_count` matches expected modified entries.
3. Rebuild/reinsert patched EBOOT and modded PZZ into your test image.
4. Re-test the known failing case (`add_pcube1_bind_m11`).
5. If multiple PZZ entries are expanded, add all of them as overrides.

## Notes

- Entry index is 1-based (matches the ROM table lookup path).
- The tool requires a decrypted ELF-format EBOOT/PRX image (not encrypted container form).
- Final size overrides must be 16-byte aligned and `>= 16`.

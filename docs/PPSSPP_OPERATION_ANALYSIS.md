# PPSSPP Operation Analysis for This Issue

## Goal

Determine how PPSSPP handles the relevant operations and whether PPSSPP itself is doing the `PZZ body_size + tail checksum` logic.

## Sources Reviewed

- `Core/HLE/sceIo.cpp` (PPSSPP upstream source)
- `Core/FileSystems/ISOFileSystem.cpp` (PPSSPP upstream source)
- `Core/HLE/sceKernelModule.cpp` (PPSSPP upstream source)
- `Core/ELF/PrxDecrypter.cpp` (PPSSPP upstream source)

## What PPSSPP Actually Does

### 1) Generic file read path (`sceIoRead`)

PPSSPP's `sceIoRead` route calls `__IoRead`, which eventually calls:

- `pspFileSystem.ReadFile(...)` for normal files
- `npdrmRead(...)` only when NPDRM flag is enabled

This means PPSSPP is emulating PSP file APIs and returning bytes; it does not parse your game's custom PZZ format here.

### 2) ISO/UMD read path

`ISOFileSystem::ReadFile` reads sectors/blocks from `blockDevice` and copies bytes into output buffers.  
No game-specific container parser (such as AFS/PZZ semantics) is built into this layer.

### 3) Executable/PRX decryption path

In `sceKernelModule.cpp`, if module magic is `~PSP`, PPSSPP calls:

- `pspDecryptPRX(...)`

`PrxDecrypter.cpp` implements PRX type decryption (Type 0/1/2/5/6), i.e. executable/module crypto handling.

### 4) NPDRM PGD path

In `sceIo.cpp`, `ioctl 0x04100001` handles PGD decryption setup:

- reads PGD header
- opens/decrypts via `pgd_open(...)`
- uses `npdrmRead(...)` to decrypt blocks on read

This is NPDRM encrypted data handling, not your game-specific PZZ tail algorithm.

## What PPSSPP Does NOT Appear to Do

- No evidence in reviewed PPSSPP code paths that PPSSPP reimplements your game's custom:
  - `dword_8A56160` body-size lookup policy
  - custom PZZ XOR/tail verification routine

## Conclusion for Your Infinite Loading Case

For this title, the failing logic is still in **game code (EBOOT)** running inside the emulator, not in PPSSPP's generic I/O layer.

So behavior parity is expected:

- If your rebuilt `pl0a.pzz` body size conflicts with EBOOT table expectations, the game can hang in PPSSPP the same way it does on real PSP.

## Practical Debugging Strategy in PPSSPP

Use PPSSPP as an execution/debug platform, but target the game code path:

1. Break around your known EBOOT functions (`sub_8885934`, `sub_88858F0`, `sub_88BD520`).
2. Compare runtime values:
   - table-derived expected body size
   - actual loaded body span
   - computed tail vs file tail
3. Confirm whether mismatch happens before/at checksum compare.

This gives runtime proof without changing your reconstruction pipeline first.

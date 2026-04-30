#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import struct
from pathlib import Path
from typing import Dict, List, Tuple


SUB_8885934_VA = 0x08885934
ROM_TABLE_A_VA = 0x08A56160
ROM_TABLE_B_VA = 0x08A58ACC
ROM_TABLE_ENTRY_COUNT = 2651


def _require(condition: bool, message: str) -> None:
    if not condition:
        raise RuntimeError(message)


def _parse_elf32_load_segments(blob: bytes) -> List[Tuple[int, int, int, int]]:
    _require(len(blob) >= 0x34, "Input file is too small to be ELF32.")
    _require(blob[0:4] == b"\x7fELF", "Input is not an ELF file.")
    _require(blob[4] == 1, "Only ELF32 is supported.")
    _require(blob[5] == 1, "Only little-endian ELF is supported.")

    e_phoff = struct.unpack_from("<I", blob, 0x1C)[0]
    e_phentsize = struct.unpack_from("<H", blob, 0x2A)[0]
    e_phnum = struct.unpack_from("<H", blob, 0x2C)[0]

    _require(e_phentsize >= 0x20, f"Unexpected ELF program header size: {e_phentsize}.")
    _require(e_phoff > 0 and e_phnum > 0, "ELF has no program headers.")
    _require(e_phoff + e_phentsize * e_phnum <= len(blob), "Program header table exceeds file size.")

    segments: List[Tuple[int, int, int, int]] = []
    for i in range(e_phnum):
        off = e_phoff + i * e_phentsize
        p_type, p_offset, p_vaddr, _, p_filesz, _, _, _ = struct.unpack_from("<IIIIIIII", blob, off)
        if p_type != 1:
            continue
        if p_filesz == 0:
            continue
        _require(p_offset + p_filesz <= len(blob), f"PT_LOAD {i} exceeds file size.")
        segments.append((p_vaddr, p_vaddr + p_filesz, p_offset, p_offset + p_filesz))

    _require(bool(segments), "No PT_LOAD segment with file payload found.")
    return segments


def _va_to_off(segments: List[Tuple[int, int, int, int]], va: int, size: int = 4) -> int:
    for seg_v_start, seg_v_end, seg_f_start, _ in segments:
        if va >= seg_v_start and va + size <= seg_v_end:
            return seg_f_start + (va - seg_v_start)
    raise RuntimeError(f"VA 0x{va:08X} (size {size}) is not mapped in any PT_LOAD file range.")


def _read_u32_va(blob: bytearray, segments: List[Tuple[int, int, int, int]], va: int) -> int:
    off = _va_to_off(segments, va, 4)
    return struct.unpack_from("<I", blob, off)[0]


def _write_u32_va(blob: bytearray, segments: List[Tuple[int, int, int, int]], va: int, value: int) -> None:
    off = _va_to_off(segments, va, 4)
    struct.pack_into("<I", blob, off, value & 0xFFFFFFFF)


def _parse_assignment(raw: str) -> Tuple[int, str]:
    _require("=" in raw, f"Invalid assignment '{raw}'. Use ENTRY=VALUE.")
    left, right = raw.split("=", 1)
    left = left.strip()
    right = right.strip()
    _require(left, f"Missing ENTRY in '{raw}'.")
    _require(right, f"Missing VALUE in '{raw}'.")
    try:
        entry = int(left, 0)
    except ValueError as exc:
        raise RuntimeError(f"ENTRY must be an integer in '{raw}'.") from exc
    return entry, right


def _parse_size_value(raw: str) -> int:
    try:
        value = int(raw, 0)
    except ValueError as exc:
        raise RuntimeError(f"SIZE must be integer bytes, got '{raw}'.") from exc
    _require(value > 0, f"SIZE must be > 0, got {value}.")
    return value


def _collect_overrides(args: argparse.Namespace) -> Dict[int, int]:
    overrides: Dict[int, int] = {}

    def insert(entry: int, size: int, source: str) -> None:
        _require(entry >= 1, f"ENTRY must be 1-based and >= 1 ({source}).")
        _require(size >= 16, f"Final stream size must be >= 16 bytes ({source}).")
        _require(size % 16 == 0, f"Final stream size must be 16-byte aligned ({source}).")
        if entry in overrides:
            raise RuntimeError(f"Duplicate override for ENTRY={entry}.")
        overrides[entry] = size

    for raw in args.override_size:
        entry, size_raw = _parse_assignment(raw)
        insert(entry, _parse_size_value(size_raw), raw)

    for raw in args.override_body:
        entry, body_raw = _parse_assignment(raw)
        body_size = _parse_size_value(body_raw)
        insert(entry, body_size + 16, raw)

    for raw in args.override_file:
        entry, file_raw = _parse_assignment(raw)
        file_path = Path(file_raw)
        _require(file_path.exists(), f"Override file does not exist: {file_path}")
        _require(file_path.is_file(), f"Override path is not a file: {file_path}")
        insert(entry, file_path.stat().st_size, raw)

    return overrides


def _install_hook_and_tables(
    blob: bytearray,
    segments: List[Tuple[int, int, int, int]],
    table_count: int,
    overrides_final_sizes: Dict[int, int],
) -> List[Dict[str, int]]:
    _require(table_count > 0, "table_count must be > 0.")

    for idx in range(table_count):
        body_size = _read_u32_va(blob, segments, ROM_TABLE_A_VA + idx * 4)
        _write_u32_va(blob, segments, ROM_TABLE_B_VA + idx * 4, body_size + 16)

    applied: List[Dict[str, int]] = []
    for entry, final_size in sorted(overrides_final_sizes.items()):
        _require(entry <= table_count, f"ENTRY {entry} exceeds table_count {table_count}.")
        idx = entry - 1
        target_va = ROM_TABLE_B_VA + idx * 4
        old_value = _read_u32_va(blob, segments, target_va)
        _write_u32_va(blob, segments, target_va, final_size)
        applied.append(
            {
                "entry": entry,
                "index0": idx,
                "table_b_va": target_va,
                "old_value": old_value,
                "new_value": final_size,
            }
        )

    # Hook sub_8885934:
    # return table_b[idx] directly (already includes +16).
    patched_insns = [
        0x30827FFF,  # andi  v0, a0, 0x7FFF
        0x00021880,  # sll   v1, v0, 2
        0x3C0208A5,  # lui   v0, 0x08A5
        0x24428ACC,  # addiu v0, v0, 0x8ACC
        0x00431021,  # addu  v0, v0, v1
        0x8C420000,  # lw    v0, 0(v0)
        0x03E00008,  # jr    ra
        0x00000000,  # nop
    ]
    for i, insn in enumerate(patched_insns):
        _write_u32_va(blob, segments, SUB_8885934_VA + i * 4, insn)

    # Verify patched instruction stream.
    for i, expected in enumerate(patched_insns):
        actual = _read_u32_va(blob, segments, SUB_8885934_VA + i * 4)
        _require(
            actual == expected,
            f"Instruction verify failed at VA 0x{SUB_8885934_VA + i * 4:08X}: 0x{actual:08X} != 0x{expected:08X}",
        )

    return applied


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Install a ROM-table hook for sub_8885934 and write PZZ final sizes into table B.\n"
            "This keeps table A untouched and makes size overrides patch-friendly."
        )
    )
    parser.add_argument("--eboot", required=True, help="Input decrypted ELF EBOOT/PRX path.")
    parser.add_argument("--out", required=True, help="Output patched ELF path.")
    parser.add_argument(
        "--table-count",
        type=int,
        default=ROM_TABLE_ENTRY_COUNT,
        help=f"ROM table entry count (default: {ROM_TABLE_ENTRY_COUNT}).",
    )
    parser.add_argument(
        "--override-size",
        action="append",
        default=[],
        help="Override final PZZ file size by entry index: ENTRY=SIZE_BYTES (can repeat).",
    )
    parser.add_argument(
        "--override-body",
        action="append",
        default=[],
        help="Override PZZ body size by entry index: ENTRY=BODY_BYTES (final size = body + 16).",
    )
    parser.add_argument(
        "--override-file",
        action="append",
        default=[],
        help="Override by file size: ENTRY=PATH_TO_PZZ_FILE (can repeat).",
    )
    return parser


def main() -> None:
    parser = _build_parser()
    args = parser.parse_args()

    in_path = Path(args.eboot)
    out_path = Path(args.out)
    _require(in_path.exists(), f"Input file does not exist: {in_path}")
    _require(in_path.is_file(), f"Input path is not a file: {in_path}")

    overrides = _collect_overrides(args)
    blob = bytearray(in_path.read_bytes())
    segments = _parse_elf32_load_segments(blob)
    applied = _install_hook_and_tables(blob, segments, args.table_count, overrides)

    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_bytes(blob)

    result = {
        "status": "ok",
        "input": str(in_path),
        "output": str(out_path),
        "hook_va": f"0x{SUB_8885934_VA:08X}",
        "table_a_va": f"0x{ROM_TABLE_A_VA:08X}",
        "table_b_va": f"0x{ROM_TABLE_B_VA:08X}",
        "table_count": args.table_count,
        "override_count": len(applied),
        "overrides": applied,
    }
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()

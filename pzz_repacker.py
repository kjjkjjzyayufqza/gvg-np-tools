#!/usr/bin/env python3
# ⚠️ DEPRECATED — 请使用 rust_converter/ 中的 Rust 版本
# 此文件仅保留作为参考，不再维护。
from __future__ import annotations

import json
import struct
import zlib
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

from gvg_converter import xor_dec, find_pzz_key, extract_pzz_streams, classify_stream


def compress_stream(data: bytes) -> bytes:
    return zlib.compress(data, level=6)


def build_pzz(streams: List[bytes], original_key: Optional[int] = None) -> bytes:
    compressed = [compress_stream(s) for s in streams]

    fc = len(streams)
    table_entries = 1 + fc
    table_bytes = table_entries * 4
    padded_table = (table_bytes + 0x7FF) & ~0x7FF

    data_start = padded_table
    offsets = []
    current = data_start
    for c in compressed:
        offsets.append(current)
        current += len(c)
        while current % 4 != 0:
            current += 1

    total_size = current
    buf = bytearray(total_size)

    struct.pack_into("<I", buf, 0, fc)
    for i, off in enumerate(offsets):
        struct.pack_into("<I", buf, (i + 1) * 4, off)

    for i, c in enumerate(compressed):
        buf[offsets[i]:offsets[i] + len(c)] = c

    if original_key is not None:
        key = original_key
    else:
        key = struct.unpack_from("<I", buf, 0)[0] ^ fc
        key = key if key != 0 else 0x12345678

    enc = xor_dec(bytes(buf), key)
    return enc


def repack_pzz_with_replaced_stream(
    original_pzz_data: bytes,
    stream_index: int,
    new_stream_data: bytes,
) -> bytes:
    key = find_pzz_key(original_pzz_data, len(original_pzz_data))
    if key is None:
        raise ValueError("Cannot find PZZ key")

    streams = extract_pzz_streams(original_pzz_data)
    if not streams:
        raise ValueError("No streams in PZZ")

    if stream_index >= len(streams):
        raise ValueError(f"Stream index {stream_index} out of range (max {len(streams) - 1})")

    streams[stream_index] = new_stream_data
    return build_pzz(streams, original_key=key)


def patch_afs_entry(
    afs_path: Path,
    entry_index: int,
    new_data: bytes,
    output_path: Path,
) -> Dict[str, Any]:
    original = bytearray(afs_path.read_bytes())
    file_count = struct.unpack_from("<I", original, 4)[0]

    if entry_index >= file_count:
        return {"error": f"Entry index {entry_index} out of range (max {file_count - 1})"}

    table_offset = 8
    entry_off_pos = table_offset + entry_index * 8
    old_offset = struct.unpack_from("<I", original, entry_off_pos)[0]
    old_size = struct.unpack_from("<I", original, entry_off_pos + 4)[0]

    if len(new_data) <= old_size:
        result = bytearray(original)
        result[old_offset:old_offset + len(new_data)] = new_data
        for i in range(len(new_data), old_size):
            result[old_offset + i] = 0
        struct.pack_into("<I", result, entry_off_pos + 4, len(new_data))
        output_path.write_bytes(bytes(result))
        return {
            "status": "in_place",
            "entry_index": entry_index,
            "old_size": old_size,
            "new_size": len(new_data),
            "offset": old_offset,
            "output": str(output_path),
        }
    else:
        new_offset = len(original)
        while new_offset % 2048 != 0:
            new_offset += 1

        result = bytearray(original)
        result.extend(b'\x00' * (new_offset - len(original)))
        result.extend(new_data)

        padding = (2048 - (len(result) % 2048)) % 2048
        result.extend(b'\x00' * padding)

        struct.pack_into("<I", result, entry_off_pos, new_offset)
        struct.pack_into("<I", result, entry_off_pos + 4, len(new_data))

        output_path.write_bytes(bytes(result))
        return {
            "status": "appended",
            "entry_index": entry_index,
            "old_offset": old_offset,
            "old_size": old_size,
            "new_offset": new_offset,
            "new_size": len(new_data),
            "output": str(output_path),
            "final_size": len(result),
        }


def full_repack(
    z_data_path: Path,
    inventory_path: Path,
    pzz_name: str,
    new_pzz_data: bytes,
    output_path: Path,
) -> Dict[str, Any]:
    inv = json.loads(inventory_path.read_text(encoding="utf-8"))
    entries = inv.get("entries", [])

    target = None
    for e in entries:
        if e.get("name", "").lower() == pzz_name.lower():
            target = e
            break

    if target is None:
        return {"error": f"Entry '{pzz_name}' not found in inventory"}

    entry_index = target["index"]
    return patch_afs_entry(z_data_path, entry_index, new_pzz_data, output_path)


if __name__ == "__main__":
    import argparse

    ap = argparse.ArgumentParser(description="PZZ repacker and AFS patcher")
    sub = ap.add_subparsers(dest="cmd")

    p_repack = sub.add_parser("repack", help="Repack streams into PZZ")
    p_repack.add_argument("stream_dir", help="Directory with stream files")
    p_repack.add_argument("--manifest", required=True, help="streams_manifest.json")
    p_repack.add_argument("--out", required=True, help="Output PZZ file")
    p_repack.add_argument("--original-pzz", help="Original PZZ for key extraction")

    p_patch = sub.add_parser("patch", help="Patch AFS with new PZZ data")
    p_patch.add_argument("afs", help="AFS file (Z_DATA.BIN)")
    p_patch.add_argument("pzz", help="New PZZ file")
    p_patch.add_argument("--inventory", required=True, help="Inventory JSON")
    p_patch.add_argument("--name", required=True, help="PZZ entry name (e.g. pl00.pzz)")
    p_patch.add_argument("--out", required=True, help="Output AFS file")

    args = ap.parse_args()

    if args.cmd == "repack":
        manifest = json.loads(Path(args.manifest).read_text(encoding="utf-8"))
        stream_dir = Path(args.stream_dir)
        streams_info = manifest["streams"]

        streams = []
        for si in sorted(streams_info, key=lambda x: x["index"]):
            sf = stream_dir / Path(si["file"]).name
            if sf.exists():
                streams.append(sf.read_bytes())
            else:
                streams.append(b'\x00' * 16)

        key = None
        if args.original_pzz:
            orig = Path(args.original_pzz).read_bytes()
            key = find_pzz_key(orig, len(orig))

        pzz_data = build_pzz(streams, original_key=key)
        Path(args.out).write_bytes(pzz_data)
        print(f"Repacked PZZ: {args.out} ({len(pzz_data)} bytes, {len(streams)} streams)")

    elif args.cmd == "patch":
        pzz_data = Path(args.pzz).read_bytes()
        result = full_repack(
            Path(args.afs), Path(args.inventory),
            args.name, pzz_data, Path(args.out)
        )
        print(json.dumps(result, indent=2, default=str))

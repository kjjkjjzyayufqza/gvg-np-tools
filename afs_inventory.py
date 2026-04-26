#!/usr/bin/env python3
"""
AFS inventory generator for DATA.BIN files.

Outputs a JSON manifest including:
  - file_count
  - entries: index, name, offset, size, magic16_hex
  - summary: by extension and by magic4

This implementation is streaming-friendly: it does not load the whole file.
"""

from __future__ import annotations

import argparse
import json
import os
import struct
from collections import Counter, defaultdict
from pathlib import Path
from typing import Dict, List, Optional, Tuple


def read_u32_le(f, offset: int) -> int:
    f.seek(offset, os.SEEK_SET)
    b = f.read(4)
    if len(b) != 4:
        return 0
    return struct.unpack("<I", b)[0]


def read_bytes(f, offset: int, size: int) -> bytes:
    f.seek(offset, os.SEEK_SET)
    return f.read(size)


def c_string(buf: bytes) -> str:
    pos = buf.find(b"\x00")
    if pos >= 0:
        buf = buf[:pos]
    return buf.decode("ascii", errors="replace")


def safe_ext(name: str) -> str:
    name = name.lower()
    if "." in name:
        return name.rsplit(".", 1)[-1]
    return ""


def parse_afs_stream(path: Path, max_name_len: int = 0x20) -> Dict:
    size = path.stat().st_size
    with path.open("rb") as f:
        magic = read_bytes(f, 0, 4)
        if magic[:3] != b"AFS":
            return {"file": str(path), "error": "not_afs", "magic4": magic.decode("ascii", "replace")}

        file_count = read_u32_le(f, 4)
        table_off = 8
        table_size = file_count * 8
        if table_off + table_size + 8 > size:
            return {
                "file": str(path),
                "error": "afs_table_out_of_range",
                "file_count": int(file_count),
                "file_size": int(size),
            }

        entries: List[Dict] = []
        for i in range(file_count):
            off = read_u32_le(f, table_off + i * 8)
            sz = read_u32_le(f, table_off + i * 8 + 4)
            entries.append({"index": int(i), "offset": int(off), "size": int(sz)})

        name_off = read_u32_le(f, table_off + table_size)
        name_size = read_u32_le(f, table_off + table_size + 4)
        name_table = None
        if 0 < name_off < size and 0 < name_size <= size - name_off:
            if name_size >= file_count * 0x30:
                name_table = {"offset": int(name_off), "size": int(name_size), "entry_size": 0x30}

        # Apply names if possible
        if name_table:
            base = name_table["offset"]
            for i in range(file_count):
                row_off = base + i * 0x30
                if row_off + 0x30 > size:
                    break
                row = read_bytes(f, row_off, 0x30)
                nm = c_string(row[:max_name_len]).lower()
                if nm:
                    entries[i]["name"] = nm

        # Add magic16 for each entry (best effort)
        for e in entries:
            off = e["offset"]
            if 0 <= off < size:
                m16 = read_bytes(f, off, 16)
            else:
                m16 = b""
            e["magic16_hex"] = m16.hex()
            e["magic4_ascii"] = (m16[:4] if len(m16) >= 4 else b"").decode("ascii", "replace")
            nm = e.get("name", f"{e['index']:04d}.bin")
            e["ext"] = safe_ext(nm)

        return {
            "file": str(path),
            "magic": magic.decode("ascii", "replace"),
            "file_size": int(size),
            "file_count": int(file_count),
            "table_offset": int(table_off),
            "name_table": name_table,
            "entries": entries,
        }


def build_summary(entries: List[Dict]) -> Dict:
    by_ext = Counter()
    by_magic4 = Counter()
    for e in entries:
        by_ext[e.get("ext", "")] += 1
        by_magic4[e.get("magic4_ascii", "")] += 1

    def top(counter: Counter, n: int = 50) -> List[Dict]:
        return [{"key": k, "count": int(v)} for k, v in counter.most_common(n)]

    return {
        "by_ext_top": top(by_ext),
        "by_magic4_top": top(by_magic4),
    }


def main() -> int:
    ap = argparse.ArgumentParser(description="Generate AFS inventory JSON")
    ap.add_argument("file", help="Path to AFS container (e.g. W_DATA.BIN)")
    ap.add_argument("--out", required=True, help="Output JSON path")
    ap.add_argument("--limit", type=int, default=0, help="Limit number of entries in output (0 = all)")
    args = ap.parse_args()

    path = Path(args.file)
    inv = parse_afs_stream(path)
    if "entries" in inv:
        entries = inv["entries"]
        if args.limit and args.limit > 0:
            inv["entries"] = entries[: args.limit]
        inv["summary"] = build_summary(entries)

    Path(args.out).write_text(json.dumps(inv, ensure_ascii=False, indent=2), encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())


#!/usr/bin/env python3
"""
Extract a few named entries from an AFS and scan for embedded magics.

This helps answer: where are model/texture assets stored?
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Dict, List

from mwo3lib import afs_extract, parse_afs


MAGICS = [
    b"GMO\x00",
    b"GIM\x00",
    b"PMO\x00",
    b"TIM2",
    b"AFS\x00",
    b"PSMF",
    b"RIFF",
    b"DDS ",
    b"VAGp",
]


def scan_bytes(data: bytes, limit_each: int = 20) -> Dict[str, List[int]]:
    out: Dict[str, List[int]] = {}
    for m in MAGICS:
        key = m.decode("ascii", errors="replace")
        hits = []
        start = 0
        while len(hits) < limit_each:
            idx = data.find(m, start)
            if idx == -1:
                break
            hits.append(idx)
            start = idx + 1
        if hits:
            out[key] = hits
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description="Extract named AFS entries and scan magics")
    ap.add_argument("--afs", required=True, help="AFS file path (e.g. Z_DATA.BIN)")
    ap.add_argument("--out-dir", required=True, help="Output directory for extracted files")
    ap.add_argument("--names", nargs="+", required=True, help="Entry names to extract (e.g. basic.pzz)")
    ap.add_argument("--report", required=True, help="Output report JSON")
    args = ap.parse_args()

    afs_path = Path(args.afs)
    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)

    afs = parse_afs(afs_path)
    if "error" in afs:
        Path(args.report).write_text(json.dumps(afs, ensure_ascii=False, indent=2), encoding="utf-8")
        return 1

    report = {"afs": str(afs_path), "items": []}
    for name in args.names:
        out_file = out_dir / name
        info = afs_extract(afs_path, name, out_file)
        item = {"name": name, "extract": info}
        if "error" not in info:
            data = out_file.read_bytes()
            item["size"] = len(data)
            item["magic4"] = data[:4].decode("ascii", errors="replace")
            item["embedded_magics"] = scan_bytes(data)
        report["items"].append(item)

    Path(args.report).write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"Wrote report: {args.report}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())


#!/usr/bin/env python3
"""
Batch inventory generator for X/Y/Z/W_DATA.BIN.

It searches for these files under a root directory (default: current working dir),
generates one JSON per file, and an aggregate summary JSON.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Dict, List

from afs_inventory import parse_afs_stream, build_summary


TARGETS = ["X_DATA.BIN", "Y_DATA.BIN", "Z_DATA.BIN", "W_DATA.BIN"]


def find_targets(root: Path) -> Dict[str, List[Path]]:
    found: Dict[str, List[Path]] = {t: [] for t in TARGETS}
    for p in root.rglob("*.BIN"):
        if not p.is_file():
            continue
        name = p.name.upper()
        if name in found:
            found[name].append(p)
    return found


def main() -> int:
    ap = argparse.ArgumentParser(description="Inventory X/Y/Z/W_DATA.BIN to JSON")
    ap.add_argument("--root", default=".", help="Root directory to search")
    ap.add_argument("--out-dir", default="test/data_bin_inventory", help="Output directory")
    args = ap.parse_args()

    root = Path(args.root).resolve()
    out_dir = Path(args.out_dir).resolve()
    out_dir.mkdir(parents=True, exist_ok=True)

    found = find_targets(root)
    outputs = []
    for name, paths in found.items():
        if not paths:
            outputs.append({"target": name, "found": 0})
            continue
        for p in paths:
            inv = parse_afs_stream(p)
            if "entries" in inv:
                inv["summary"] = build_summary(inv["entries"])
            out_path = out_dir / f"{p.name}.inventory.json"
            out_path.write_text(json.dumps(inv, ensure_ascii=False, indent=2), encoding="utf-8")
            outputs.append({"target": name, "path": str(p), "out": str(out_path), "error": inv.get("error")})

    agg = {"root": str(root), "out_dir": str(out_dir), "results": outputs}
    (out_dir / "_aggregate.json").write_text(json.dumps(agg, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"Wrote: {out_dir}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())


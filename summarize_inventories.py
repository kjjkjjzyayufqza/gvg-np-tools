#!/usr/bin/env python3
"""
Summarize generated *.inventory.json files.

Reads inventory JSONs and prints a compact report, also writes summary JSON.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Dict, List


def main() -> int:
    ap = argparse.ArgumentParser(description="Summarize DATA.BIN inventories")
    ap.add_argument("--dir", required=True, help="Directory containing *.inventory.json")
    ap.add_argument("--out", required=True, help="Output summary JSON path")
    args = ap.parse_args()

    inv_dir = Path(args.dir)
    items: List[Dict] = []
    for p in sorted(inv_dir.glob("*.inventory.json")):
        try:
            j = json.loads(p.read_text(encoding="utf-8"))
        except Exception as e:
            items.append({"file": str(p), "error": str(e)})
            continue
        items.append(
            {
                "file": j.get("file"),
                "file_count": j.get("file_count"),
                "file_size": j.get("file_size"),
                "magic": j.get("magic"),
                "ext_top10": (j.get("summary") or {}).get("by_ext_top", [])[:10],
                "magic4_top10": (j.get("summary") or {}).get("by_magic4_top", [])[:10],
            }
        )

    out = {"dir": str(inv_dir), "items": items}
    Path(args.out).write_text(json.dumps(out, ensure_ascii=False, indent=2), encoding="utf-8")

    # Windows console encoding may be non-UTF8 (e.g. GBK). Avoid crashing on print.
    def safe_print(s: str) -> None:
        try:
            print(s)
        except UnicodeEncodeError:
            print(s.encode("utf-8", errors="replace").decode("utf-8", errors="replace"))

    for it in items:
        safe_print(f"== {Path(it.get('file','?')).name} ==")
        if it.get("error"):
            safe_print(f"error: {it['error']}")
            continue
        safe_print(f"file_count={it.get('file_count')} file_size={it.get('file_size')} magic={it.get('magic')}")
        safe_print("ext_top10=" + json.dumps(it.get("ext_top10", []), ensure_ascii=False))
        safe_print("magic4_top10=" + json.dumps(it.get("magic4_top10", []), ensure_ascii=False))

    return 0


if __name__ == "__main__":
    raise SystemExit(main())


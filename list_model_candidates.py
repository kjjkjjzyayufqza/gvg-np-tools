#!/usr/bin/env python3
"""
List likely model-related resources from Z_DATA.BIN inventory.

Heuristic:
  - pl??.pzz and pl??l.pzz are large and appear to be character/unit bundles.
  - This script extracts those entries and writes a JSON report with indices,
    offsets, sizes, and name patterns.
"""

from __future__ import annotations

import argparse
import json
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, List, Optional


PL_RE = re.compile(r"^pl([0-9a-f]{2})(l)?\.pzz$", re.IGNORECASE)


@dataclass(frozen=True)
class Candidate:
    index: int
    name: str
    offset: int
    size: int
    pl_id_hex: str
    variant: str

    def to_json(self) -> Dict[str, Any]:
        return {
            "index": self.index,
            "name": self.name,
            "offset": self.offset,
            "size": self.size,
            "pl_id_hex": self.pl_id_hex,
            "variant": self.variant,
        }


def load_inventory(inv_path: Path) -> List[Dict[str, Any]]:
    j = json.loads(inv_path.read_text(encoding="utf-8"))
    entries = j.get("entries", [])
    if not isinstance(entries, list):
        raise ValueError("Invalid inventory JSON: entries is not a list")
    return [e for e in entries if isinstance(e, dict)]


def main(argv: Optional[List[str]] = None) -> int:
    ap = argparse.ArgumentParser(description="List Z_DATA model candidate entries (pl??*.pzz)")
    ap.add_argument(
        "--inv",
        default="test/data_bin_inventory/Z_DATA.BIN.inventory.json",
        help="Path to Z_DATA.BIN.inventory.json",
    )
    ap.add_argument(
        "--out",
        default="test/model_candidates_pl_pzz.json",
        help="Output JSON path",
    )
    ap.add_argument(
        "--min-size",
        type=int,
        default=200_000,
        help="Minimum size in bytes to include (default: 200000)",
    )
    args = ap.parse_args(argv)

    inv_path = Path(args.inv)
    out_path = Path(args.out)
    entries = load_inventory(inv_path)

    cands: List[Candidate] = []
    for e in entries:
        name = e.get("name")
        if not isinstance(name, str):
            continue
        m = PL_RE.match(name)
        if not m:
            continue
        size = int(e.get("size", 0))
        if size < args.min_size:
            continue
        idx = int(e.get("index", -1))
        off = int(e.get("offset", -1))
        if idx < 0 or off < 0:
            continue
        pl_id_hex = m.group(1).lower()
        variant = "lod_or_alt" if m.group(2) else "base"
        cands.append(
            Candidate(
                index=idx,
                name=name,
                offset=off,
                size=size,
                pl_id_hex=pl_id_hex,
                variant=variant,
            )
        )

    cands.sort(key=lambda c: (c.pl_id_hex, c.variant, c.index))

    report = {
        "inventory": str(inv_path),
        "min_size": args.min_size,
        "count": len(cands),
        "items": [c.to_json() for c in cands],
    }
    out_path.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"Wrote: {out_path} ({len(cands)} items)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())


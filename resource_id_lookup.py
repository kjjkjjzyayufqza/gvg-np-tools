"""
Small helper to map the game's numeric resource IDs to AFS entry names using
previously generated inventory JSON files.

This is useful when IDA shows calls like:
    sub_8886EA4(0x1D6, &dword_8D7AF20, 0)

Then you can quickly resolve which entry is being requested (e.g. Z_DATA.BIN index 470).
"""

from __future__ import annotations

import argparse
import json
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, Iterable, List, Optional, Tuple


@dataclass(frozen=True)
class InventoryHit:
    bin_name: str
    index: int
    name: Optional[str]
    offset: Optional[int]
    size: Optional[int]
    kind: Optional[str]

    def to_json(self) -> Dict[str, Any]:
        return {
            "bin": self.bin_name,
            "index": self.index,
            "name": self.name,
            "offset": self.offset,
            "size": self.size,
            "kind": self.kind,
        }


def parse_int(text: str) -> int:
    s = text.strip().lower()
    base = 16 if s.startswith("0x") else 10
    return int(s, base)


def load_inventory(inv_path: Path) -> Dict[str, Any]:
    with inv_path.open("r", encoding="utf-8") as f:
        return json.load(f)


def iter_inventory_files(inv_dir: Path) -> Iterable[Tuple[str, Path]]:
    for p in sorted(inv_dir.glob("*.inventory.json")):
        # Expect names like: Z_DATA.BIN.inventory.json
        stem = p.name.replace(".inventory.json", "")
        yield stem, p


def lookup_index(inv: Dict[str, Any], idx: int) -> Optional[Dict[str, Any]]:
    entries = inv.get("entries", [])
    if not isinstance(entries, list):
        return None
    if idx < 0 or idx >= len(entries):
        return None
    e = entries[idx]
    if not isinstance(e, dict):
        return None
    return e


def hit_from_entry(bin_name: str, idx: int, e: Optional[Dict[str, Any]]) -> InventoryHit:
    if not e:
        return InventoryHit(bin_name=bin_name, index=idx, name=None, offset=None, size=None, kind=None)
    return InventoryHit(
        bin_name=bin_name,
        index=idx,
        name=e.get("name"),
        offset=e.get("offset"),
        size=e.get("size"),
        kind=e.get("kind"),
    )


def resolve_ids(
    inv_dir: Path, ids: List[int], only_bins: Optional[List[str]] = None
) -> Dict[int, List[InventoryHit]]:
    only = None
    if only_bins:
        only = {b.upper() for b in only_bins}

    results: Dict[int, List[InventoryHit]] = {i: [] for i in ids}
    for bin_name, inv_path in iter_inventory_files(inv_dir):
        if only is not None and bin_name.upper() not in only:
            continue
        inv = load_inventory(inv_path)
        for rid in ids:
            e = lookup_index(inv, rid)
            results[rid].append(hit_from_entry(bin_name, rid, e))
    return results


def search_names(inv_dir: Path, needle: str, only_bins: Optional[List[str]] = None) -> List[InventoryHit]:
    only = None
    if only_bins:
        only = {b.upper() for b in only_bins}
    needle_l = needle.lower()
    hits: List[InventoryHit] = []
    for bin_name, inv_path in iter_inventory_files(inv_dir):
        if only is not None and bin_name.upper() not in only:
            continue
        inv = load_inventory(inv_path)
        entries = inv.get("entries", [])
        if not isinstance(entries, list):
            continue
        for e in entries:
            if not isinstance(e, dict):
                continue
            name = e.get("name")
            if not isinstance(name, str):
                continue
            if needle_l in name.lower():
                idx = int(e.get("index", -1))
                hits.append(hit_from_entry(bin_name, idx, e))
    hits.sort(key=lambda h: (h.bin_name, h.index))
    return hits


def main(argv: List[str]) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument(
        "--inv-dir",
        default=str(Path(__file__).resolve().parent / "data_bin_inventory"),
        help="Directory containing *.inventory.json files.",
    )
    ap.add_argument(
        "--bin",
        action="append",
        dest="bins",
        default=None,
        help="Limit to a specific BIN inventory (repeatable), e.g. --bin Z_DATA.BIN",
    )
    ap.add_argument("--json", action="store_true", help="Print machine-readable JSON.")
    ap.add_argument("--name", default=None, help="Search by substring in entry names instead of ID lookup.")
    ap.add_argument("ids", nargs="*", help="Resource IDs (decimal or 0xHEX).")
    args = ap.parse_args(argv)

    inv_dir = Path(args.inv_dir)
    if not inv_dir.exists():
        print(f"Inventory dir not found: {inv_dir}", file=sys.stderr)
        return 2

    if args.name:
        hits = search_names(inv_dir, args.name, only_bins=args.bins)
        if args.json:
            print(json.dumps([h.to_json() for h in hits], ensure_ascii=False, indent=2))
        else:
            for h in hits:
                print(f"{h.bin_name} idx={h.index} name={h.name} size={h.size} off={h.offset} kind={h.kind}")
        return 0

    if not args.ids:
        ap.error("Provide at least one ID, or use --name.")

    ids = [parse_int(s) for s in args.ids]
    resolved = resolve_ids(inv_dir, ids, only_bins=args.bins)

    if args.json:
        out = {str(k): [h.to_json() for h in v] for k, v in resolved.items()}
        print(json.dumps(out, ensure_ascii=False, indent=2))
        return 0

    for rid in ids:
        print(f"ID {rid} (0x{rid:X}):")
        for h in resolved[rid]:
            if h.name is None:
                print(f"  - {h.bin_name}: <out of range>")
            else:
                print(f\"  - {h.bin_name}: {h.name} (size={h.size}, off={h.offset}, kind={h.kind})\")
    return 0


if __name__ == \"__main__\":
    raise SystemExit(main(sys.argv[1:]))


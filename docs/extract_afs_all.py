import argparse
import re
from pathlib import Path

from afs_inventory import parse_afs_stream


def main() -> int:
    ap = argparse.ArgumentParser(
        description="Extract every AFS entry (e.g. Z_DATA.BIN) to a folder"
    )
    ap.add_argument("afs", type=Path, help="AFS container (e.g. Z_DATA.BIN)")
    ap.add_argument(
        "out",
        type=Path,
        nargs="?",
        default=Path("converted_out"),
        help="Output directory",
    )
    args = ap.parse_args()
    if not args.afs.is_file():
        print("missing", args.afs)
        return 1
    args.out.mkdir(parents=True, exist_ok=True)
    inv = parse_afs_stream(args.afs)
    if "entries" not in inv:
        print(inv)
        return 1

    def safe_name(s: str, idx: int) -> str:
        s = re.sub(r'[<>:"/\\|?*\x00-\x1f]', "_", s)
        s = s.strip() or f"{idx:04d}.bin"
        s = s[:200]
        return f"{idx:04d}_{s}"

    n = 0
    with args.afs.open("rb") as f:
        for e in inv["entries"]:
            idx = e["index"]
            raw = e.get("name", f"{idx:04d}.bin")
            p = args.out / safe_name(str(raw), idx)
            f.seek(e["offset"])
            p.write_bytes(f.read(e["size"]))
            n += 1
    print("entries", n, "->", args.out)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

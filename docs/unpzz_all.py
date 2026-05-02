import argparse
import subprocess
import sys
from pathlib import Path


def find_converter(root: Path) -> Path:
    for sub in ("release", "debug"):
        p = root / "rust_converter" / "target" / sub / "gvg_converter.exe"
        if p.is_file():
            return p
    p = root / "rust_converter" / "target" / "debug" / "gvg_converter"
    if p.is_file():
        return p
    raise FileNotFoundError("build gvg_converter first: cargo build -p gvg_converter")


def main() -> int:
    ap = argparse.ArgumentParser(
        description="For each .pzz, write streams into a same-name folder (e.g. 2500_pbg001.pzz -> 2500_pbg001/)"
    )
    ap.add_argument(
        "dir",
        type=Path,
        nargs="?",
        default=Path("converted_out"),
        help="Directory containing *.pzz",
    )
    ap.add_argument(
        "--strict",
        action="store_true",
        help="Exit 1 if any PZZ failed to extract",
    )
    args = ap.parse_args()
    root = Path(__file__).resolve().parent
    exe = find_converter(root)
    d = args.dir.resolve()
    if not d.is_dir():
        print("not a directory", d, file=sys.stderr)
        return 1
    pzzs = sorted(d.glob("*.pzz"))
    if not pzzs:
        print("no .pzz in", d, file=sys.stderr)
        return 0
    ok = 0
    fail = 0
    for pzz in pzzs:
        out = d / pzz.stem
        r = subprocess.run(
            [str(exe), "extract-streams", str(pzz), "--out", str(out)],
            cwd=str(root),
        )
        if r.returncode == 0:
            ok += 1
        else:
            print("failed", pzz.name, file=sys.stderr)
            fail += 1
    print("done", ok, "ok", fail, "fail", "->", d)
    return 1 if (fail and args.strict) else 0


if __name__ == "__main__":
    raise SystemExit(main())

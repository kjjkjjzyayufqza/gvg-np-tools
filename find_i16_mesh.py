#!/usr/bin/env python3
"""
Scan pzz_harvest_out stream*.bin for int16 xyz vertex-like data and export OBJ point clouds.
Use --debug to also write raw unfiltered OBJ for Blender comparison (if not Gundam-like, analysis is wrong).
"""

from __future__ import annotations

import argparse
import json
import math
import struct
from pathlib import Path
from typing import Any, Dict, List, Literal, Tuple

SCRIPT_DIR = Path(__file__).resolve().parent

Interpret = Literal["i16le", "i16be", "u16le", "u16be"]
AxisOrder = Literal["xyz", "xzy", "yxz", "yzx", "zxy", "zyx"]


def ri16(d: bytes, o: int) -> int:
    return struct.unpack_from("<h", d, o)[0] if o + 2 <= len(d) else 0

def ru32(d: bytes, o: int) -> int:
    return struct.unpack_from("<I", d, o)[0] if o + 4 <= len(d) else 0

def rf32(d: bytes, o: int) -> float:
    return struct.unpack_from("<f", d, o)[0] if o + 4 <= len(d) else 0.0


def _read_i16_triplet(data: bytes, off: int, interpret: Interpret) -> Tuple[int, int, int]:
    if off + 6 > len(data):
        return 0, 0, 0
    if interpret == "i16le":
        return struct.unpack_from("<hhh", data, off)
    if interpret == "i16be":
        return struct.unpack_from(">hhh", data, off)
    if interpret == "u16le":
        a, b, c = struct.unpack_from("<HHH", data, off)
        return int(a), int(b), int(c)
    if interpret == "u16be":
        a, b, c = struct.unpack_from(">HHH", data, off)
        return int(a), int(b), int(c)
    raise ValueError(f"Unknown interpret={interpret!r}")


def _apply_axis(x: float, y: float, z: float, axis: AxisOrder) -> Tuple[float, float, float]:
    if axis == "xyz":
        return x, y, z
    if axis == "xzy":
        return x, z, y
    if axis == "yxz":
        return y, x, z
    if axis == "yzx":
        return y, z, x
    if axis == "zxy":
        return z, x, y
    if axis == "zyx":
        return z, y, x
    raise ValueError(f"Unknown axis={axis!r}")


def scan_i16_vertices(data: bytes, shift: int, stride: int, sample: int = 512) -> dict | None:
    count = (len(data) - shift) // stride
    if count < 64:
        return None
    n = min(count, sample)
    valid = 0
    nonzero = 0
    x_min = x_max = y_min = y_max = z_min = z_max = 0
    first_time = True
    for i in range(n):
        off = shift + i * stride
        x = ri16(data, off)
        y = ri16(data, off + 2)
        z = ri16(data, off + 4)
        if abs(x) < 20000 and abs(y) < 20000 and abs(z) < 20000:
            valid += 1
            if abs(x) + abs(y) + abs(z) > 10:
                nonzero += 1
                if first_time:
                    x_min = x_max = x
                    y_min = y_max = y
                    z_min = z_max = z
                    first_time = False
                else:
                    x_min, x_max = min(x_min, x), max(x_max, x)
                    y_min, y_max = min(y_min, y), max(y_max, y)
                    z_min, z_max = min(z_min, z), max(z_max, z)
    ratio = valid / n
    nz_ratio = nonzero / max(valid, 1)
    if ratio < 0.85 or nz_ratio < 0.5 or nonzero < 32:
        return None
    spread_x = x_max - x_min
    spread_y = y_max - y_min
    spread_z = z_max - z_min
    if spread_x < 100 and spread_y < 100 and spread_z < 100:
        return None
    return {
        "shift": shift,
        "stride": stride,
        "count": count,
        "valid_ratio": round(ratio, 4),
        "nonzero_ratio": round(nz_ratio, 4),
        "x_range": [x_min, x_max],
        "y_range": [y_min, y_max],
        "z_range": [z_min, z_max],
    }


def _obj_header(
    source: str,
    shift: int,
    stride: int,
    count: int,
    filtered: bool,
    interpret: Interpret,
    axis: AxisOrder,
    scale: float,
    u16_center: bool,
) -> str:
    """OBJ header for Blender debugging: source file, stride, shift, vertex count."""
    return (
        f"# source={source} shift={shift} stride={stride} count={count} filtered={filtered}\n"
        f"# interpret={interpret} axis={axis} scale={scale} u16_center={u16_center}\n"
        f"# Import to Blender: if not Gundam-like shape, analysis is wrong.\n"
    )


def write_obj_i16(
    path: Path,
    data: bytes,
    shift: int,
    stride: int,
    count: int,
    scale: float = 1.0 / 256.0,
    interpret: Interpret = "i16le",
    axis: AxisOrder = "xyz",
    u16_center: bool = True,
    debug_raw: bool = False,
    source_name: str = "",
) -> int:
    """
    Export int16 xyz as OBJ point cloud. If debug_raw, write all vertices (no filter) for debugging.
    """
    header = _obj_header(
        source=source_name or path.stem,
        shift=shift,
        stride=stride,
        count=count,
        filtered=not debug_raw,
        interpret=interpret,
        axis=axis,
        scale=scale,
        u16_center=u16_center,
    )
    lines = [header.strip()]
    n = 0
    for i in range(count):
        off = shift + i * stride
        if off + 6 > len(data):
            break
        x_i, y_i, z_i = _read_i16_triplet(data, off, interpret=interpret)

        if interpret in ("u16le", "u16be") and u16_center:
            # Center unsigned values around 0 for easier visual inspection.
            x_i -= 32768
            y_i -= 32768
            z_i -= 32768

        if debug_raw:
            # No filter: export all for Blender debug (user checks if Gundam-like)
            pass
        else:
            if abs(x_i) >= 20000 or abs(y_i) >= 20000 or abs(z_i) >= 20000:
                continue
            if x_i == -32768 or y_i == -32768 or z_i == -32768:
                continue
            if abs(x_i) + abs(y_i) + abs(z_i) <= 10:
                continue

        x = x_i * scale
        y = y_i * scale
        z = z_i * scale
        x, y, z = _apply_axis(x, y, z, axis=axis)
        lines.append(f"v {x:.6f} {y:.6f} {z:.6f}")
        n += 1

    if n >= 32:
        path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return n


def main() -> int:
    ap = argparse.ArgumentParser(description="Scan stream*.bin for int16 xyz, export OBJ point clouds.")
    ap.add_argument(
        "--debug",
        action="store_true",
        help="Also write raw unfiltered .i16mesh.raw.obj for each hit (import to Blender to verify; if not Gundam-like, analysis is wrong).",
    )
    ap.add_argument(
        "--interpret",
        choices=["i16le", "i16be", "u16le", "u16be"],
        default="i16le",
        help="How to interpret the 3x16-bit position fields when exporting OBJ (default: i16le).",
    )
    ap.add_argument(
        "--axis",
        choices=["xyz", "xzy", "yxz", "yzx", "zxy", "zyx"],
        default="xyz",
        help="Axis order to apply when exporting OBJ (default: xyz).",
    )
    ap.add_argument(
        "--scale",
        type=float,
        default=1.0 / 256.0,
        help="Scale factor applied to decoded components (default: 1/256).",
    )
    ap.add_argument(
        "--u16-center",
        action="store_true",
        help="When using u16*, subtract 32768 so positions are centered around 0.",
    )
    ap.add_argument(
        "--no-u16-center",
        action="store_true",
        help="When using u16*, do not subtract 32768.",
    )
    ap.add_argument(
        "--multi",
        action="store_true",
        help="For each hit, also export a few common alternate interpretations (endianness/axis) for quick Blender comparison.",
    )
    args = ap.parse_args()

    harvest_dir = SCRIPT_DIR / "pzz_harvest_out"
    u16_center = bool(args.u16_center) and not bool(args.no_u16_center)
    results: Dict[str, Any] = {
        "scans": [],
        "obj_files": [],
        "debug_raw": args.debug,
        "export": {
            "interpret": args.interpret,
            "axis": args.axis,
            "scale": args.scale,
            "u16_center": u16_center,
            "multi": args.multi,
        },
    }

    for pzz_name in ["pl00", "pl00l", "pl10", "pl41", "dm00", "basic"]:
        pzz_dir = harvest_dir / pzz_name
        if not pzz_dir.exists():
            continue

        for f in sorted(pzz_dir.glob("stream*.bin")):
            data = f.read_bytes()
            if len(data) < 10000:
                continue
            if data[:4] in (b"GIM\x00", b"PMF2") or data[:11] == b"MIG.00.1PSP":
                continue

            best = None
            for stride in [6, 8, 10, 12, 14, 16, 18, 20, 24, 28, 32]:
                for shift in range(0, min(len(data), 2048), 2):
                    r = scan_i16_vertices(data, shift, stride)
                    if r and (best is None or r["count"] * r["nonzero_ratio"] > best["count"] * best["nonzero_ratio"]):
                        best = r

            if best:
                entry = {"pzz": pzz_name, "file": f.name, "size": len(data), **best}
                results["scans"].append(entry)
                source = f"{pzz_name}/{f.name}"
                print(f"{source}: stride={best['stride']} shift={best['shift']} "
                      f"count={best['count']} x=[{best['x_range'][0]},{best['x_range'][1]}] "
                      f"y=[{best['y_range'][0]},{best['y_range'][1]}] "
                      f"z=[{best['z_range'][0]},{best['z_range'][1]}]")

                obj_path = f.with_suffix(".i16mesh.obj")
                n = write_obj_i16(
                    obj_path, data, best["shift"], best["stride"], best["count"],
                    source_name=source,
                    scale=args.scale,
                    interpret=args.interpret,
                    axis=args.axis,
                    u16_center=u16_center,
                )
                if n >= 32:
                    results["obj_files"].append({
                        "pzz": pzz_name,
                        "file": f.name,
                        "obj": str(obj_path),
                        "vertices": n,
                        "stride": best["stride"],
                        "shift": best["shift"],
                        "interpret": args.interpret,
                        "axis": args.axis,
                        "scale": args.scale,
                        "u16_center": u16_center,
                    })
                    print(f"  -> OBJ: {n} vertices (filtered)")

                if args.debug:
                    raw_path = f.with_suffix(".i16mesh.raw.obj")
                    n_raw = write_obj_i16(
                        raw_path, data, best["shift"], best["stride"], best["count"],
                        debug_raw=True,
                        source_name=source,
                        scale=args.scale,
                        interpret=args.interpret,
                        axis=args.axis,
                        u16_center=u16_center,
                    )
                    if n_raw >= 32:
                        print(f"  -> raw OBJ: {n_raw} vertices (unfiltered, for Blender debug)")

                if args.multi:
                    variants = [
                        ("i16le", "xyz"),
                        ("i16le", "xzy"),
                        ("i16be", "xyz"),
                        ("u16le", "xyz"),
                    ]
                    for interp, ax in variants:
                        if interp == args.interpret and ax == args.axis:
                            continue
                        alt_path = f.with_suffix(f".i16mesh.{interp}.{ax}.obj")
                        n_alt = write_obj_i16(
                            alt_path,
                            data,
                            best["shift"],
                            best["stride"],
                            best["count"],
                            debug_raw=False,
                            source_name=source,
                            scale=args.scale,
                            interpret=interp,  # type: ignore[arg-type]
                            axis=ax,  # type: ignore[arg-type]
                            u16_center=u16_center,
                        )
                        if n_alt >= 32:
                            print(f"  -> alt OBJ: {n_alt} vertices (filtered) {interp} {ax}")

    out_path = SCRIPT_DIR / "i16_mesh_report.json"
    out_path.write_text(json.dumps(results, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"\nWrote: {out_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3

from __future__ import annotations

import json
import math
import struct
from pathlib import Path
from typing import Any, Dict, List

SCRIPT_DIR = Path(__file__).resolve().parent


def rf32(d: bytes, o: int) -> float:
    return struct.unpack_from("<f", d, o)[0] if o + 4 <= len(d) else 0.0

def ru32(d: bytes, o: int) -> int:
    return struct.unpack_from("<I", d, o)[0] if o + 4 <= len(d) else 0

def ru16(d: bytes, o: int) -> int:
    return struct.unpack_from("<H", d, o)[0] if o + 2 <= len(d) else 0

def ru8(d: bytes, o: int) -> int:
    return d[o] if o < len(d) else 0


def analyze_block_deep(path: Path) -> Dict:
    data = path.read_bytes()
    sz = len(data)
    result: Dict[str, Any] = {
        "file": path.name,
        "size": sz,
        "magic4_hex": data[:4].hex() if sz >= 4 else "",
        "magic4_ascii": data[:4].decode("ascii", errors="replace") if sz >= 4 else "",
        "head_64_hex": data[:64].hex() if sz >= 64 else data.hex(),
    }

    magic = data[:4] if sz >= 4 else b""

    if magic == b"GIM\x00" or data[:11] == b"MIG.00.1PSP":
        result["format"] = "GIM_texture"
        return result

    if magic == b"PMF2":
        result["format"] = "PMF2"
        if sz >= 0x30:
            result["pmf2_header"] = {
                "u32_04": hex(ru32(data, 4)),
                "u32_08": hex(ru32(data, 8)),
                "u32_0c": hex(ru32(data, 12)),
                "float_10": round(rf32(data, 0x10), 4),
                "float_14": round(rf32(data, 0x14), 4),
                "float_18": round(rf32(data, 0x18), 4),
                "u32_1c": hex(ru32(data, 0x1c)),
            }
            offsets = []
            for i in range(0x20, min(0x200, sz), 4):
                v = ru32(data, i)
                if 0 < v < sz:
                    offsets.append({"table_off": hex(i), "value": hex(v)})
            result["pmf2_offset_table_sample"] = offsets[:30]
        return result

    if sz >= 0x20:
        u32s = [ru32(data, i * 4) for i in range(min(sz // 4, 16))]
        result["header_u32_hex"] = [hex(v) for v in u32s]

        floats = [rf32(data, i * 4) for i in range(min(sz // 4, 16))]
        finite_floats = [f for f in floats if math.isfinite(f) and abs(f) < 10000]
        result["header_as_floats"] = [round(f, 4) for f in floats[:16]]
        result["finite_float_count"] = len(finite_floats)

    for stride in [12, 16, 20, 24, 28, 32, 36, 40, 48]:
        for shift in range(0, min(sz, 128), 4):
            count = (sz - shift) // stride
            if count < 64:
                continue
            n = min(count, 256)
            valid = 0
            nonzero = 0
            for i in range(n):
                off = shift + i * stride
                x = rf32(data, off)
                y = rf32(data, off + 4)
                z = rf32(data, off + 8)
                ok = (math.isfinite(x) and math.isfinite(y) and math.isfinite(z)
                      and abs(x) < 1000 and abs(y) < 1000 and abs(z) < 1000)
                if ok:
                    valid += 1
                    if abs(x) + abs(y) + abs(z) > 0.1:
                        nonzero += 1
            ratio = valid / n
            nz_ratio = nonzero / max(valid, 1)
            if ratio > 0.85 and nz_ratio > 0.5 and valid >= 32:
                preview = []
                for i in range(min(10, count)):
                    off = shift + i * stride
                    preview.append([
                        round(rf32(data, off), 4),
                        round(rf32(data, off + 4), 4),
                        round(rf32(data, off + 8), 4),
                    ])
                result.setdefault("good_vertex_streams", []).append({
                    "shift": shift,
                    "stride": stride,
                    "count": count,
                    "valid_ratio": round(ratio, 4),
                    "nonzero_ratio": round(nz_ratio, 4),
                    "preview": preview,
                })
                break
        if "good_vertex_streams" in result:
            break

    return result


def export_obj_from_best(path: Path, analysis: Dict) -> str:
    streams = analysis.get("good_vertex_streams", [])
    if not streams:
        return ""
    best = streams[0]
    data = path.read_bytes()
    start = best["shift"]
    stride = best["stride"]
    count = best["count"]
    obj_path = path.with_suffix(".mesh.obj")
    lines = []
    n = 0
    pts = []
    for i in range(count):
        off = start + i * stride
        if off + 12 > len(data):
            break
        x = rf32(data, off)
        y = rf32(data, off + 4)
        z = rf32(data, off + 8)
        if (math.isfinite(x) and math.isfinite(y) and math.isfinite(z)
                and abs(x) < 1000 and abs(y) < 1000 and abs(z) < 1000
                and abs(x) + abs(y) + abs(z) > 0.1):
            pts.append((x, y, z))
            n += 1

    if n < 64:
        return ""

    xs = [p[0] for p in pts]
    ys = [p[1] for p in pts]
    zs = [p[2] for p in pts]
    span_x = max(xs) - min(xs)
    span_y = max(ys) - min(ys)
    span_z = max(zs) - min(zs)
    max_span = max(span_x, span_y, span_z, 1e-9)

    # Reject degenerate "streams" that are overwhelmingly axis-aligned or
    # effectively 1D/2D tables (common false positives in blind float probing).
    zero_eps = 1e-6
    zx = sum(1 for v in xs if abs(v) <= zero_eps) / n
    zy = sum(1 for v in ys if abs(v) <= zero_eps) / n
    zz = sum(1 for v in zs if abs(v) <= zero_eps) / n
    thin_axes = sum(1 for s in (span_x, span_y, span_z) if s <= max_span * 0.01)
    mostly_zero_axes = sum(1 for r in (zx, zy, zz) if r >= 0.85)
    if thin_axes >= 2 or mostly_zero_axes >= 2:
        return ""

    for x, y, z in pts:
        lines.append(f"v {x:.6f} {y:.6f} {z:.6f}")

    obj_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return str(obj_path)


def main() -> int:
    harvest_dir = SCRIPT_DIR / "pzz_harvest_out"
    if not harvest_dir.exists():
        print("Run pzz_zlib_harvest.py first")
        return 1

    report: Dict[str, Any] = {"pzz_blocks": {}, "model_candidates": []}

    for pzz_dir in sorted(harvest_dir.iterdir()):
        if not pzz_dir.is_dir():
            continue
        pzz_name = pzz_dir.name
        blocks = []

        for f in sorted(pzz_dir.glob("stream*.bin")):
            analysis = analyze_block_deep(f)
            blocks.append(analysis)

            fmt = analysis.get("format", "")
            if fmt == "PMF2" or analysis.get("good_vertex_streams"):
                obj_path = export_obj_from_best(f, analysis)
                entry = {
                    "pzz": pzz_name,
                    "block": f.name,
                    "format": fmt or "vertex_stream",
                    "size": analysis["size"],
                }
                if obj_path:
                    entry["obj"] = obj_path
                report["model_candidates"].append(entry)

        report["pzz_blocks"][pzz_name] = blocks

    out_path = SCRIPT_DIR / "block_analysis_report.json"
    out_path.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"Wrote: {out_path}")

    print(f"\nModel candidates: {len(report['model_candidates'])}")
    for c in report["model_candidates"]:
        print(f"  {c['pzz']}/{c['block']}: {c['format']} ({c['size']} bytes)")
        if c.get("obj"):
            print(f"    -> {c['obj']}")

    print("\nAll blocks by PZZ:")
    for pzz_name, blocks in report["pzz_blocks"].items():
        non_gim = [b for b in blocks if b.get("format") != "GIM_texture"]
        if non_gim:
            print(f"  {pzz_name}:")
            for b in non_gim:
                fmt = b.get("format", "unknown")
                vs = b.get("good_vertex_streams")
                vs_str = f" vertices={vs[0]['count']}(stride={vs[0]['stride']})" if vs else ""
                print(f"    {b['file']}: {b['size']}B {fmt}{vs_str}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())

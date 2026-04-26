#!/usr/bin/env python3

from __future__ import annotations

import json
import math
import struct
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

SCRIPT_DIR = Path(__file__).resolve().parent


def rf32(d: bytes, o: int) -> float:
    return struct.unpack_from("<f", d, o)[0] if o + 4 <= len(d) else 0.0

def ri32(d: bytes, o: int) -> int:
    return struct.unpack_from("<i", d, o)[0] if o + 4 <= len(d) else 0

def ru32(d: bytes, o: int) -> int:
    return struct.unpack_from("<I", d, o)[0] if o + 4 <= len(d) else 0

def ru16(d: bytes, o: int) -> int:
    return struct.unpack_from("<H", d, o)[0] if o + 2 <= len(d) else 0

def ri16(d: bytes, o: int) -> int:
    return struct.unpack_from("<h", d, o)[0] if o + 2 <= len(d) else 0

def ri8(d: bytes, o: int) -> int:
    return struct.unpack_from("<b", d, o)[0] if o + 1 <= len(d) else 0


def parse_pmf2(data: bytes) -> Dict:
    if len(data) < 0x20 or data[:4] != b"PMF2":
        return {"error": "not_pmf2"}

    num_sections = ru32(data, 4)
    header_size = ru32(data, 8)
    flags = ru32(data, 12)

    result: Dict[str, Any] = {
        "magic": "PMF2",
        "num_sections": num_sections,
        "header_size": header_size,
        "flags": hex(flags),
    }

    if len(data) >= 0x20:
        result["bbox_or_params"] = [round(rf32(data, 0x10 + i * 4), 4) for i in range(4)]

    if header_size > 0x20 and header_size < len(data):
        offset_table = []
        for i in range(0x20, header_size, 4):
            v = ru32(data, i)
            offset_table.append(hex(v))
        result["offset_table"] = offset_table[:60]

    sections = []
    off = header_size
    for i in range(num_sections):
        if off >= len(data):
            break
        sec_size = ru32(data, off) if off + 4 <= len(data) else 0
        sec_head = data[off:off + min(32, len(data) - off)].hex()
        sections.append({
            "index": i,
            "offset": hex(off),
            "first_u32": hex(sec_size),
            "head_hex": sec_head,
        })
        if sec_size > 0 and sec_size < len(data):
            off += sec_size
        else:
            off += 0x10

    result["sections_preview"] = sections[:20]
    return result


def analyze_large_block(path: Path) -> Dict:
    data = path.read_bytes()
    sz = len(data)
    result: Dict[str, Any] = {
        "file": path.name,
        "size": sz,
    }

    result["head_128_hex"] = data[:128].hex()
    result["head_u32"] = [hex(ru32(data, i * 4)) for i in range(min(sz // 4, 32))]

    for stride in [12, 16, 20, 24, 28, 32, 36, 40, 48]:
        for shift in range(0, min(sz, 4096), 4):
            count = (sz - shift) // stride
            if count < 100:
                continue
            n = min(count, 512)
            valid = 0
            nonzero = 0
            x_range = [1e9, -1e9]
            y_range = [1e9, -1e9]
            z_range = [1e9, -1e9]
            for i in range(n):
                off = shift + i * stride
                x = rf32(data, off)
                y = rf32(data, off + 4)
                z = rf32(data, off + 8)
                ok = (math.isfinite(x) and math.isfinite(y) and math.isfinite(z)
                      and abs(x) < 500 and abs(y) < 500 and abs(z) < 500)
                if ok:
                    valid += 1
                    if abs(x) + abs(y) + abs(z) > 0.1:
                        nonzero += 1
                        x_range = [min(x_range[0], x), max(x_range[1], x)]
                        y_range = [min(y_range[0], y), max(y_range[1], y)]
                        z_range = [min(z_range[0], z), max(z_range[1], z)]
            ratio = valid / n
            nz_ratio = nonzero / max(valid, 1)
            if ratio > 0.85 and nz_ratio > 0.6 and valid >= 64:
                preview = []
                for i in range(min(10, count)):
                    off = shift + i * stride
                    preview.append([
                        round(rf32(data, off), 4),
                        round(rf32(data, off + 4), 4),
                        round(rf32(data, off + 8), 4),
                    ])
                result.setdefault("vertex_candidates", []).append({
                    "shift": shift,
                    "stride": stride,
                    "count": count,
                    "valid_ratio": round(ratio, 4),
                    "nonzero_ratio": round(nz_ratio, 4),
                    "x_range": [round(x_range[0], 2), round(x_range[1], 2)] if nonzero else [0, 0],
                    "y_range": [round(y_range[0], 2), round(y_range[1], 2)] if nonzero else [0, 0],
                    "z_range": [round(z_range[0], 2), round(z_range[1], 2)] if nonzero else [0, 0],
                    "preview": preview,
                })
                if len(result.get("vertex_candidates", [])) >= 3:
                    break
        if len(result.get("vertex_candidates", [])) >= 3:
            break

    i16_vals = [ri16(data, i * 2) for i in range(min(sz // 2, 200))]
    i16_range = [min(i16_vals), max(i16_vals)] if i16_vals else [0, 0]
    result["i16_value_range_first400bytes"] = i16_range

    for off_check in range(0, min(sz, 0x1000), 4):
        v = ru32(data, off_check)
        if v > 16 and v < sz and v % 4 == 0:
            target = data[v:v+16]
            if len(target) >= 12:
                fx = rf32(target, 0)
                fy = rf32(target, 4)
                fz = rf32(target, 8)
                if all(math.isfinite(f) and abs(f) < 500 for f in [fx, fy, fz]) and abs(fx) + abs(fy) + abs(fz) > 0.1:
                    result.setdefault("pointer_to_float", []).append({
                        "ptr_off": hex(off_check),
                        "ptr_val": hex(v),
                        "floats": [round(fx, 4), round(fy, 4), round(fz, 4)],
                    })
                    if len(result.get("pointer_to_float", [])) >= 10:
                        break

    return result


def write_point_cloud(path: Path, data: bytes, shift: int, stride: int, count: int) -> int:
    lines = []
    n = 0
    for i in range(count):
        off = shift + i * stride
        if off + 12 > len(data):
            break
        x = rf32(data, off)
        y = rf32(data, off + 4)
        z = rf32(data, off + 8)
        if (math.isfinite(x) and math.isfinite(y) and math.isfinite(z)
                and abs(x) < 500 and abs(y) < 500 and abs(z) < 500
                and abs(x) + abs(y) + abs(z) > 0.1):
            lines.append(f"v {x:.6f} {y:.6f} {z:.6f}")
            n += 1
    if n >= 32:
        path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return n


def main() -> int:
    harvest_dir = SCRIPT_DIR / "pzz_harvest_out"

    report: Dict[str, Any] = {"pmf2_analyses": [], "large_block_analyses": [], "obj_files": []}

    for pzz_name in ["pl00", "pl00l", "pl10", "pl41", "basic", "dm00"]:
        pzz_dir = harvest_dir / pzz_name
        if not pzz_dir.exists():
            continue

        for f in sorted(pzz_dir.glob("stream*.bin")):
            data = f.read_bytes()
            if data[:4] == b"PMF2":
                print(f"PMF2: {pzz_name}/{f.name} ({len(data)} bytes)")
                pmf2 = parse_pmf2(data)
                pmf2["pzz"] = pzz_name
                pmf2["file"] = f.name
                pmf2["size"] = len(data)
                report["pmf2_analyses"].append(pmf2)
            elif len(data) > 10000 and data[:4] != b"GIM\x00" and data[:11] != b"MIG.00.1PSP":
                print(f"Large block: {pzz_name}/{f.name} ({len(data)} bytes)")
                analysis = analyze_large_block(f)
                analysis["pzz"] = pzz_name
                report["large_block_analyses"].append(analysis)

                vcs = analysis.get("vertex_candidates", [])
                if vcs:
                    best = vcs[0]
                    obj_path = f.with_suffix(".point_cloud.obj")
                    n = write_point_cloud(obj_path, data, best["shift"], best["stride"], best["count"])
                    if n >= 32:
                        report["obj_files"].append({
                            "pzz": pzz_name,
                            "block": f.name,
                            "obj": str(obj_path),
                            "vertices": n,
                            "stride": best["stride"],
                            "shift": best["shift"],
                            "xyz_range": {
                                "x": best["x_range"],
                                "y": best["y_range"],
                                "z": best["z_range"],
                            },
                        })
                        print(f"  -> OBJ: {n} vertices, stride={best['stride']}")

    out_path = SCRIPT_DIR / "pmf2_mesh_report.json"
    out_path.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"\nWrote: {out_path}")

    for o in report["obj_files"]:
        print(f"  {o['pzz']}/{o['block']}: {o['vertices']}v stride={o['stride']} {o['xyz_range']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

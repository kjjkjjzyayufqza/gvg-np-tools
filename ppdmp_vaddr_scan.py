#!/usr/bin/env python3
"""
PPSSPP .ppdmp quick scanner.

Goal: avoid guessing "model file formats" by using the frame dump to locate vertex buffer candidates.

Strategy:
- Decompress the zstd payload.
- Collect VADDR values (GE cmd 0x01) and their frequency.
- For the most frequent VADDR offsets, treat payload[vaddr:] as a byte blob and run an int16 xyz
  point-cloud scan (stride/shift) similar to find_i16_mesh.py.
- Export OBJ point clouds for Blender inspection.
"""

from __future__ import annotations

import argparse
import json
import math
import struct
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path
from typing import Dict, List, Optional, Tuple

import zstandard as zstd


ZSTD_MAGIC_LE = b"\x28\xb5\x2f\xfd"


def _decompress_first_frame_and_unused(raw: bytes) -> Tuple[bytes, bytes]:
    start = raw.find(ZSTD_MAGIC_LE)
    if start < 0:
        raise ValueError("zstd magic not found in .ppdmp")
    obj = zstd.ZstdDecompressor().decompressobj()
    out = obj.decompress(raw[start:])
    out += obj.flush()
    return out, obj.unused_data


def decompress_ppdmp_blocks(path: Path) -> Tuple[bytes, bytes]:
    """
    Returns (cmd_stream_blob, mem_dump_blob).

    Observed container in this repository:
    - Header (starts with PPSSPPGE...)
    - zstd frame #1 (GE command stream / state)
    - u32 size + zstd frame #2 (large memory dump, ~24MB)
    """
    raw = path.read_bytes()
    cmd_blob, unused = _decompress_first_frame_and_unused(raw)
    if len(unused) < 8:
        raise ValueError("ppdmp: missing second block")
    comp2_size = struct.unpack_from("<I", unused, 0)[0]
    comp2 = unused[4 : 4 + comp2_size]
    if not comp2.startswith(ZSTD_MAGIC_LE):
        raise ValueError("ppdmp: second block does not look like zstd")
    mem_blob = zstd.ZstdDecompressor().decompress(comp2)
    return cmd_blob, mem_blob


def ri16_le(d: bytes, o: int) -> int:
    return struct.unpack_from("<h", d, o)[0] if o + 2 <= len(d) else 0


@dataclass(frozen=True)
class VaddrHit:
    vaddr: int
    count: int
    sample_vtype: int
    sample_cmd_off: int


def collect_vaddrs(payload: bytes) -> List[VaddrHit]:
    """
    Collect VADDR values that are actually used by PRIM kicks.

    This avoids noise from unrelated state blobs that may contain VADDR-like words.
    """
    last_vtype = 0
    last_vaddr = 0
    freq: Counter[int] = Counter()
    sample: Dict[int, Tuple[int, int]] = {}  # vaddr -> (vtype, cmd_off)

    words = len(payload) // 4
    for i in range(words):
        w = struct.unpack_from("<I", payload, i * 4)[0]
        op = (w >> 24) & 0xFF
        arg = w & 0x00FFFFFF
        if op == 0x12:  # VTYPE
            last_vtype = arg
        elif op == 0x01:  # VADDR
            last_vaddr = arg
        elif op == 0x04:  # PRIM
            cnt = arg & 0xFFFF
            if cnt <= 0:
                continue
            if last_vaddr <= 0:
                continue
            freq[last_vaddr] += 1
            sample.setdefault(last_vaddr, (last_vtype, i * 4))

    hits: List[VaddrHit] = []
    for vaddr, cnt in freq.most_common():
        vt, off = sample.get(vaddr, (0, 0))
        hits.append(VaddrHit(vaddr=vaddr, count=cnt, sample_vtype=vt, sample_cmd_off=off))
    return hits


def map_vaddr_to_mem_offset(vaddr_low24: int, mem_size: int) -> List[Tuple[str, int]]:
    """
    Try a few common PSP RAM base mappings for low24 addresses.
    Returns list of (label, offset) candidates within mem_blob.
    """
    candidates: List[Tuple[str, int]] = []
    for base in (0x08000000, 0x08800000):
        full = base | (vaddr_low24 & 0x00FFFFFF)
        for mem_base in (0x08000000, 0x08800000):
            off = full - mem_base
            if 0 <= off < mem_size:
                candidates.append((f"full={hex(full)} mem_base={hex(mem_base)}", off))
    seen = set()
    out: List[Tuple[str, int]] = []
    for label, off in candidates:
        if off in seen:
            continue
        seen.add(off)
        out.append((label, off))
    return out


def scan_i16_vertices(data: bytes, shift: int, stride: int, pos_off: int, sample: int = 1024) -> Optional[Dict]:
    count = (len(data) - shift) // stride
    if count < 128:
        return None
    n = min(count, sample)
    valid = 0
    nonzero = 0
    x_min = x_max = y_min = y_max = z_min = z_max = 0
    first = True
    for i in range(n):
        off = shift + i * stride + pos_off
        x = ri16_le(data, off + 0)
        y = ri16_le(data, off + 2)
        z = ri16_le(data, off + 4)
        if abs(x) >= 20000 or abs(y) >= 20000 or abs(z) >= 20000:
            continue
        valid += 1
        if abs(x) + abs(y) + abs(z) <= 10:
            continue
        nonzero += 1
        if first:
            x_min = x_max = x
            y_min = y_max = y
            z_min = z_max = z
            first = False
        else:
            x_min, x_max = min(x_min, x), max(x_max, x)
            y_min, y_max = min(y_min, y), max(y_max, y)
            z_min, z_max = min(z_min, z), max(z_max, z)

    if valid < 256 or nonzero < 128:
        return None
    spread_x = x_max - x_min
    spread_y = y_max - y_min
    spread_z = z_max - z_min
    if max(spread_x, spread_y, spread_z) < 200:
        return None

    # Reject near-1D candidates early (common: reading UV/weights/index tables).
    spreads = sorted([spread_x, spread_y, spread_z])
    if spreads[1] < 50:
        return None

    return {
        "shift": shift,
        "stride": stride,
        "pos_off": pos_off,
        "count": count,
        "valid": valid,
        "nonzero": nonzero,
        "x_range": [x_min, x_max],
        "y_range": [y_min, y_max],
        "z_range": [z_min, z_max],
        "spread": [spread_x, spread_y, spread_z],
    }


def write_obj_point_cloud(
    path: Path,
    data: bytes,
    shift: int,
    stride: int,
    pos_off: int,
    max_vertices: int,
    scale: float,
    header: str,
    raw: bool,
) -> int:
    lines = [header.rstrip()]
    written = 0
    count = min((len(data) - shift) // stride, max_vertices)
    for i in range(count):
        off = shift + i * stride + pos_off
        if off + 6 > len(data):
            break
        x = ri16_le(data, off + 0)
        y = ri16_le(data, off + 2)
        z = ri16_le(data, off + 4)
        if not raw:
            if abs(x) >= 20000 or abs(y) >= 20000 or abs(z) >= 20000:
                continue
            if abs(x) + abs(y) + abs(z) <= 10:
                continue
        lines.append(f"v {x * scale:.6f} {y * scale:.6f} {z * scale:.6f}")
        written += 1
    if written >= 64:
        path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return written


def main() -> int:
    ap = argparse.ArgumentParser(description="Scan PPSSPP .ppdmp VADDRs for int16 xyz point clouds.")
    ap.add_argument("--ppdmp", default="test/ppsspp_dump/NPJH50107_0001.ppdmp", help="Path to .ppdmp")
    ap.add_argument("--out-dir", default="test/ppsspp_dump/vaddr_scan_out", help="Output directory")
    ap.add_argument("--top", type=int, default=50, help="How many top VADDRs to try")
    ap.add_argument("--blob-max", type=int, default=2_000_000, help="Max bytes from VADDR to scan")
    ap.add_argument("--max-vertices", type=int, default=200_000, help="Max vertices written per OBJ")
    ap.add_argument("--scale", type=float, default=1.0 / 256.0, help="Scale for int16 positions")
    ap.add_argument("--posoff-max", type=int, default=64, help="Max pos_off to try inside each stride (bytes)")
    ap.add_argument(
        "--strides",
        default="18,16,20,24,12",
        help="Comma-separated stride candidates to try (default: 18,16,20,24,12).",
    )
    ap.add_argument(
        "--shift-step",
        type=int,
        default=8,
        help="Shift step in bytes when scanning (default: 8).",
    )
    args = ap.parse_args()

    ppdmp_path = Path(args.ppdmp)
    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)

    cmd_blob, mem_blob = decompress_ppdmp_blocks(ppdmp_path)
    vaddrs = collect_vaddrs(cmd_blob)[: args.top]

    report: Dict[str, object] = {
        "ppdmp": str(ppdmp_path),
        "cmd_blob_size": len(cmd_blob),
        "mem_blob_size": len(mem_blob),
        "top": args.top,
        "results": [],
    }

    for hit in vaddrs:
        maps = map_vaddr_to_mem_offset(hit.vaddr, mem_size=len(mem_blob))
        if not maps:
            continue

        best = None
        best_meta: Optional[Dict[str, object]] = None
        stride_list = []
        for tok in args.strides.split(","):
            tok = tok.strip()
            if not tok:
                continue
            try:
                stride_list.append(int(tok))
            except ValueError:
                pass
        if not stride_list:
            stride_list = [18, 16, 20, 24, 12]

        for map_label, mem_off in maps:
            blob = mem_blob[mem_off : min(len(mem_blob), mem_off + args.blob_max)]
            print(f"Scanning vaddr=0x{hit.vaddr:06X} freq={hit.count} map=({map_label}) blob={len(blob)} bytes ...")

            for stride in stride_list:
                for shift in range(0, min(2048, max(0, len(blob) - 6)), max(2, args.shift_step)):
                    # Search for position field offset inside the record.
                    for pos_off in range(0, min(args.posoff_max, stride - 6) + 1, 2):
                        r = scan_i16_vertices(blob, shift, stride, pos_off=pos_off)
                        if not r:
                            continue
                        spreads = r["spread"]
                        score = spreads[0] * spreads[1] * spreads[2]
                        if best is None or score > best.get("score", 0):
                            best = {**r, "score": score}
                            best_meta = {
                                "map": map_label,
                                "mem_off": int(mem_off),
                                "blob_len": int(len(blob)),
                            }
        if not best:
            continue
        assert best_meta is not None

        mem_off = int(best_meta["mem_off"])
        blob = mem_blob[mem_off : min(len(mem_blob), mem_off + args.blob_max)]

        header = (
            f"# ppdmp={ppdmp_path.name} vaddr=0x{hit.vaddr:06X} freq={hit.count} cmd_off=0x{hit.sample_cmd_off:X} vtype=0x{hit.sample_vtype:06X}\n"
            f"# map={best_meta['map']} mem_off=0x{mem_off:X} blob_len={best_meta['blob_len']}\n"
            f"# best_shift={best['shift']} best_stride={best['stride']} best_pos_off={best['pos_off']} count={best['count']} scale={args.scale}\n"
            f"# ranges x={best['x_range']} y={best['y_range']} z={best['z_range']} spread={best['spread']}\n"
            f"# If not Gundam-like in Blender, parsing is wrong.\n"
        )

        stem = (
            f"vaddr{hit.vaddr:06x}_freq{hit.count:04d}"
            f"_stride{best['stride']}_shift{best['shift']}_pos{best['pos_off']}"
        )
        obj_filtered = out_dir / f"{stem}.obj"
        obj_raw = out_dir / f"{stem}.raw.obj"

        n_f = write_obj_point_cloud(
            obj_filtered,
            blob,
            shift=best["shift"],
            stride=best["stride"],
            pos_off=best["pos_off"],
            max_vertices=args.max_vertices,
            scale=args.scale,
            header=header + "# filtered=True\n",
            raw=False,
        )
        n_r = write_obj_point_cloud(
            obj_raw,
            blob,
            shift=best["shift"],
            stride=best["stride"],
            pos_off=best["pos_off"],
            max_vertices=args.max_vertices,
            scale=args.scale,
            header=header + "# filtered=False\n",
            raw=True,
        )

        report["results"].append(
            {
                "vaddr": hit.vaddr,
                "freq": hit.count,
                "sample_vtype": hit.sample_vtype,
                "sample_cmd_off": hit.sample_cmd_off,
                "map": best_meta,
                "best": best,
                "written_filtered": n_f,
                "written_raw": n_r,
                "obj_filtered": str(obj_filtered) if n_f >= 64 else None,
                "obj_raw": str(obj_raw) if n_r >= 64 else None,
            }
        )
        print(f"vaddr=0x{hit.vaddr:06X} freq={hit.count} -> {n_f} filtered, {n_r} raw ({best_meta['map']})")

    (out_dir / "vaddr_scan_report.json").write_text(
        json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    print(f"Wrote report: {out_dir / 'vaddr_scan_report.json'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())


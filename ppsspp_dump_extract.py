#!/usr/bin/env python3
"""
Extract vertex point clouds from a PPSSPP frame dump (.ppdmp).

This is intended for *debugging correctness* of model analysis:
- If extracted point clouds do not look Gundam-like in Blender, our parsing assumptions are wrong.

The PPSSPP dump observed in this repo contains a zstd-compressed payload and embeds a GE-like command
stream (VTYPE/VADDR/IADDR/PRIM). We parse the command stream to locate draw calls, then decode
vertex records according to the PSP GE VTYPE bitfield (YAPSPD documentation).
"""

from __future__ import annotations

import argparse
import json
import math
import struct
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Dict, Iterable, List, Optional, Tuple

import zstandard as zstd


ZSTD_MAGIC_LE = b"\x28\xb5\x2f\xfd"


def read_u32_le(data: bytes, off: int) -> int:
    if off + 4 > len(data):
        return 0
    return struct.unpack_from("<I", data, off)[0]


def find_zstd_offset(ppdmp: bytes) -> int:
    idx = ppdmp.find(ZSTD_MAGIC_LE)
    if idx < 0:
        raise ValueError("zstd magic not found in .ppdmp")
    return idx


def decompress_payload(ppdmp: bytes) -> bytes:
    off = find_zstd_offset(ppdmp)
    return zstd.ZstdDecompressor().decompress(ppdmp[off:])


def iter_u32_words(data: bytes) -> Iterable[Tuple[int, int]]:
    """Yield (byte_offset, u32_word)."""
    n = len(data) & ~3
    for off in range(0, n, 4):
        yield off, struct.unpack_from("<I", data, off)[0]


@dataclass(frozen=True)
class VTypeInfo:
    raw: int
    through: bool
    morph_count: int
    weight_count: int
    index_fmt: str
    weight_fmt: str
    tex_fmt: str
    color_fmt: str
    normal_fmt: str
    pos_fmt: str


def _fmt_size(fmt: str) -> int:
    if fmt == "none":
        return 0
    if fmt == "8":
        return 1
    if fmt == "16":
        return 2
    if fmt == "32f":
        return 4
    raise ValueError(f"Unknown fmt={fmt!r}")


def decode_vtype(arg24: int) -> VTypeInfo:
    # YAPSPD chap11: VTYPE bitfield (lower 24 bits used).
    through = bool((arg24 >> 23) & 1)

    morph_count = ((arg24 >> 18) & 0x7) + 1
    weight_count = ((arg24 >> 14) & 0x7) + 1

    index_fmt_bits = (arg24 >> 11) & 0x3
    index_fmt = {0: "none", 1: "u8", 2: "u16", 3: "reserved"}.get(index_fmt_bits, "reserved")

    w_bits = (arg24 >> 9) & 0x3
    weight_fmt = {0: "none", 1: "8", 2: "16", 3: "32f"}.get(w_bits, "none")

    p_bits = (arg24 >> 7) & 0x3
    pos_fmt = {0: "none", 1: "8", 2: "16", 3: "32f"}.get(p_bits, "none")

    n_bits = (arg24 >> 5) & 0x3
    normal_fmt = {0: "none", 1: "8", 2: "16", 3: "32f"}.get(n_bits, "none")

    c_bits = (arg24 >> 2) & 0x7
    # Color uses a different encoding.
    color_fmt = {
        0: "none",
        4: "bgr5650",
        5: "abgr5551",
        6: "abgr4444",
        7: "abgr8888",
    }.get(c_bits, "other")

    t_bits = (arg24 >> 0) & 0x3
    tex_fmt = {0: "none", 1: "8", 2: "16", 3: "32f"}.get(t_bits, "none")

    return VTypeInfo(
        raw=arg24,
        through=through,
        morph_count=morph_count,
        weight_count=weight_count,
        index_fmt=index_fmt,
        weight_fmt=weight_fmt,
        tex_fmt=tex_fmt,
        color_fmt=color_fmt,
        normal_fmt=normal_fmt,
        pos_fmt=pos_fmt,
    )


def vertex_stride_bytes(vt: VTypeInfo) -> int:
    # This is a practical approximation for debugging. It ignores some corner cases
    # (e.g., alignments, some color formats not in common set).
    stride = 0
    stride += 2 * _fmt_size(vt.tex_fmt)
    if vt.color_fmt == "none":
        stride += 0
    elif vt.color_fmt in ("bgr5650", "abgr5551", "abgr4444"):
        stride += 2
    elif vt.color_fmt == "abgr8888":
        stride += 4
    else:
        # Unknown color format: do not guess.
        return 0

    stride += 3 * _fmt_size(vt.normal_fmt)
    stride += vt.weight_count * _fmt_size(vt.weight_fmt)
    stride += 3 * _fmt_size(vt.pos_fmt)

    # Morphing duplicates position/normal/tex/weights per morph target.
    if vt.morph_count > 1:
        stride *= vt.morph_count
    return stride


def _read_fixed(data: bytes, off: int, size: int, signed: bool) -> float:
    if size == 1:
        v = data[off]
        if signed and v >= 128:
            v -= 256
        return float(v)
    if size == 2:
        v = struct.unpack_from("<H", data, off)[0]
        if signed and v >= 0x8000:
            v -= 0x10000
        return float(v)
    raise ValueError("fixed size must be 1 or 2")


def read_position_xyz(data: bytes, off: int, vt: VTypeInfo, scale: float) -> Optional[Tuple[float, float, float]]:
    if vt.pos_fmt == "none":
        return None
    if vt.pos_fmt == "32f":
        if off + 12 > len(data):
            return None
        x, y, z = struct.unpack_from("<fff", data, off)
        if not (math.isfinite(x) and math.isfinite(y) and math.isfinite(z)):
            return None
        return x, y, z
    if vt.pos_fmt in ("8", "16"):
        sz = _fmt_size(vt.pos_fmt)
        if off + 3 * sz > len(data):
            return None
        x = _read_fixed(data, off + 0 * sz, sz, signed=True) * scale
        y = _read_fixed(data, off + 1 * sz, sz, signed=True) * scale
        z = _read_fixed(data, off + 2 * sz, sz, signed=True) * scale
        return x, y, z
    return None


@dataclass
class DrawCall:
    cmd_off: int
    vtype: int
    vaddr: int
    iaddr: int
    prim_type: int
    count: int


def parse_drawcalls(payload: bytes, max_words: int = 500000) -> List[DrawCall]:
    vtype = 0
    vaddr = 0
    iaddr = 0
    calls: List[DrawCall] = []

    words = min(len(payload) // 4, max_words)
    for i in range(words):
        w = struct.unpack_from("<I", payload, i * 4)[0]
        op = (w >> 24) & 0xFF
        arg = w & 0x00FFFFFF

        if op == 0x12:  # VTYPE
            vtype = arg
        elif op == 0x01:  # VADDR
            vaddr = arg
        elif op == 0x02:  # IADDR
            iaddr = arg
        elif op == 0x04:  # PRIM
            prim_type = (arg >> 16) & 0x7
            count = arg & 0xFFFF
            calls.append(
                DrawCall(
                    cmd_off=i * 4,
                    vtype=vtype,
                    vaddr=vaddr,
                    iaddr=iaddr,
                    prim_type=prim_type,
                    count=count,
                )
            )
        elif op == 0x0C:  # END
            # Frame dumps can contain multiple command lists or sentinel ENDs.
            # Reset state but keep scanning.
            vtype = 0
            vaddr = 0
            iaddr = 0
    return calls


def extract_points_for_call(payload: bytes, call: DrawCall, scale16: float) -> List[Tuple[float, float, float]]:
    vt = decode_vtype(call.vtype)
    stride = vertex_stride_bytes(vt)
    if stride <= 0:
        return []

    # Layout: tex, color, normal, weights, position. We only need position offset.
    pos_off = 0
    pos_off += 2 * _fmt_size(vt.tex_fmt)
    if vt.color_fmt == "none":
        pos_off += 0
    elif vt.color_fmt in ("bgr5650", "abgr5551", "abgr4444"):
        pos_off += 2
    elif vt.color_fmt == "abgr8888":
        pos_off += 4
    else:
        return []
    pos_off += 3 * _fmt_size(vt.normal_fmt)
    pos_off += vt.weight_count * _fmt_size(vt.weight_fmt)

    start = call.vaddr
    pts: List[Tuple[float, float, float]] = []
    for i in range(call.count):
        voff = start + i * stride + pos_off
        pos = read_position_xyz(payload, voff, vt, scale=scale16)
        if not pos:
            continue
        x, y, z = pos
        if not (math.isfinite(x) and math.isfinite(y) and math.isfinite(z)):
            continue
        pts.append((x, y, z))
    return pts


def write_obj(path: Path, points: List[Tuple[float, float, float]], header: str) -> None:
    lines = [header.rstrip()]
    for x, y, z in points:
        lines.append(f"v {x:.6f} {y:.6f} {z:.6f}")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> int:
    ap = argparse.ArgumentParser(description="Extract vertex point clouds from a PPSSPP .ppdmp dump.")
    ap.add_argument("--ppdmp", default="test/ppsspp_dump/NPJH50107_0001.ppdmp", help="Path to .ppdmp file")
    ap.add_argument("--out-dir", default="test/ppsspp_dump/out", help="Output directory")
    ap.add_argument("--min-count", type=int, default=256, help="Minimum PRIM vertex count to export")
    ap.add_argument("--max-calls", type=int, default=30, help="Maximum draw calls to export")
    ap.add_argument("--scale16", type=float, default=1.0 / 256.0, help="Scale for fixed-point 8/16-bit positions")
    args = ap.parse_args()

    ppdmp_path = Path(args.ppdmp)
    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)

    raw = ppdmp_path.read_bytes()
    payload = decompress_payload(raw)

    calls = parse_drawcalls(payload)
    report: Dict[str, object] = {
        "ppdmp": str(ppdmp_path),
        "payload_size": len(payload),
        "drawcall_count": len(calls),
        "exported": [],
    }

    exported = 0
    for idx, call in enumerate(calls):
        if call.count < args.min_count:
            continue
        vt = decode_vtype(call.vtype)
        if vt.pos_fmt == "none":
            continue
        pts = extract_points_for_call(payload, call, scale16=args.scale16)
        if len(pts) < args.min_count:
            continue

        header = (
            f"# ppdmp={ppdmp_path.name} cmd_off=0x{call.cmd_off:X} idx={idx}\n"
            f"# vtype=0x{call.vtype:06X} pos_fmt={vt.pos_fmt} normal_fmt={vt.normal_fmt} tex_fmt={vt.tex_fmt} "
            f"color_fmt={vt.color_fmt} weight_fmt={vt.weight_fmt} index_fmt={vt.index_fmt}\n"
            f"# vaddr=0x{call.vaddr:06X} iaddr=0x{call.iaddr:06X} prim_type={call.prim_type} count={call.count}\n"
            f"# If not Gundam-like in Blender, parsing is wrong.\n"
        )
        obj_path = out_dir / f"call{idx:04d}_off{call.cmd_off:08x}_vaddr{call.vaddr:06x}_vtype{call.vtype:06x}.obj"
        write_obj(obj_path, pts, header=header)

        report["exported"].append(
            {
                "index": idx,
                "cmd_off": call.cmd_off,
                "vtype": call.vtype,
                "vtype_decoded": asdict(vt),
                "stride": vertex_stride_bytes(vt),
                "vaddr": call.vaddr,
                "iaddr": call.iaddr,
                "prim_type": call.prim_type,
                "count": call.count,
                "points_written": len(pts),
                "obj": str(obj_path),
            }
        )
        exported += 1
        if exported >= args.max_calls:
            break

    (out_dir / "ppdmp_extract_report.json").write_text(
        json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    print(f"Wrote {exported} OBJ files to: {out_dir}")
    print(f"Wrote report: {out_dir / 'ppdmp_extract_report.json'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())


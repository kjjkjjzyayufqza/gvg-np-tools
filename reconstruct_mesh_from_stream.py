#!/usr/bin/env python3
"""
Heuristic reconstruction of a mesh (vertices + indices) from a single decompressed stream block.

Why:
  - In this game, PZZ zlib streams often contain PMF2 metadata + one large "mesh blob".
  - We may not yet understand PMF2 enough to directly locate vertex/index buffers.
  - This script tries to find an index buffer and a matching vertex buffer by scanning patterns.

Inputs:
  - A binary blob (typically something like stream006_offXXXXXX.bin).

Outputs:
  - One or more OBJ files for debugging (triangles and/or triangle strip interpretation).
  - A JSON report with candidate offsets and scores.

No TODOs in code by design.
"""

from __future__ import annotations

import argparse
import json
import math
import struct
from dataclasses import dataclass
from pathlib import Path
from typing import Dict, List, Optional, Tuple


def u16_at(data: bytes, off: int) -> int:
    if off + 2 > len(data):
        return 0
    return struct.unpack_from("<H", data, off)[0]


def i16_triplet_at(data: bytes, off: int) -> Optional[Tuple[int, int, int]]:
    if off + 6 > len(data):
        return None
    return struct.unpack_from("<hhh", data, off)


@dataclass(frozen=True)
class IndexCandidate:
    off: int
    sample_triplets: int
    hit_triplets: int
    max_index: int
    est_index_count: int
    score: float


def score_u16_triangle_triplets(vals: List[int], max_allowed: int) -> Tuple[int, int, int]:
    triplets = len(vals) // 3
    hit = 0
    mx = 0
    for i in range(triplets):
        a, b, c = vals[i * 3], vals[i * 3 + 1], vals[i * 3 + 2]
        mx = max(mx, a, b, c)
        if a == b or b == c or a == c:
            continue
        if a > max_allowed or b > max_allowed or c > max_allowed:
            continue
        hit += 1
    return triplets, hit, mx


def score_u16_loose(vals: List[int], *, small_lt: int, max_allowed: int) -> Tuple[float, int]:
    if not vals:
        return 0.0, 0
    small = 0
    mx = 0
    for v in vals:
        mx = max(mx, v)
        if v < small_lt:
            small += 1
    ratio = small / len(vals)
    if mx > max_allowed:
        ratio *= 0.2
    return ratio, mx


def find_index_candidates_u16(
    data: bytes,
    *,
    step: int = 2,
    window_bytes: int = 0x6000,
    sample_bytes: int = 4096,
    max_allowed: int = 60000,
    small_lt: int = 8192,
    loose: bool = False,
    top_k: int = 20,
) -> List[IndexCandidate]:
    out: List[IndexCandidate] = []
    n = len(data)
    if n < 64:
        return out
    window_bytes = max(0x600, min(window_bytes, n))
    sample_bytes = max(0x600, min(sample_bytes, window_bytes, n))

    for off in range(0, n - sample_bytes, step):
        # Progress hint for long scans (prints are cheap, and help avoid the impression of hanging.)
        if off != 0 and (off // step) % 5000 == 0:
            print(f"Index scan progress: off=0x{off:X}/{n:X} candidates={len(out)}")
        # Fast reject: index buffers usually have lots of small values, so u16 < 4096 ratio is high.
        small = 0
        total = 0
        for j in range(0, min(sample_bytes, 1024), 2):
            v = u16_at(data, off + j)
            total += 1
            if v < 4096:
                small += 1
        if total and (small / total) < 0.70:
            continue

        if loose:
            small = 0
            mx = 0
            total = 0
            for j in range(0, sample_bytes, 2):
                v = u16_at(data, off + j)
                total += 1
                mx = max(mx, v)
                if v < small_lt:
                    small += 1
            ratio = (small / total) if total else 0.0
            hit = int(ratio * 1000000)
            trip = max(1, total // 3)
            if ratio < 0.90:
                continue
        else:
            vals = list(struct.unpack_from("<" + "H" * (sample_bytes // 2), data, off))
            trip, hit, mx = score_u16_triangle_triplets(vals, max_allowed=max_allowed)
            if trip < 256:
                continue
            ratio = hit / trip
            if ratio < 0.60:
                continue

        # Estimate index count by scanning forward while values stay "reasonable".
        est_count = 0
        mx2 = 0
        limit_bytes = min(window_bytes, n - off)
        for j in range(0, limit_bytes, 2):
            v = u16_at(data, off + j)
            mx2 = max(mx2, v)
            if v > max_allowed:
                break
            est_count += 1
        score = ratio * (1.0 + min(1.0, est_count / 20000.0))
        out.append(
            IndexCandidate(
                off=off,
                sample_triplets=trip,
                hit_triplets=hit,
                max_index=max(mx, mx2),
                est_index_count=est_count,
                score=score,
            )
        )

    out.sort(key=lambda c: (c.score, c.hit_triplets, c.est_index_count), reverse=True)
    # Deduplicate by near offsets.
    dedup: List[IndexCandidate] = []
    for c in out:
        if any(abs(c.off - d.off) < 0x200 for d in dedup):
            continue
        dedup.append(c)
        if len(dedup) >= top_k:
            break
    return dedup


@dataclass(frozen=True)
class VertexCandidate:
    shift: int
    stride: int
    pos_off: int
    count: int
    spread: Tuple[int, int, int]
    score: float


def scan_i16_vertices(
    data: bytes,
    *,
    shift: int,
    stride: int,
    pos_off: int,
    sample_n: int = 4096,
    reject_abs_ge: int = 32000,
) -> Optional[VertexCandidate]:
    if stride < 6:
        return None
    n = (len(data) - shift - pos_off) // stride
    if n <= 0:
        return None
    n_s = min(n, sample_n)

    xs: List[int] = []
    ys: List[int] = []
    zs: List[int] = []
    valid = 0
    for i in range(n_s):
        off = shift + i * stride + pos_off
        t = i16_triplet_at(data, off)
        if t is None:
            break
        x, y, z = t
        if abs(x) >= reject_abs_ge or abs(y) >= reject_abs_ge or abs(z) >= reject_abs_ge:
            continue
        if x == -32768 or y == -32768 or z == -32768:
            continue
        if abs(x) + abs(y) + abs(z) <= 6:
            continue
        xs.append(x)
        ys.append(y)
        zs.append(z)
        valid += 1

    if valid < max(128, n_s // 20):
        return None

    sx = max(xs) - min(xs)
    sy = max(ys) - min(ys)
    sz = max(zs) - min(zs)
    spread = (sx, sy, sz)

    spreads_sorted = sorted(spread)
    if spreads_sorted[1] < 20:
        return None

    score = float(sx) * float(sy) * float(sz)
    return VertexCandidate(
        shift=shift,
        stride=stride,
        pos_off=pos_off,
        count=n,
        spread=spread,
        score=score,
    )


def find_vertex_candidates_i16(
    data: bytes,
    *,
    min_count: int,
    strides: List[int],
    shift_max: int = 4096,
    posoff_max: int = 64,
    shift_step: int = 4,
    posoff_step: int = 2,
    top_k: int = 10,
) -> List[VertexCandidate]:
    best: List[VertexCandidate] = []
    for stride in strides:
        for shift in range(0, min(shift_max, max(0, len(data) - 6)), shift_step):
            for pos_off in range(0, min(posoff_max, max(0, stride - 6)) + 1, posoff_step):
                cand = scan_i16_vertices(data, shift=shift, stride=stride, pos_off=pos_off)
                if not cand:
                    continue
                if cand.count < min_count:
                    continue
                best.append(cand)
    best.sort(key=lambda c: c.score, reverse=True)
    dedup: List[VertexCandidate] = []
    for c in best:
        if any((c.stride == d.stride and abs(c.shift - d.shift) < 8 and abs(c.pos_off - d.pos_off) < 4) for d in dedup):
            continue
        dedup.append(c)
        if len(dedup) >= top_k:
            break
    return dedup


def indices_to_faces_triangles(indices: List[int], max_faces: int) -> List[Tuple[int, int, int]]:
    faces: List[Tuple[int, int, int]] = []
    for i in range(0, len(indices) - 2, 3):
        if len(faces) >= max_faces:
            break
        a, b, c = indices[i], indices[i + 1], indices[i + 2]
        if a == b or b == c or a == c:
            continue
        faces.append((a, b, c))
    return faces


def indices_to_faces_strip(indices: List[int], max_faces: int) -> List[Tuple[int, int, int]]:
    faces: List[Tuple[int, int, int]] = []
    flip = False
    for i in range(len(indices) - 2):
        if len(faces) >= max_faces:
            break
        a, b, c = indices[i], indices[i + 1], indices[i + 2]
        if a == b or b == c or a == c:
            flip = False
            continue
        faces.append((b, a, c) if flip else (a, b, c))
        flip = not flip
    return faces


def write_obj(path: Path, verts: List[Tuple[float, float, float]], faces: List[Tuple[int, int, int]], header: List[str]) -> None:
    lines: List[str] = []
    for h in header:
        lines.append(f"# {h}")
    for x, y, z in verts:
        lines.append(f"v {x:.6f} {y:.6f} {z:.6f}")
    for a, b, c in faces:
        # OBJ is 1-based.
        lines.append(f"f {a + 1} {b + 1} {c + 1}")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def is_valid_i16_triplet(t: Tuple[int, int, int]) -> bool:
    x, y, z = t
    if x == -32768 or y == -32768 or z == -32768:
        return False
    if abs(x) >= 32000 or abs(y) >= 32000 or abs(z) >= 32000:
        return False
    return True


def filter_and_compact(
    verts_raw: List[Tuple[float, float, float]],
    valid: List[bool],
    faces: List[Tuple[int, int, int]],
) -> Tuple[List[Tuple[float, float, float]], List[Tuple[int, int, int]]]:
    kept_faces: List[Tuple[int, int, int]] = []
    for a, b, c in faces:
        if a < 0 or b < 0 or c < 0:
            continue
        if a >= len(valid) or b >= len(valid) or c >= len(valid):
            continue
        if not (valid[a] and valid[b] and valid[c]):
            continue
        kept_faces.append((a, b, c))

    if not kept_faces:
        return [], []

    used_set = set()
    for a, b, c in kept_faces:
        used_set.add(a)
        used_set.add(b)
        used_set.add(c)
    used = sorted(i for i in used_set if 0 <= i < len(verts_raw))
    remap = {old: new for new, old in enumerate(used)}

    verts = [verts_raw[i] for i in used]
    out_faces: List[Tuple[int, int, int]] = []
    for a, b, c in kept_faces:
        out_faces.append((remap[a], remap[b], remap[c]))
    return verts, out_faces


def main(argv: Optional[List[str]] = None) -> int:
    ap = argparse.ArgumentParser(description="Heuristically reconstruct a mesh from a stream blob.")
    ap.add_argument("--in", dest="in_path", required=True, help="Input stream blob path (e.g. stream006_off*.bin)")
    ap.add_argument(
        "--verts-from",
        dest="verts_from",
        default=None,
        help="Optional separate blob to search vertices in (useful when indices and vertices are in different streams).",
    )
    ap.add_argument("--out-dir", default=None, help="Output directory (default: alongside input)")
    ap.add_argument("--max-faces", type=int, default=200000, help="Maximum faces to write")
    ap.add_argument("--index-window", type=lambda s: int(s, 0), default=0x6000, help="Index scan window bytes")
    ap.add_argument("--index-sample", type=lambda s: int(s, 0), default=0x2000, help="Index scan sample bytes (per offset)")
    ap.add_argument("--index-step", type=int, default=32, help="Index scan step bytes (higher is faster)")
    ap.add_argument("--index-top", type=int, default=10, help="Top index candidates to try")
    ap.add_argument("--strict-index", action="store_true", help="Also run strict triangle-like index scan (slower)")
    ap.add_argument("--strides", default="12,16,18,20,24,28,32", help="Candidate vertex strides for i16 scanning")
    ap.add_argument("--shift-max", type=int, default=4096, help="Max shift to try for vertex scanning")
    ap.add_argument("--posoff-max", type=int, default=64, help="Max pos_off to try within stride")
    ap.add_argument("--scale", type=float, default=1.0 / 256.0, help="Scale applied to i16 positions")
    ap.add_argument("--v-shift", type=int, default=-1, help="Force vertex shift (skip vertex search if set >= 0)")
    ap.add_argument("--v-stride", type=int, default=-1, help="Force vertex stride (skip vertex search if set > 0)")
    ap.add_argument("--v-posoff", type=int, default=-1, help="Force vertex pos_off (skip vertex search if set >= 0)")
    ap.add_argument("--export-all", action="store_true", help="Export multiple index candidates (up to --index-top)")
    args = ap.parse_args(argv)

    in_path = Path(args.in_path)
    idx_data = in_path.read_bytes()
    vtx_path = Path(args.verts_from) if args.verts_from else in_path
    vtx_data = vtx_path.read_bytes() if vtx_path != in_path else idx_data
    out_dir = Path(args.out_dir) if args.out_dir else in_path.parent / (in_path.stem + "_recon")
    out_dir.mkdir(parents=True, exist_ok=True)

    strides = [int(x.strip()) for x in args.strides.split(",") if x.strip()]

    # Loose scan first (fast): indices often look like strips, with degenerates.
    idx_cands = find_index_candidates_u16(
        idx_data,
        step=max(2, args.index_step),
        window_bytes=min(args.index_window, len(idx_data)),
        sample_bytes=min(args.index_sample, len(idx_data)),
        top_k=args.index_top,
        loose=True,
    )
    if args.strict_index:
        idx_cands = find_index_candidates_u16(
            idx_data,
            step=max(2, args.index_step),
            window_bytes=min(args.index_window, len(idx_data)),
            sample_bytes=min(args.index_sample, len(idx_data)),
            top_k=args.index_top,
            loose=False,
        )

    report: Dict[str, object] = {
        "input": str(in_path),
        "verts_from": str(vtx_path) if args.verts_from else None,
        "idx_size": len(idx_data),
        "vtx_size": len(vtx_data),
        "index_candidates": [c.__dict__ for c in idx_cands],
        "exports": [],
    }

    if not idx_cands:
        (out_dir / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
        print(f"No index candidates found. Wrote: {out_dir / 'report.json'}")
        return 0

    for ci, ic in enumerate(idx_cands[: args.index_top]):
        # Parse indices using estimated count.
        ib = idx_data[ic.off : ic.off + ic.est_index_count * 2]
        indices = list(struct.unpack_from("<" + "H" * (len(ib) // 2), ib, 0))
        max_idx = max(indices) if indices else 0
        min_count = max_idx + 1

        if args.v_shift >= 0 and args.v_stride > 0 and args.v_posoff >= 0:
            vc = VertexCandidate(
                shift=args.v_shift,
                stride=args.v_stride,
                pos_off=args.v_posoff,
                count=max(0, (len(vtx_data) - args.v_shift - args.v_posoff) // args.v_stride),
                spread=(0, 0, 0),
                score=0.0,
            )
            if vc.count < min_count:
                continue
        else:
            v_cands = find_vertex_candidates_i16(
                vtx_data,
                min_count=min_count,
                strides=strides,
                shift_max=args.shift_max,
                posoff_max=args.posoff_max,
                top_k=3,
            )
            if not v_cands:
                continue
            vc = v_cands[0]

        # Decode vertices (only what indices need).
        need = min_count
        verts: List[Tuple[float, float, float]] = []
        valid: List[bool] = []
        for i in range(need):
            off = vc.shift + i * vc.stride + vc.pos_off
            t = i16_triplet_at(vtx_data, off)
            if not t:
                verts.append((0.0, 0.0, 0.0))
                valid.append(False)
                continue
            x, y, z = t
            verts.append((x * args.scale, y * args.scale, z * args.scale))
            valid.append(is_valid_i16_triplet(t))

        # Produce both interpretations for debugging.
        faces_tri_raw = indices_to_faces_triangles(indices, max_faces=args.max_faces)
        faces_strip_raw = indices_to_faces_strip(indices, max_faces=args.max_faces)
        tri_verts, faces_tri = filter_and_compact(verts, valid, faces_tri_raw)
        strip_verts, faces_strip = filter_and_compact(verts, valid, faces_strip_raw)

        base = (
            f"cand{ci:02d}_idx{in_path.stem}_idxOff{ic.off:06x}_idxN{ic.est_index_count}_max{max_idx}_"
            f"vtx{vtx_path.stem}_vShift{vc.shift}_stride{vc.stride}_pos{vc.pos_off}"
        )
        header = [
            f"input={in_path.name}",
            f"verts_from={vtx_path.name}",
            f"index_off=0x{ic.off:X} index_count={ic.est_index_count} max_index={max_idx}",
            f"vertex_shift={vc.shift} stride={vc.stride} pos_off={vc.pos_off} scale={args.scale}",
            f"faces_tri={len(faces_tri)} faces_strip={len(faces_strip)}",
        ]

        tri_path = out_dir / f"{base}.tri.obj"
        strip_path = out_dir / f"{base}.strip.obj"
        if faces_tri and tri_verts:
            write_obj(tri_path, tri_verts, faces_tri, header + ["prim=triangles"])
        if faces_strip and strip_verts:
            write_obj(strip_path, strip_verts, faces_strip, header + ["prim=triangle_strip"])

        report["exports"].append(
            {
                "candidate_index": ci,
                "index_off": ic.off,
                "index_count": ic.est_index_count,
                "max_index": max_idx,
                "vertex": vc.__dict__,
                "tri_obj": str(tri_path),
                "strip_obj": str(strip_path),
                "faces_tri": len(faces_tri),
                "faces_strip": len(faces_strip),
            }
        )

        if not args.export_all:
            # Only export the best candidate by default.
            break

    (out_dir / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"Wrote: {out_dir / 'report.json'}")
    for e in report["exports"]:
        print(f"OBJ: {e['tri_obj']}")
        print(f"OBJ: {e['strip_obj']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())


#!/usr/bin/env python3
"""
Reusable helpers for Gundam VS PSP resources.

- AFS parsing / extracting
- MWo3 analysis and pointer-guided vertex stream probing
- OBJ point cloud exporter
"""

from __future__ import annotations

import json
import math
import re
import struct
from pathlib import Path
from typing import Dict, List, Optional, Tuple


def read_u32_le(data: bytes, offset: int) -> int:
    if offset + 4 > len(data):
        return 0
    return struct.unpack_from("<I", data, offset)[0]


def c_string(data: bytes, offset: int, max_len: int) -> str:
    end = min(offset + max_len, len(data))
    raw = data[offset:end]
    pos = raw.find(b"\x00")
    if pos >= 0:
        raw = raw[:pos]
    return raw.decode("ascii", errors="replace")


def write_json(path: Optional[Path], obj: Dict) -> None:
    text = json.dumps(obj, ensure_ascii=False, indent=2)
    if path:
        path.write_text(text, encoding="utf-8")
    else:
        print(text)


def section_plan(file_size: int, len_a: int, len_b: int) -> Dict[str, Dict[str, int]]:
    plan: Dict[str, Dict[str, int]] = {}
    header_end = min(file_size, 0x100)
    plan["header"] = {"offset": 0, "size": header_end}

    a_off = 0x100
    if 0 < len_a <= file_size and a_off + len_a <= file_size:
        plan["section_a"] = {"offset": a_off, "size": len_a}

    b_off = a_off + len_a
    if 0 < len_b <= file_size and b_off + len_b <= file_size:
        plan["section_b"] = {"offset": b_off, "size": len_b}
    elif b_off < file_size:
        plan["tail"] = {"offset": b_off, "size": file_size - b_off}

    return plan


def f32_stats(data: bytes) -> Dict[str, float]:
    words = len(data) // 4
    if words <= 0:
        return {
            "word_count": 0,
            "finite_ratio": 0.0,
            "range_1k_ratio": 0.0,
            "range_100_ratio": 0.0,
            "near_unit_ratio": 0.0,
        }
    vals = struct.unpack("<" + "f" * words, data[: words * 4])
    finite = [v for v in vals if math.isfinite(v)]
    in_1k = [v for v in finite if -1000.0 <= v <= 1000.0]
    in_100 = [v for v in finite if -100.0 <= v <= 100.0]
    near_unit = [v for v in finite if -1.5 <= v <= 1.5]
    return {
        "word_count": words,
        "finite_ratio": round(len(finite) / words, 6),
        "range_1k_ratio": round(len(in_1k) / words, 6),
        "range_100_ratio": round(len(in_100) / words, 6),
        "near_unit_ratio": round(len(near_unit) / words, 6),
    }


def u16_stats(data: bytes) -> Dict[str, float]:
    words = len(data) // 2
    if words <= 0:
        return {
            "word_count": 0,
            "lt_256_ratio": 0.0,
            "lt_4096_ratio": 0.0,
            "zero_ratio": 0.0,
        }
    vals = struct.unpack("<" + "H" * words, data[: words * 2])
    lt_256 = sum(1 for x in vals if x < 256)
    lt_4096 = sum(1 for x in vals if x < 4096)
    zero = sum(1 for x in vals if x == 0)
    return {
        "word_count": words,
        "lt_256_ratio": round(lt_256 / words, 6),
        "lt_4096_ratio": round(lt_4096 / words, 6),
        "zero_ratio": round(zero / words, 6),
    }


def detect_triangle_like_u16(data: bytes, limit: int = 200000) -> Dict[str, int]:
    words = len(data) // 2
    vals = struct.unpack("<" + "H" * words, data[: words * 2]) if words else ()
    triplets = min(words // 3, limit // 3)
    hit = 0
    for i in range(triplets):
        a, b, c = vals[i * 3], vals[i * 3 + 1], vals[i * 3 + 2]
        if a != b and b != c and a != c and a < 30000 and b < 30000 and c < 30000:
            hit += 1
    return {
        "checked_triplets": triplets,
        "triangle_like_triplets": hit,
    }


def detect_weight_like_u8(data: bytes, limit: int = 200000) -> Dict[str, int]:
    groups = min(len(data) // 4, limit // 4)
    hit = 0
    for i in range(groups):
        b0 = data[i * 4]
        b1 = data[i * 4 + 1]
        b2 = data[i * 4 + 2]
        b3 = data[i * 4 + 3]
        s = b0 + b1 + b2 + b3
        if 240 <= s <= 270:
            hit += 1
    return {
        "checked_groups": groups,
        "weight_like_groups": hit,
    }


def analyze_mwo3(path: Path) -> Dict:
    data = path.read_bytes()
    file_size = len(data)
    magic = data[0:4].decode("ascii", errors="replace") if file_size >= 4 else ""
    h_u32 = [read_u32_le(data, i * 4) for i in range(16)]
    embedded_name = c_string(data, 0x20, 0x40) if file_size >= 0x21 else ""

    len_a = h_u32[4]
    len_b = h_u32[3]
    plan = section_plan(file_size, len_a, len_b)

    result = {
        "file": str(path),
        "size": file_size,
        "magic": magic,
        "embedded_name": embedded_name,
        "header_u32_0_15_hex": [hex(x) for x in h_u32],
        "sections": {},
        "global_float_stats": f32_stats(data),
        "global_u16_stats": u16_stats(data),
        "global_triangle_like": detect_triangle_like_u16(data),
        "global_weight_like": detect_weight_like_u8(data),
    }

    for sec_name, sec in plan.items():
        off = sec["offset"]
        size = sec["size"]
        chunk = data[off : off + size]
        result["sections"][sec_name] = {
            "offset": off,
            "size": size,
            "float_stats": f32_stats(chunk),
            "u16_stats": u16_stats(chunk),
            "triangle_like": detect_triangle_like_u16(chunk),
            "weight_like": detect_weight_like_u8(chunk),
        }

    return result


_OV_NAME_RE = re.compile(rb"pl(\d{2})ov([0-5])\.bin\x00", re.IGNORECASE)


def scan_w_data_ov_table(path: Path) -> Dict:
    data = path.read_bytes()
    hits = [m.start() for m in _OV_NAME_RE.finditer(data)]
    if not hits:
        return {"file": str(path), "error": "no_plXXovY_entries_found"}

    runs: List[List[int]] = []
    current = [hits[0]]
    for off in hits[1:]:
        if off - current[-1] == 0x30:
            current.append(off)
        else:
            runs.append(current)
            current = [off]
    runs.append(current)
    best = max(runs, key=len)

    entries = []
    for off in best:
        if off + 0x30 > len(data):
            continue
        row = data[off : off + 0x30]
        name = c_string(row, 0, 16).lower()
        if not re.fullmatch(r"pl\d{2}ov[0-5]\.bin", name):
            continue
        meta = struct.unpack("<8I", row[16:48])
        entries.append(
            {
                "offset": off,
                "name": name,
                "meta_u32_hex": [hex(x) for x in meta],
            }
        )

    grouped: Dict[str, Dict] = {}
    by_pl: Dict[int, List[Dict]] = {}
    for e in entries:
        m = re.fullmatch(r"pl(\d{2})ov([0-5])\.bin", e["name"])
        if not m:
            continue
        pid = int(m.group(1))
        ov = int(m.group(2))
        by_pl.setdefault(pid, []).append({"ov": ov, "size": int(e["meta_u32_hex"][7], 16), "entry": e})

    for pid, arr in sorted(by_pl.items()):
        arr = sorted(arr, key=lambda x: x["ov"])
        grouped[f"{pid:02d}"] = {
            "ov_list": [x["ov"] for x in arr],
            "sizes": sorted(set(x["size"] for x in arr)),
            "count": len(arr),
        }

    return {
        "file": str(path),
        "total_hits": len(hits),
        "best_run_length": len(best),
        "best_run_start": best[0] if best else None,
        "best_run_end": best[-1] if best else None,
        "entry_count": len(entries),
        "pl_count": len(grouped),
        "pl_summary": grouped,
        "entries_preview": entries[:20],
    }


def compare_files(paths: List[Path]) -> Dict:
    blobs = {p.name: p.read_bytes() for p in paths}
    names = list(blobs.keys())
    pairwise = {}
    for i in range(len(names)):
        for j in range(i + 1, len(names)):
            a = blobs[names[i]]
            b = blobs[names[j]]
            n = min(len(a), len(b))
            diff = sum(1 for k in range(n) if a[k] != b[k]) + abs(len(a) - len(b))
            ratio = diff / max(len(a), len(b), 1)
            pairwise[f"{names[i]} vs {names[j]}"] = {
                "diff_bytes": diff,
                "ratio": round(ratio, 6),
            }
    return {"pairwise_diff": pairwise}


def parse_afs(path: Path) -> Dict:
    data = path.read_bytes()
    size = len(data)
    if size < 8:
        return {"file": str(path), "error": "file_too_small"}
    magic = data[:4]
    if magic[:3] != b"AFS":
        return {"file": str(path), "error": "not_afs", "magic": magic.decode("ascii", "replace")}

    file_count = read_u32_le(data, 4)
    table_off = 8
    table_size = file_count * 8
    if table_off + table_size + 8 > size:
        return {"file": str(path), "error": "afs_table_out_of_range", "file_count": file_count}

    entries = []
    for i in range(file_count):
        off = read_u32_le(data, table_off + i * 8)
        sz = read_u32_le(data, table_off + i * 8 + 4)
        entries.append({"index": i, "offset": off, "size": sz})

    name_off = read_u32_le(data, table_off + table_size)
    name_size = read_u32_le(data, table_off + table_size + 4)
    name_table = None
    if 0 < name_off < size and 0 < name_size <= size - name_off:
        if name_size >= file_count * 0x30:
            name_table = {"offset": name_off, "size": name_size, "entry_size": 0x30}

    if name_table:
        names = []
        base = name_table["offset"]
        for i in range(file_count):
            row_off = base + i * 0x30
            if row_off + 0x30 > size:
                break
            row = data[row_off : row_off + 0x30]
            nm = c_string(row, 0, 0x20).lower()
            meta = struct.unpack("<4I", row[0x20:0x30])
            names.append(
                {
                    "index": i,
                    "name": nm,
                    "meta_u32_hex": [hex(x) for x in meta],
                }
            )
        for e in entries:
            idx = e["index"]
            if idx < len(names):
                e["name"] = names[idx]["name"]
                e["name_meta_u32_hex"] = names[idx]["meta_u32_hex"]

    return {
        "file": str(path),
        "magic": magic.decode("ascii", "replace"),
        "file_count": file_count,
        "table_offset": table_off,
        "name_table": name_table,
        "entries": entries,
    }


def afs_extract(path: Path, name: str, out_file: Path) -> Dict:
    afs = parse_afs(path)
    if "error" in afs:
        return afs
    data = path.read_bytes()
    target = None
    for e in afs["entries"]:
        if e.get("name") == name.lower():
            target = e
            break
    if not target:
        return {"file": str(path), "error": "entry_not_found", "name": name}
    off = target["offset"]
    sz = target["size"]
    if off + sz > len(data):
        return {"file": str(path), "error": "entry_out_of_range", "name": name, "offset": off, "size": sz}
    out_file.write_bytes(data[off : off + sz])
    return {
        "file": str(path),
        "extracted": str(out_file),
        "name": name,
        "offset": off,
        "size": sz,
        "magic4": data[off : off + 4].decode("ascii", "replace"),
    }


def _score_xyz_stream(
    data: bytes,
    start: int,
    stride: int,
    max_abs: float,
    min_norm: float,
    sample_records: int = 1024,
) -> Optional[Dict]:
    if stride < 12 or start < 0 or start + 12 > len(data):
        return None
    count = (len(data) - start) // stride
    if count < 64:
        return None

    valid = 0
    non_zero = 0
    preview: List[List[float]] = []
    n = min(count, sample_records)
    for i in range(n):
        off = start + i * stride
        x, y, z = struct.unpack_from("<fff", data, off)
        ok = (
            math.isfinite(x)
            and math.isfinite(y)
            and math.isfinite(z)
            and abs(x) <= max_abs
            and abs(y) <= max_abs
            and abs(z) <= max_abs
        )
        if not ok:
            continue
        valid += 1
        if abs(x) + abs(y) + abs(z) >= min_norm:
            non_zero += 1
        if len(preview) < 10:
            preview.append([round(x, 6), round(y, 6), round(z, 6)])

    if valid < 32:
        return None
    ratio = valid / n
    nz_ratio = non_zero / max(valid, 1)
    score = ratio * 0.75 + nz_ratio * 0.25
    if ratio < 0.70:
        return None
    return {
        "start": start,
        "stride": stride,
        "record_count": count,
        "valid_xyz_count": valid,
        "valid_ratio": round(ratio, 6),
        "non_zero_ratio": round(nz_ratio, 6),
        "score": round(score, 6),
        "preview": preview,
    }


def extract_vertices_pointer_guided(
    path: Path,
    limit: int = 5000,
    max_abs: float = 5000.0,
    min_norm: float = 0.01,
    pick: str = "best",
) -> Dict:
    data = path.read_bytes()
    if len(data) < 0x120:
        return {"file": str(path), "error": "file_too_small"}

    h_u32 = [read_u32_le(data, i * 4) for i in range(16)]
    base_addr = h_u32[2]
    end_addr = h_u32[6]
    is_mem_image = (
        0x08000000 <= base_addr <= 0x0AFFFFFF
        and 0x08000000 <= end_addr <= 0x0AFFFFFF
        and end_addr > base_addr
        and (end_addr - base_addr) == len(data)
    )

    len_a = h_u32[4]
    len_b = h_u32[3]
    plan = section_plan(len(data), len_a, len_b)

    strides = [12, 16, 20, 24, 28, 32, 36, 40, 48]
    candidates: List[Dict] = []

    if is_mem_image:
        u32_count = len(data) // 4
        ptrs = []
        for i in range(u32_count):
            v = struct.unpack_from("<I", data, i * 4)[0]
            if base_addr <= v < end_addr:
                ptrs.append(v)

        seen = set()
        ptrs_uniq = []
        for v in ptrs:
            if v in seen:
                continue
            seen.add(v)
            ptrs_uniq.append(v)
            if len(ptrs_uniq) >= 2000:
                break

        for v in ptrs_uniq:
            local_off = v - base_addr
            if local_off < 0x100 or local_off >= len(data):
                continue
            for stride in strides:
                s = _score_xyz_stream(
                    data=data,
                    start=local_off,
                    stride=stride,
                    max_abs=max_abs,
                    min_norm=min_norm,
                )
                if not s:
                    continue
                candidates.append(
                    {
                        "region": "ptr_target",
                        "ptr_value": hex(v),
                        "local_shift": local_off,
                        "global_start": local_off,
                        **s,
                    }
                )

    if not candidates:
        # Fallback: scan common sections (still uses MWo3 header sizes)
        regions: List[Tuple[str, int, bytes]] = []
        for key in ("section_a", "section_b", "tail", "header"):
            if key in plan:
                off = plan[key]["offset"]
                size = plan[key]["size"]
                if size >= 256:
                    regions.append((key, off, data[off : off + size]))
        payload_off = 0x100 if len(data) > 0x100 else 0
        if len(data) - payload_off >= 256:
            regions.append(("payload", payload_off, data[payload_off:]))

        for region_name, region_off, chunk in regions:
            for shift in (0, 4, 8, 12, 16):
                for stride in strides:
                    s = _score_xyz_stream(
                        data=chunk,
                        start=shift,
                        stride=stride,
                        max_abs=max_abs,
                        min_norm=min_norm,
                    )
                    if not s:
                        continue
                    candidates.append(
                        {
                            "region": region_name,
                            "region_offset": region_off,
                            "local_shift": shift,
                            "global_start": region_off + shift,
                            **s,
                        }
                    )

    if not candidates:
        return {
            "file": str(path),
            "error": "no_vertex_like_stream_found",
        }

    ranked = sorted(candidates, key=lambda x: (x["score"], x["valid_ratio"], x["record_count"]), reverse=True)
    if pick == "largest":
        chosen = sorted(ranked, key=lambda x: (x["record_count"], x["score"]), reverse=True)[0]
    else:
        chosen = ranked[0]

    start = int(chosen["global_start"])
    stride = int(chosen["stride"])
    max_count = min(int(chosen["record_count"]), max(1, limit))

    vertices: List[List[float]] = []
    for i in range(max_count):
        off = start + i * stride
        if off + 12 > len(data):
            break
        x, y, z = struct.unpack_from("<fff", data, off)
        ok = (
            math.isfinite(x)
            and math.isfinite(y)
            and math.isfinite(z)
            and abs(x) <= max_abs
            and abs(y) <= max_abs
            and abs(z) <= max_abs
            and (abs(x) + abs(y) + abs(z) >= min_norm)
        )
        if ok:
            vertices.append([round(x, 6), round(y, 6), round(z, 6)])

    return {
        "file": str(path),
        "magic": c_string(data, 0, 4),
        "embedded_name": c_string(data, 0x20, 0x40),
        "is_mem_image": is_mem_image,
        "base_addr_hex": hex(base_addr),
        "end_addr_hex": hex(end_addr),
        "min_norm": min_norm,
        "pick": pick,
        "chosen_stream": chosen,
        "candidate_streams_top10": ranked[:10],
        "vertex_count": len(vertices),
        "vertices": vertices,
    }


def write_obj_point_cloud(obj_path: Path, vertices: List[List[float]]) -> None:
    lines = ["# MWo3 point cloud", f"# vertex_count={len(vertices)}"]
    for x, y, z in vertices:
        lines.append(f"v {x} {y} {z}")
    obj_path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def is_mwo3_mem_image(data: bytes) -> Optional[Tuple[int, int]]:
    """
    MWo3 appears to store a "memory image" where file offsets correspond to
    in-game addresses: [base_addr, end_addr) and (end-base)==file_size.
    """
    if len(data) < 0x20:
        return None
    base_addr = read_u32_le(data, 8)   # h_u32[2]
    end_addr = read_u32_le(data, 24)   # h_u32[6]
    if (
        0x08000000 <= base_addr <= 0x0AFFFFFF
        and 0x08000000 <= end_addr <= 0x0AFFFFFF
        and end_addr > base_addr
        and (end_addr - base_addr) == len(data)
    ):
        return base_addr, end_addr
    return None


def build_pointer_xrefs(data: bytes, base: int, end: int) -> Dict[int, List[int]]:
    """
    Scan all u32 words and treat values within [base,end) as pointers.
    Return: {target_local_off: [src_local_off,...]}.
    """
    xrefs: Dict[int, List[int]] = {}
    n = len(data)
    for src in range(0, n - 4, 4):
        v = struct.unpack_from("<I", data, src)[0]
        if base <= v < end:
            tgt = v - base
            if 0 <= tgt < n:
                xrefs.setdefault(tgt, []).append(src)
    return xrefs


def scan_u16_index_runs(
    data: bytes,
    min_triplets: int = 256,
    max_index: int = 60000,
    step_bytes: int = 2,
    window_triplets: int = 1024,
) -> List[Dict]:
    """
    Find regions likely containing triangle index buffers (u16 triplets).
    Uses a sliding window and scores by triangle-like ratio and index range.
    """
    if len(data) < 6:
        return []
    vals = struct.unpack("<" + "H" * (len(data) // 2), data[: (len(data) // 2) * 2])
    results: List[Dict] = []
    w = max(64, window_triplets)
    step_words = max(1, step_bytes // 2)
    for word_start in range(0, len(vals) - 3 * w, step_words):
        tri = 0
        hit = 0
        vmax = 0
        bad = 0
        base_i = word_start
        for i in range(w):
            a = vals[base_i + i * 3]
            b = vals[base_i + i * 3 + 1]
            c = vals[base_i + i * 3 + 2]
            tri += 1
            if a == b or b == c or a == c:
                bad += 1
                continue
            if a > max_index or b > max_index or c > max_index:
                bad += 1
                continue
            hit += 1
            if a > vmax:
                vmax = a
            if b > vmax:
                vmax = b
            if c > vmax:
                vmax = c
        if tri <= 0:
            continue
        ratio = hit / tri
        if hit >= min_triplets and ratio >= 0.55 and vmax >= 64:
            off_bytes = word_start * 2
            results.append(
                {
                    "offset": off_bytes,
                    "window_triplets": w,
                    "hit_triplets": hit,
                    "hit_ratio": round(ratio, 6),
                    "max_index": int(vmax),
                }
            )
    # Keep top candidates (dedupe by coarse region)
    results.sort(key=lambda x: (x["hit_ratio"], x["hit_triplets"], x["max_index"]), reverse=True)
    picked: List[Dict] = []
    seen_regions = set()
    for r in results:
        key = r["offset"] // 0x200
        if key in seen_regions:
            continue
        seen_regions.add(key)
        picked.append(r)
        if len(picked) >= 30:
            break
    return picked


def summarize_pointer_targets(xrefs: Dict[int, List[int]], top: int = 50) -> List[Dict]:
    items = [{"target_off": k, "ref_count": len(v), "sources": v[:8]} for k, v in xrefs.items()]
    items.sort(key=lambda x: x["ref_count"], reverse=True)
    return items[:top]


def read_u32_window(data: bytes, off: int, count: int = 32) -> List[str]:
    """
    Read a u32 window for quick manual inspection.
    """
    out: List[str] = []
    for i in range(count):
        o = off + i * 4
        if o + 4 > len(data):
            break
        out.append(hex(read_u32_le(data, o)))
    return out


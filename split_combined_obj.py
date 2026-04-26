#!/usr/bin/env python3
"""
Split a combined OBJ (with many 'o <name>' objects) into separate OBJ files.

This is designed for PPSSPP frame exports where everything is stacked together
and hard to identify in Blender.

Input assumptions:
  - The OBJ uses sequential global vertex indices.
  - Each object starts with an 'o <name>' line.
  - Faces are 'f i j k' (optionally with /vt/vn), referencing the global vertex list.

Output:
  - Writes selected objects to <out_dir>/<rank>_<name>_v<verts>_f<faces>.obj
  - Writes an index JSON: <out_dir>/index.json sorted by faces desc
"""

from __future__ import annotations

import argparse
import json
import math
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Dict, List, Optional, Tuple


@dataclass
class ObjPart:
    name: str
    base_v: int  # 1-based global vertex index at start of this object
    verts: List[Tuple[float, float, float]]
    faces: List[Tuple[int, int, int]]  # 1-based, local to this part
    bbox_min: Optional[Tuple[float, float, float]] = None
    bbox_max: Optional[Tuple[float, float, float]] = None

    def update_bbox(self, v: Tuple[float, float, float]) -> None:
        if self.bbox_min is None or self.bbox_max is None:
            self.bbox_min = v
            self.bbox_max = v
            return
        mn = self.bbox_min
        mx = self.bbox_max
        self.bbox_min = (min(mn[0], v[0]), min(mn[1], v[1]), min(mn[2], v[2]))
        self.bbox_max = (max(mx[0], v[0]), max(mx[1], v[1]), max(mx[2], v[2]))

    def diag(self) -> float:
        if self.bbox_min is None or self.bbox_max is None:
            return 0.0
        mn = self.bbox_min
        mx = self.bbox_max
        dx = mx[0] - mn[0]
        dy = mx[1] - mn[1]
        dz = mx[2] - mn[2]
        return math.sqrt(dx * dx + dy * dy + dz * dz)


def sanitize_name(name: str) -> str:
    name = name.strip()
    if not name:
        return "unnamed"
    name = re.sub(r"[^0-9A-Za-z._-]+", "_", name)
    return name[:120]


def parse_face_vertex(token: str) -> Optional[int]:
    # token can be "v", "v/vt", "v//vn", "v/vt/vn"
    if not token:
        return None
    v = token.split("/", 1)[0]
    try:
        return int(v)
    except ValueError:
        return None


def write_part_obj(path: Path, part: ObjPart) -> None:
    lines: List[str] = []
    lines.append(f"# split from combined obj")
    lines.append(f"# name={part.name} verts={len(part.verts)} faces={len(part.faces)} diag={part.diag():.6f}")
    if part.bbox_min and part.bbox_max:
        lines.append(f"# bbox_min={part.bbox_min[0]:.6f},{part.bbox_min[1]:.6f},{part.bbox_min[2]:.6f}")
        lines.append(f"# bbox_max={part.bbox_max[0]:.6f},{part.bbox_max[1]:.6f},{part.bbox_max[2]:.6f}")
    lines.append(f"o {part.name}")
    for x, y, z in part.verts:
        lines.append(f"v {x:.6f} {y:.6f} {z:.6f}")
    for a, b, c in part.faces:
        lines.append(f"f {a} {b} {c}")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def write_combined_obj(path: Path, parts: List[ObjPart]) -> None:
    lines: List[str] = []
    lines.append("# combined filtered objects")
    v_base = 0
    for p in parts:
        lines.append(f"o {p.name}")
        for x, y, z in p.verts:
            lines.append(f"v {x:.6f} {y:.6f} {z:.6f}")
        for a, b, c in p.faces:
            lines.append(f"f {a + v_base} {b + v_base} {c + v_base}")
        v_base += len(p.verts)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def split_obj(in_path: Path) -> List[ObjPart]:
    text = in_path.read_text(encoding="utf-8", errors="replace")
    parts: List[ObjPart] = []
    current: Optional[ObjPart] = None
    global_v = 0  # count of all 'v' seen so far

    for raw_line in text.splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue
        if line.startswith("o "):
            name = sanitize_name(line[2:])
            current = ObjPart(name=name, base_v=global_v + 1, verts=[], faces=[])
            parts.append(current)
            continue
        if current is None:
            # If no object header, create a default one.
            current = ObjPart(name="default", base_v=global_v + 1, verts=[], faces=[])
            parts.append(current)

        if line.startswith("v "):
            # v x y z
            fields = line.split()
            if len(fields) < 4:
                continue
            try:
                x = float(fields[1])
                y = float(fields[2])
                z = float(fields[3])
            except ValueError:
                continue
            current.verts.append((x, y, z))
            current.update_bbox((x, y, z))
            global_v += 1
            continue

        if line.startswith("f "):
            fields = line.split()
            idxs: List[int] = []
            for tok in fields[1:]:
                v = parse_face_vertex(tok)
                if v is None:
                    continue
                idxs.append(v)
            if len(idxs) < 3:
                continue
            # Triangulate polygons (fan) if needed.
            base = idxs[0]
            for i in range(1, len(idxs) - 1):
                a, b, c = base, idxs[i], idxs[i + 1]
                # Convert global 1-based -> local 1-based by subtracting (base_v-1)
                a -= current.base_v - 1
                b -= current.base_v - 1
                c -= current.base_v - 1
                if a <= 0 or b <= 0 or c <= 0:
                    continue
                current.faces.append((a, b, c))
            continue

    return parts


def main() -> int:
    ap = argparse.ArgumentParser(description="Split a combined OBJ into per-object OBJ files.")
    ap.add_argument("--in", dest="in_path", default="test/ppsspp_dump/record_mesh_combined/frame_combined.obj")
    ap.add_argument("--out-dir", default="test/ppsspp_dump/record_mesh_split")
    ap.add_argument("--min-faces", type=int, default=50)
    ap.add_argument("--min-diag", type=float, default=0.0)
    ap.add_argument("--max-diag", type=float, default=0.0, help="If > 0, skip objects with diag larger than this.")
    ap.add_argument("--top", type=int, default=200, help="Max objects to write (sorted by faces desc).")
    args = ap.parse_args()

    in_path = Path(args.in_path)
    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)

    parts = split_obj(in_path)
    rows: List[Dict[str, object]] = []
    for p in parts:
        rows.append(
            {
                "name": p.name,
                "verts": len(p.verts),
                "faces": len(p.faces),
                "diag": p.diag(),
                "bbox_min": p.bbox_min,
                "bbox_max": p.bbox_max,
            }
        )

    rows.sort(key=lambda r: int(r["faces"]), reverse=True)

    written = 0
    selected_parts: List[ObjPart] = []
    for rank, r in enumerate(rows):
        if written >= args.top:
            break
        faces = int(r["faces"])
        diag = float(r["diag"])
        if faces < args.min_faces:
            continue
        if diag < args.min_diag:
            continue
        if args.max_diag > 0.0 and diag > args.max_diag:
            continue
        name = str(r["name"])
        # Find part by name (names are unique in our exporter; keep a fallback scan.)
        part = next((p for p in parts if p.name == name), None)
        if part is None:
            continue
        fn = f"{written:04d}_faces{faces:06d}_diag{diag:.3f}_{sanitize_name(name)}.obj"
        write_part_obj(out_dir / fn, part)
        r["obj"] = str((out_dir / fn))
        selected_parts.append(part)
        written += 1

    if selected_parts:
        write_combined_obj(out_dir / "top_combined.obj", selected_parts)

    index = {
        "input": str(in_path),
        "out_dir": str(out_dir),
        "min_faces": args.min_faces,
        "min_diag": args.min_diag,
        "max_diag": args.max_diag,
        "top": args.top,
        "total_objects": len(rows),
        "written": written,
        "objects_sorted": rows[: min(len(rows), 2000)],
    }
    (out_dir / "index.json").write_text(json.dumps(index, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"Split {len(rows)} objects, wrote {written} objs to: {out_dir}")
    print(f"Wrote index: {out_dir / 'index.json'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())


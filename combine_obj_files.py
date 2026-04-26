#!/usr/bin/env python3
"""
Combine multiple OBJ mesh files into one OBJ, preserving per-file objects.

This is meant to reduce "too many files / too many stacked things" when inspecting
reconstructed meshes (e.g. from PZZ stream indices + vertex blobs).

Supported input:
  - 'v x y z'
  - 'f i j k' or 'f i/... j/... k/...'
  - Ignores other lines.

Output:
  - One combined OBJ with 'o <name>' per input file.
  - One index JSON with faces/verts/bbox/diag for each object.
"""

from __future__ import annotations

import argparse
import glob
import json
import math
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Dict, List, Optional, Tuple


def sanitize_name(name: str) -> str:
    name = name.strip()
    if not name:
        return "unnamed"
    name = re.sub(r"[^0-9A-Za-z._-]+", "_", name)
    return name[:120]


def parse_face_vertex(token: str) -> Optional[int]:
    if not token:
        return None
    v = token.split("/", 1)[0]
    try:
        return int(v)
    except ValueError:
        return None


@dataclass
class ObjMesh:
    name: str
    verts: List[Tuple[float, float, float]]
    faces: List[Tuple[int, int, int]]  # 1-based indices into verts
    source: str
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


def load_obj(path: Path) -> ObjMesh:
    text = path.read_text(encoding="utf-8", errors="replace")
    name = sanitize_name(path.stem)
    verts: List[Tuple[float, float, float]] = []
    faces: List[Tuple[int, int, int]] = []
    mesh = ObjMesh(name=name, verts=verts, faces=faces, source=str(path))

    for raw in text.splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        if line.startswith("v "):
            parts = line.split()
            if len(parts) < 4:
                continue
            try:
                x = float(parts[1])
                y = float(parts[2])
                z = float(parts[3])
            except ValueError:
                continue
            verts.append((x, y, z))
            mesh.update_bbox((x, y, z))
        elif line.startswith("f "):
            parts = line.split()
            idxs: List[int] = []
            for tok in parts[1:]:
                v = parse_face_vertex(tok)
                if v is None:
                    continue
                idxs.append(v)
            if len(idxs) < 3:
                continue
            # Triangulate polygons if any.
            base = idxs[0]
            for i in range(1, len(idxs) - 1):
                a, b, c = base, idxs[i], idxs[i + 1]
                if a <= 0 or b <= 0 or c <= 0:
                    continue
                faces.append((a, b, c))

    return mesh


def write_combined_obj(path: Path, meshes: List[ObjMesh], header: List[str]) -> None:
    lines: List[str] = []
    for h in header:
        lines.append(f"# {h}")
    v_base = 0
    for m in meshes:
        lines.append(f"o {m.name}")
        for x, y, z in m.verts:
            lines.append(f"v {x:.6f} {y:.6f} {z:.6f}")
        for a, b, c in m.faces:
            lines.append(f"f {a + v_base} {b + v_base} {c + v_base}")
        v_base += len(m.verts)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> int:
    ap = argparse.ArgumentParser(description="Combine multiple OBJ meshes into one OBJ.")
    ap.add_argument("--out-obj", required=True, help="Output combined OBJ path")
    ap.add_argument("--out-index", required=True, help="Output index JSON path")
    ap.add_argument("--inputs", nargs="+", required=True, help="Input OBJ files")
    args = ap.parse_args()

    # Expand wildcards passed by PowerShell (which does not expand them for external programs.)
    raw_inputs: List[str] = list(args.inputs)
    expanded_inputs: List[str] = []
    for raw in raw_inputs:
        if any(ch in raw for ch in ["*", "?", "["]):
            expanded_inputs.extend(sorted(glob.glob(raw)))
        else:
            expanded_inputs.append(raw)
    in_paths = [Path(p) for p in expanded_inputs]
    meshes: List[ObjMesh] = []
    for p in in_paths:
        if not p.exists():
            continue
        m = load_obj(p)
        if len(m.verts) < 3 or len(m.faces) < 1:
            continue
        meshes.append(m)

    meshes.sort(key=lambda m: (len(m.faces), m.diag()), reverse=True)

    out_obj = Path(args.out_obj)
    out_obj.parent.mkdir(parents=True, exist_ok=True)
    out_index = Path(args.out_index)
    out_index.parent.mkdir(parents=True, exist_ok=True)

    header = [
        "combined_from_multiple_obj_files",
        f"mesh_count={len(meshes)}",
    ]
    write_combined_obj(out_obj, meshes, header=header)

    index: Dict[str, object] = {
        "out_obj": str(out_obj),
        "mesh_count": len(meshes),
        "items": [
            {
                "name": m.name,
                "verts": len(m.verts),
                "faces": len(m.faces),
                "diag": m.diag(),
                "bbox_min": m.bbox_min,
                "bbox_max": m.bbox_max,
                "source": m.source,
            }
            for m in meshes
        ],
        "inputs_raw": raw_inputs,
        "inputs_expanded": [str(p) for p in in_paths],
    }
    out_index.write_text(json.dumps(index, ensure_ascii=False, indent=2), encoding="utf-8")

    print(f"Wrote: {out_obj}")
    print(f"Wrote: {out_index}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())


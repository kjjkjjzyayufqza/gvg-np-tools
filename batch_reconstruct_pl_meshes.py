#!/usr/bin/env python3
"""
Batch reconstruct meshes for multiple pl?? PZZ bundles after pzz_zlib_harvest.py.

This uses the discovered pattern:
  - Vertex positions are often in a large stream006_off*.bin with int16 xyz.
  - Indices for parts are often in a smaller stream008_off*.bin (u16 indices).
  - Some parts are fully self-contained in stream007_off*.bin (indices + vertices).

We do not rely on PMF2 yet; this is a pragmatic bridge to get real triangle meshes.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Dict, List, Optional, Tuple

import reconstruct_mesh_from_stream as recon


def pick_stream(dir_path: Path, prefix: str) -> Optional[Path]:
    cands = sorted(dir_path.glob(prefix + "*.bin"))
    return cands[0] if cands else None


def main() -> int:
    ap = argparse.ArgumentParser(description="Batch mesh reconstruction for pl?? bundles.")
    ap.add_argument("--harvest-dir", default="test/pzz_harvest_out", help="Directory created by pzz_zlib_harvest.py")
    ap.add_argument("--out-dir", default="test/pzz_mesh_recon_out", help="Output directory")
    ap.add_argument("--pls", default="pl00,pl10,pl41", help="Comma-separated PZZ dirs to process")
    ap.add_argument("--v-shift", type=int, default=2044)
    ap.add_argument("--v-stride", type=int, default=6)
    ap.add_argument("--v-posoff", type=int, default=0)
    ap.add_argument("--scale", type=float, default=1.0 / 256.0)
    ap.add_argument("--max-faces", type=int, default=200000)
    args = ap.parse_args()

    harvest_dir = Path(args.harvest_dir)
    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)

    pls = [p.strip() for p in args.pls.split(",") if p.strip()]
    report: Dict[str, object] = {"harvest_dir": str(harvest_dir), "out_dir": str(out_dir), "items": []}

    for pl in pls:
        pl_dir = harvest_dir / pl
        if not pl_dir.exists():
            report["items"].append({"pl": pl, "error": "dir_not_found"})
            continue

        vtx = pick_stream(pl_dir, "stream006_off")
        idx = pick_stream(pl_dir, "stream008_off")
        self_geom = pick_stream(pl_dir, "stream007_off")

        item: Dict[str, object] = {"pl": pl, "vertex_stream": str(vtx) if vtx else None, "index_stream": str(idx) if idx else None}

        exports: List[str] = []
        if vtx and idx:
            # Reconstruct main mesh parts (indices in idx, vertices in vtx).
            out_sub = out_dir / pl / "idx_stream008"
            out_sub.mkdir(parents=True, exist_ok=True)
            recon.main(
                argv=[
                    "--in",
                    str(idx),
                    "--verts-from",
                    str(vtx),
                    "--out-dir",
                    str(out_sub),
                    "--max-faces",
                    str(args.max_faces),
                    "--index-top",
                    "5",
                    "--index-window",
                    "0x4000",
                    "--index-sample",
                    "0x1000",
                    "--index-step",
                    "16",
                    "--scale",
                    str(args.scale),
                    "--v-shift",
                    str(args.v_shift),
                    "--v-stride",
                    str(args.v_stride),
                    "--v-posoff",
                    str(args.v_posoff),
                ]
            )
            exports.append(str(out_sub))

        if self_geom:
            # Reconstruct possible self-contained parts.
            out_sub = out_dir / pl / "self_stream007"
            out_sub.mkdir(parents=True, exist_ok=True)
            recon.main(
                argv=[
                    "--in",
                    str(self_geom),
                    "--out-dir",
                    str(out_sub),
                    "--max-faces",
                    str(args.max_faces),
                    "--index-top",
                    "5",
                    "--index-window",
                    "0x4000",
                    "--index-sample",
                    "0x1000",
                    "--index-step",
                    "16",
                    "--scale",
                    str(args.scale),
                ]
            )
            exports.append(str(out_sub))

        item["exports"] = exports
        report["items"].append(item)

    (out_dir / "batch_report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"Wrote: {out_dir / 'batch_report.json'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())


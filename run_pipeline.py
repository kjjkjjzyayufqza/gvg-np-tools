#!/usr/bin/env python3
# ⚠️ DEPRECATED — 请使用 rust_converter/ 中的 Rust 版本
# Usage: cd rust_converter && cargo build --release
#        ./target/release/gvg_converter pipeline Z_DATA.BIN inventory.json
"""
DEPRECATED: Full pipeline: PZZ -> FBX -> PMF2 -> PZZ -> Z_DATA_1.BIN
Use the Rust version instead: rust_converter/
"""
from __future__ import annotations

import json
import struct
import sys
from pathlib import Path

Z_DATA_BIN = Path("Z_DATA.BIN")
INVENTORY = Path("data_bin_inventory/Z_DATA.BIN.inventory.json")
PZZ_NAME = "pl00.pzz"
OUT_DIR = Path("pipeline_out")
OUTPUT_BIN = Path("Z_DATA_1.BIN")


def step(name: str):
    print(f"\n{'='*60}")
    print(f"  {name}")
    print(f"{'='*60}")


def main():
    from gvg_converter import extract_pzz_streams, classify_stream, find_pzz_key
    from pmf2_fbx import export_pzz_to_fbx, rebuild_pmf2_from_meta
    from pzz_repacker import repack_pzz_with_replaced_stream, full_repack

    if not Z_DATA_BIN.exists():
        print(f"ERROR: {Z_DATA_BIN} not found")
        return 1
    if not INVENTORY.exists():
        print(f"ERROR: {INVENTORY} not found")
        return 1

    inv = json.loads(INVENTORY.read_text(encoding="utf-8"))
    target = None
    for e in inv["entries"]:
        if e.get("name", "").lower() == PZZ_NAME.lower():
            target = e
            break

    if target is None:
        print(f"ERROR: {PZZ_NAME} not found in inventory")
        return 1

    print(f"Target: {PZZ_NAME}")
    print(f"  Index: {target['index']}")
    print(f"  Offset: {target['offset']}")
    print(f"  Size: {target['size']}")

    step("1. Extract PZZ from Z_DATA.BIN")
    with Z_DATA_BIN.open("rb") as f:
        f.seek(target["offset"])
        pzz_data = f.read(target["size"])
    print(f"  Read {len(pzz_data)} bytes")

    pzz_out = OUT_DIR / "original_pl00.pzz"
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    pzz_out.write_bytes(pzz_data)
    print(f"  Saved: {pzz_out}")

    step("2. Export PZZ to FBX")
    fbx_dir = OUT_DIR / "pl00_fbx"
    export_result = export_pzz_to_fbx(pzz_data, fbx_dir, model_name="pl00")
    if "error" in export_result:
        print(f"  ERROR: {export_result['error']}")
        return 1

    print(f"  Streams: {export_result['stream_count']}")
    print(f"  Textures exported: {len(export_result['exported_textures'])}")
    for fbx_info in export_result["exported_fbx"]:
        if "error" not in fbx_info:
            print(f"  FBX: {fbx_info.get('fbx_file', 'N/A')}")
            print(f"    Vertices: {fbx_info.get('total_vertices', 0)}")
            print(f"    Faces: {fbx_info.get('total_faces', 0)}")
            print(f"    Bones: {fbx_info.get('bone_count', 0)}")
            print(f"    Mesh parts: {fbx_info.get('mesh_parts', 0)}")
        else:
            print(f"  FBX (stream {fbx_info.get('pmf2_stream_index', '?')}): {fbx_info['error']}")

    step("3. Verify FBX files created")
    fbx_files = list(fbx_dir.glob("*.fbx"))
    meta_files = list(fbx_dir.glob("*.pmf2meta.json"))
    print(f"  FBX files: {len(fbx_files)}")
    for f in fbx_files:
        size = f.stat().st_size
        print(f"    {f.name}: {size:,} bytes")
    print(f"  Meta files: {len(meta_files)}")

    step("4. Round-trip: FBX -> PMF2")
    streams = extract_pzz_streams(pzz_data)
    stream_types = [(i, classify_stream(s)) for i, s in enumerate(streams)]
    pmf2_indices = [i for i, t in stream_types if t == "pmf2"]

    rebuilt_streams = list(streams)
    for meta_file in sorted(meta_files):
        meta = json.loads(meta_file.read_text(encoding="utf-8"))

        fbx_name = meta_file.stem.replace(".pmf2meta", "")
        for fbx_info in export_result["exported_fbx"]:
            if fbx_info.get("meta_file", "").endswith(meta_file.name):
                pmf2_idx = fbx_info["pmf2_stream_index"]
                break
        else:
            continue

        print(f"  Rebuilding PMF2 from {meta_file.name} -> stream {pmf2_idx}")
        new_pmf2 = rebuild_pmf2_from_meta(meta_file)
        print(f"    Original PMF2 size: {len(streams[pmf2_idx])} bytes")
        print(f"    Rebuilt PMF2 size:  {len(new_pmf2)} bytes")

        rebuilt_pmf2_path = OUT_DIR / f"rebuilt_stream{pmf2_idx:03d}.pmf2"
        rebuilt_pmf2_path.write_bytes(new_pmf2)
        print(f"    Saved: {rebuilt_pmf2_path}")

        if new_pmf2[:4] == b"PMF2":
            nsec = struct.unpack_from("<I", new_pmf2, 4)[0]
            print(f"    Magic: PMF2, Sections: {nsec}")
        else:
            print(f"    WARNING: Invalid magic: {new_pmf2[:4]}")

        rebuilt_streams[pmf2_idx] = new_pmf2

    step("5. Verify rebuilt PMF2 round-trip")
    from pmf2_fbx import extract_per_bone_meshes
    for pmf2_idx in pmf2_indices:
        orig_data = streams[pmf2_idx]
        rebuilt_data = rebuilt_streams[pmf2_idx]

        orig_meshes, orig_secs, orig_bbox, _ = extract_per_bone_meshes(orig_data, swap_yz=False)
        rebuilt_meshes, rebuilt_secs, rebuilt_bbox, _ = extract_per_bone_meshes(rebuilt_data, swap_yz=False)

        print(f"  Stream {pmf2_idx}:")
        print(f"    Original: {len(orig_secs)} bones, {len(orig_meshes)} mesh parts, bbox={orig_bbox}")
        print(f"    Rebuilt:  {len(rebuilt_secs)} bones, {len(rebuilt_meshes)} mesh parts, bbox={rebuilt_bbox}")

        orig_verts = sum(len(m.vertices) for m in orig_meshes)
        rebuilt_verts = sum(len(m.vertices) for m in rebuilt_meshes)
        orig_faces = sum(len(m.faces) for m in orig_meshes)
        rebuilt_faces = sum(len(m.faces) for m in rebuilt_meshes)
        print(f"    Original verts/faces: {orig_verts}/{orig_faces}")
        print(f"    Rebuilt verts/faces:  {rebuilt_verts}/{rebuilt_faces}")

    step("6. Repack into PZZ")
    new_pzz_path = OUT_DIR / "rebuilt_pl00.pzz"

    key = find_pzz_key(pzz_data, len(pzz_data))
    print(f"  Original PZZ key: 0x{key:08X}" if key else "  WARNING: Could not find key")

    from pzz_repacker import build_pzz
    new_pzz = build_pzz(rebuilt_streams, original_key=key)
    new_pzz_path.write_bytes(new_pzz)
    print(f"  Original PZZ: {len(pzz_data):,} bytes")
    print(f"  Rebuilt PZZ:  {len(new_pzz):,} bytes")
    print(f"  Saved: {new_pzz_path}")

    verify_streams = extract_pzz_streams(new_pzz)
    print(f"  Verification: extracted {len(verify_streams)} streams from rebuilt PZZ")
    for i, s in enumerate(verify_streams):
        ct = classify_stream(s)
        orig_ct = classify_stream(streams[i]) if i < len(streams) else "?"
        status = "OK" if ct == orig_ct else f"MISMATCH (was {orig_ct})"
        print(f"    Stream {i}: {ct} ({len(s)} bytes) - {status}")

    step("7. Patch Z_DATA.BIN -> Z_DATA_1.BIN")
    result = full_repack(
        Z_DATA_BIN, INVENTORY,
        PZZ_NAME, new_pzz, OUTPUT_BIN,
    )
    print(f"  Result: {json.dumps(result, indent=2, default=str)}")

    step("DONE")
    print(f"  FBX output:    {fbx_dir}")
    print(f"  Rebuilt PZZ:   {new_pzz_path}")
    print(f"  New Z_DATA:    {OUTPUT_BIN}")

    return 0


if __name__ == "__main__":
    sys.exit(main())

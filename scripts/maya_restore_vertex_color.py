"""
maya_restore_vertex_color.py
Restore vertex color RGBA (especially alpha/transparency) to Maya meshes
from the original pmf2meta.json sidecar.

Maya's Collada importer silently converts RGBA vertex color sets to RGB,
dropping the alpha channel.  This script reads the ground-truth RGBA data
from the pmf2meta.json that was exported alongside the DAE and writes it
back onto the matching Maya meshes so that the subsequent FBX export
carries correct alpha values through the pipeline.

Usage
-----
1. Import your DAE into Maya.
2. Open Script Editor  (Windows > General Editors > Script Editor).
3. Python tab, run ONE of:

   a) execfile(r"E:\\research\\gvg_np\\scripts\\maya_restore_vertex_color.py")

   b) Or copy-paste the whole file into the editor and press Ctrl+Enter.

4. Check the output log -- it will list every mesh it touched and how many
   vertices had alpha < 1.
5. Continue editing, then export FBX as usual.

The script is fully automatic -- no mesh selection required.
"""

import maya.cmds as cmds
import json

# ===== CONFIGURATION (edit these before running) ==========================

# Path to the pmf2meta.json that contains the ORIGINAL vertex colors.
# This is the sidecar generated when you first exported PMF2 -> DAE.
META_PATH = r"E:\research\gvg_np\game_assets\z_data\stream000.pmf2meta.json"

# Squared-distance threshold for the position-based vertex matching
# fallback (used only when Maya vertex count differs from the meta).
POS_TOLERANCE_SQ = 1.0

# =========================================================================


def _find_matching_bone(xform_name, lookup):
    """Return (bone_name, data) or (None, None).  Prefer exact match."""
    if xform_name in lookup:
        return xform_name, lookup[xform_name]
    for bn, data in lookup.items():
        if bn in xform_name:
            return bn, data
    return None, None


def _ensure_rgba_color_set(shape):
    """Make sure *shape* has a current RGBA color set.  Returns set name."""
    cs_list = cmds.polyColorSet(shape, q=True, allColorSets=True) or []
    if cs_list:
        cs = cs_list[0]
    else:
        cs = "colorSet1"
        cmds.polyColorSet(shape, create=True, colorSet=cs,
                          representation="RGBA")
    cmds.polyColorSet(shape, currentColorSet=True, colorSet=cs)
    cmds.polyColorSet(shape, colorSet=cs, representation="RGBA")
    return cs


def _apply_by_index(shape, vc, nvtx):
    """Fast path: vertex counts match, apply color by index."""
    for i in range(nvtx):
        r, g, b, a = vc[i]
        cmds.polyColorPerVertex(
            "%s.vtx[%d]" % (shape, i),
            rgb=(r, g, b), a=a, cdo=True)
    return nvtx


def _apply_by_position(shape, lv, vc, nvtx):
    """Slow path: match each Maya vertex to nearest meta vertex by position."""
    applied = 0
    for vi in range(nvtx):
        pos = cmds.pointPosition("%s.vtx[%d]" % (shape, vi), local=True)
        best_i, best_d = -1, POS_TOLERANCE_SQ
        for mi in range(len(lv)):
            dx = pos[0] - lv[mi][0]
            dy = pos[1] - lv[mi][1]
            dz = pos[2] - lv[mi][2]
            d2 = dx * dx + dy * dy + dz * dz
            if d2 < best_d:
                best_i, best_d = mi, d2
        if best_i >= 0:
            r, g, b, a = vc[best_i]
            cmds.polyColorPerVertex(
                "%s.vtx[%d]" % (shape, vi),
                rgb=(r, g, b), a=a, cdo=True)
            applied += 1
    return applied


def restore_vertex_colors(meta_path=META_PATH):
    with open(meta_path) as f:
        meta = json.load(f)

    lookup = {}
    for bm in meta.get("bone_meshes", []):
        vc = bm.get("vertex_colors_rgba")
        if not vc:
            continue
        lv = bm.get("local_vertices", [])
        if len(vc) != len(lv):
            continue
        lookup[bm["bone_name"]] = (lv, vc)

    if not lookup:
        print("[restore] No vertex colors in meta -- nothing to do.")
        return

    total_alpha = sum(
        sum(1 for c in vc if abs(c[3] - 1.0) > 0.01)
        for _, vc in lookup.values()
    )
    print("[restore] Loaded %d bone meshes (%d vertices with alpha<1) from:\n  %s"
          % (len(lookup), total_alpha, meta_path))

    all_shapes = cmds.ls(type="mesh", long=True) or []
    if not all_shapes:
        print("[restore] No meshes in scene.")
        return

    cmds.undoInfo(openChunk=True)
    restored = 0
    try:
        for shape in all_shapes:
            parents = cmds.listRelatives(shape, parent=True, fullPath=True)
            if not parents:
                continue
            xform = parents[0].split("|")[-1]

            bone_name, data = _find_matching_bone(xform, lookup)
            if data is None:
                continue

            lv, vc = data
            nvtx = cmds.polyEvaluate(shape, vertex=True)
            if nvtx == 0:
                continue

            _ensure_rgba_color_set(shape)

            if nvtx == len(lv):
                applied = _apply_by_index(shape, vc, nvtx)
                method = "index"
            else:
                applied = _apply_by_position(shape, lv, vc, nvtx)
                method = "position(%d->%d)" % (len(lv), nvtx)

            alpha_count = sum(1 for c in vc if abs(c[3] - 1.0) > 0.01)
            print("  %-25s -> %-20s  %4d vtx applied (%s), %d alpha<1"
                  % (xform, bone_name, applied, method, alpha_count))
            restored += 1
    finally:
        cmds.undoInfo(closeChunk=True)

    print("[restore] Done. Restored vertex colors on %d / %d meshes."
          % (restored, len(lookup)))


restore_vertex_colors()

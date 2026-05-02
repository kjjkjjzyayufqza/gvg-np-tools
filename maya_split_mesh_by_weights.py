"""
Maya 2026 Python Script
- Step 1: Log all scene assets
- Step 2: Find mesh named 'ms'
- Step 3: Split 'ms' into separate meshes based on dominant skin weights

Usage: Copy/paste into Maya Script Editor (Python tab) and run.
"""

import maya.cmds as cmds
import maya.api.OpenMaya as om2
import maya.api.OpenMayaAnim as oma2
from collections import defaultdict


# ─── Step 1: Scene Asset Report ──────────────────────────────────────

def log_scene_assets():
    """Print a categorized summary of every asset in the current scene."""

    print("\n" + "=" * 70)
    print("  SCENE ASSET REPORT")
    print("=" * 70)

    type_table = [
        ("Transforms",     "transform"),
        ("Meshes",         "mesh"),
        ("Joints",         "joint"),
        ("Skin Clusters",  "skinCluster"),
        ("Blend Shapes",   "blendShape"),
        ("Nurbs Curves",   "nurbsCurve"),
        ("Nurbs Surfaces", "nurbsSurface"),
        ("Locators",       "locator"),
        ("Cameras",        "camera"),
    ]

    for label, node_type in type_table:
        nodes = cmds.ls(type=node_type) or []
        print("\n  [{0}] ({1})".format(label, len(nodes)))
        if not nodes:
            print("    (none)")
            continue

        for n in nodes:
            extra = ""
            if node_type == "mesh":
                parent = cmds.listRelatives(n, parent=True, fullPath=False)
                try:
                    vtx = cmds.polyEvaluate(n, vertex=True)
                except Exception:
                    vtx = "?"
                extra = "  |  parent={0}  verts={1}".format(
                    parent[0] if parent else "?", vtx
                )
            elif node_type == "skinCluster":
                try:
                    geo = cmds.skinCluster(n, q=True, geometry=True) or []
                    inf = cmds.skinCluster(n, q=True, influence=True) or []
                    extra = "  |  geo={0}  influences={1}".format(geo, len(inf))
                except Exception:
                    pass
            print("    {0}{1}".format(n, extra))

    materials = cmds.ls(materials=True) or []
    print("\n  [Materials] ({0})".format(len(materials)))
    for mat in materials:
        print("    {0}  ({1})".format(mat, cmds.nodeType(mat)))

    refs = cmds.file(query=True, reference=True) or []
    print("\n  [References] ({0})".format(len(refs)))
    for ref in refs:
        ns = cmds.file(ref, query=True, namespace=True)
        print("    {0}: {1}".format(ns, ref))

    print("\n" + "=" * 70)
    print("  END OF REPORT")
    print("=" * 70 + "\n")


# ─── Step 2: Find Mesh ──────────────────────────────────────────────

def find_mesh(name):
    """
    Locate a mesh by name (exact first, then partial).
    Returns (transform, shape) or (None, None).
    """
    if cmds.objExists(name):
        ntype = cmds.nodeType(name)
        if ntype == "transform":
            shapes = cmds.listRelatives(
                name, shapes=True, type="mesh", noIntermediate=True
            ) or []
            if shapes:
                print("[find_mesh] Exact match: transform='{0}', shape='{1}'".format(
                    name, shapes[0]))
                return name, shapes[0]
        elif ntype == "mesh":
            parent = cmds.listRelatives(name, parent=True, fullPath=False)
            t = parent[0] if parent else name
            print("[find_mesh] Exact match (shape): transform='{0}', shape='{1}'".format(
                t, name))
            return t, name

    for candidate in (cmds.ls("*{0}*".format(name), type="transform") or []):
        shapes = cmds.listRelatives(
            candidate, shapes=True, type="mesh", noIntermediate=True
        ) or []
        if shapes:
            print("[find_mesh] Partial match: transform='{0}', shape='{1}'".format(
                candidate, shapes[0]))
            return candidate, shapes[0]

    for candidate in (cmds.ls("*{0}*".format(name), type="mesh") or []):
        parent = cmds.listRelatives(candidate, parent=True, fullPath=False)
        t = parent[0] if parent else candidate
        print("[find_mesh] Partial match (shape): transform='{0}', shape='{1}'".format(
            t, candidate))
        return t, candidate

    return None, None


# ─── Step 3: Split Mesh by Skin Weights ─────────────────────────────

def _get_skin_cluster(node):
    """Return the skinCluster name on *node*, or None."""
    for h in (cmds.listHistory(node, pruneDagObjects=True) or []):
        if cmds.nodeType(h) == "skinCluster":
            return h
    return None


def _get_vertex_dominant_influences(mesh_shape, skin_name):
    """
    Use API 2.0 to batch-query all vertex weights.
    Returns (influence_names, vert_dominant)
      vert_dominant: dict  vertex_index -> dominant influence name
    """
    sel = om2.MSelectionList()
    sel.add(skin_name)
    skin_fn = oma2.MFnSkinCluster(sel.getDependNode(0))

    sel2 = om2.MSelectionList()
    sel2.add(mesh_shape)
    mesh_dag = sel2.getDagPath(0)

    inf_dags = skin_fn.influenceObjects()
    inf_names = [d.partialPathName() for d in inf_dags]
    num_inf = len(inf_names)

    num_verts = om2.MFnMesh(mesh_dag).numVertices

    comp_fn = om2.MFnSingleIndexedComponent()
    vert_comp = comp_fn.create(om2.MFn.kMeshVertComponent)
    comp_fn.addElements(list(range(num_verts)))

    weights, _ = skin_fn.getWeights(mesh_dag, vert_comp)

    vert_dominant = {}
    for vi in range(num_verts):
        offset = vi * num_inf
        best_idx = 0
        best_val = weights[offset]
        for ii in range(1, num_inf):
            w = weights[offset + ii]
            if w > best_val:
                best_val = w
                best_idx = ii
        vert_dominant[vi] = inf_names[best_idx]

    return inf_names, vert_dominant


def _get_face_vertices_fast(mesh_shape):
    """
    Use API 2.0 MItMeshPolygon for fast face -> vertex mapping.
    Returns dict  face_index -> [vertex_indices]
    """
    sel = om2.MSelectionList()
    sel.add(mesh_shape)
    mesh_dag = sel.getDagPath(0)

    result = {}
    it = om2.MItMeshPolygon(mesh_dag)
    while not it.isDone():
        result[it.index()] = list(it.getVertices())
        it.next()
    return result


def _get_bind_pose_positions(transform):
    """
    Read vertex positions from the intermediate (orig) shape node,
    which stores the pre-deformation bind pose (T-pose).
    Returns MPointArray of bind-pose positions.
    """
    all_shapes = cmds.listRelatives(
        transform, shapes=True, type="mesh", fullPath=True
    ) or []

    orig_shape = None
    for s in all_shapes:
        if cmds.getAttr("{0}.intermediateObject".format(s)):
            orig_shape = s
            break

    if not orig_shape:
        cmds.warning("No intermediate (orig) shape found on '{0}'. "
                      "Will use deformed positions.".format(transform))
        visible = cmds.listRelatives(
            transform, shapes=True, type="mesh",
            noIntermediate=True, fullPath=True
        ) or []
        orig_shape = visible[0] if visible else None

    if not orig_shape:
        return None

    print("[bind_pose] Reading positions from '{0}'".format(orig_shape))

    sel = om2.MSelectionList()
    sel.add(orig_shape)
    mesh_dag = sel.getDagPath(0)
    mesh_fn = om2.MFnMesh(mesh_dag)
    return mesh_fn.getPoints(om2.MSpace.kObject)


def _set_mesh_positions(transform, points):
    """
    Overwrite all vertex positions on a mesh with the given MPointArray.
    Must be called when vertex count matches len(points).
    """
    shapes = cmds.listRelatives(
        transform, shapes=True, type="mesh",
        noIntermediate=True, fullPath=True
    ) or []
    if not shapes:
        return

    sel = om2.MSelectionList()
    sel.add(shapes[0])
    mesh_dag = sel.getDagPath(0)
    mesh_fn = om2.MFnMesh(mesh_dag)
    mesh_fn.setPoints(points, om2.MSpace.kObject)
    mesh_fn.updateSurface()


def _clean_duplicate(dup):
    """
    Fully clean a duplicated skinned mesh so it becomes a normal,
    freely editable static mesh: unlock transforms, remove intermediate
    shapes, delete construction history, break leftover connections.
    """
    for attr in ["tx", "ty", "tz", "rx", "ry", "rz", "sx", "sy", "sz"]:
        full = "{0}.{1}".format(dup, attr)
        cmds.setAttr(full, lock=False)
        conns = cmds.listConnections(full, source=True, destination=False,
                                      plugs=True) or []
        for c in conns:
            try:
                cmds.disconnectAttr(c, full)
            except Exception:
                pass

    cmds.setAttr("{0}.overrideEnabled".format(dup), 0)

    all_shapes = cmds.listRelatives(
        dup, shapes=True, type="mesh", fullPath=True
    ) or []
    for s in all_shapes:
        if cmds.getAttr("{0}.intermediateObject".format(s)):
            cmds.delete(s)

    cmds.delete(dup, constructionHistory=True)


def _compress_indices(indices):
    """
    Compress a sorted list of integers into contiguous ranges.
    [0,1,2,5,6,10] -> [(0,2), (5,6), (10,10)]
    """
    if not indices:
        return []
    s = sorted(indices)
    ranges = []
    start = end = s[0]
    for i in s[1:]:
        if i == end + 1:
            end = i
        else:
            ranges.append((start, end))
            start = end = i
    ranges.append((start, end))
    return ranges


def split_mesh_by_weights(transform, shape):
    """
    Duplicate the mesh once per dominant influence, keeping only the
    faces that belong to each influence. Returns list of new transform names.
    """
    skin = _get_skin_cluster(shape)
    if not skin:
        cmds.warning("No skinCluster found on '{0}'. Cannot split.".format(transform))
        return []

    print("\n[split] skinCluster = '{0}'".format(skin))

    influences = cmds.skinCluster(skin, q=True, influence=True) or []
    print("[split] {0} influences: {1}".format(len(influences), influences))

    num_verts = cmds.polyEvaluate(transform, vertex=True)
    num_faces = cmds.polyEvaluate(transform, face=True)
    print("[split] {0} vertices, {1} faces".format(num_verts, num_faces))

    print("[split] Querying weights via API 2.0 ...")
    inf_names, vert_dominant = _get_vertex_dominant_influences(shape, skin)

    print("[split] Building face -> vertex map via API 2.0 ...")
    face_verts = _get_face_vertices_fast(shape)

    print("[split] Assigning faces to influences ...")
    face_groups = defaultdict(list)
    for fi in range(num_faces):
        verts = face_verts.get(fi, [])
        vote = defaultdict(int)
        for vi in verts:
            vote[vert_dominant[vi]] += 1
        winner = max(vote, key=vote.get)
        face_groups[winner].append(fi)

    print("\n[split] Face distribution ({0} groups):".format(len(face_groups)))
    for inf in sorted(face_groups, key=lambda k: -len(face_groups[k])):
        print("    {0}: {1} faces".format(inf, len(face_groups[inf])))

    print("[split] Reading bind-pose (T-pose) vertex positions ...")
    bind_points = _get_bind_pose_positions(transform)
    if bind_points:
        print("[split] Got {0} bind-pose vertex positions.".format(len(bind_points)))
    else:
        print("[split] WARNING: Could not read bind pose. "
              "Meshes will be in current (deformed) pose.")

    cmds.undoInfo(openChunk=True, chunkName="split_ms_by_weights")
    created = []
    try:
        for inf, keep_faces in face_groups.items():
            if not keep_faces:
                continue

            safe = inf.replace("|", "_").replace(":", "_")
            dup_name = "{0}__{1}".format(transform, safe)
            dup = cmds.duplicate(transform, name=dup_name, renameChildren=True)[0]

            dup_skin = _get_skin_cluster(dup)
            if dup_skin:
                cmds.skinCluster(dup_skin, edit=True, unbind=True)

            if bind_points:
                _set_mesh_positions(dup, bind_points)

            _clean_duplicate(dup)

            delete_set = set(range(num_faces)) - set(keep_faces)
            if delete_set:
                ranges = _compress_indices(delete_set)
                comps = []
                for a, b in ranges:
                    if a == b:
                        comps.append("{0}.f[{1}]".format(dup, a))
                    else:
                        comps.append("{0}.f[{1}:{2}]".format(dup, a, b))
                cmds.delete(comps)

            created.append(dup)
            remaining = cmds.polyEvaluate(dup, face=True)
            print("[split] Created '{0}' -> {1} faces (T-pose)".format(dup, remaining))
    finally:
        cmds.undoInfo(closeChunk=True)

    print("\n[split] Done! {0} meshes created from '{1}'.".format(
        len(created), transform))
    print("[split] You can Ctrl+Z to undo the split.\n")
    return created


# ─── Main ────────────────────────────────────────────────────────────

def main():
    # Step 1: list everything in the scene
    log_scene_assets()

    # Step 2: locate mesh "ms"
    print(">>> Step 2: Searching for mesh 'ms' ...")
    transform, shape = find_mesh("ms")
    if not transform:
        cmds.warning("Mesh 'ms' not found in scene! Aborting.")
        return

    # Step 3: split by dominant skin weight
    print(">>> Step 3: Splitting by skin weights ...")
    split_mesh_by_weights(transform, shape)


main()

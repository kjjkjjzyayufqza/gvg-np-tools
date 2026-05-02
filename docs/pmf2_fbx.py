#!/usr/bin/env python3
# ⚠️ DEPRECATED — 请使用 rust_converter/ 中的 Rust 版本
# 此文件仅保留作为参考，不再维护。
# Usage: cd rust_converter && cargo build --release
#        ./target/release/gvg_converter pipeline Z_DATA.BIN inventory.json
from __future__ import annotations

import json
import math
import struct
import re
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

from gvg_converter import (
    parse_pmf2_sections, compute_world_matrices, BoneSection,
    scan_ge_display_list, extract_draw_calls, decode_vertex,
    VtypeInfo, ParsedVertex, GE_CMD_ORIGIN,
    PRIM_TRIANGLE_STRIP, PRIM_TRIANGLE_FAN, PRIM_TRIANGLES,
    strip_to_triangles, fan_to_triangles,
    ru32, ru16, ri16, ru8, rf32, cstr,
    extract_pzz_streams, classify_stream, find_pzz_key, xor_dec,
)


@dataclass
class BoneMeshData:
    bone_index: int
    bone_name: str
    vertices: List[ParsedVertex]
    faces: List[Tuple[int, int, int]]
    local_vertices: List[ParsedVertex]
    has_uv: bool = False
    has_normals: bool = False
    draw_call_vtypes: List[int] = field(default_factory=list)


def extract_per_bone_meshes(
    pmf2_data: bytes,
    swap_yz: bool = True,
) -> Tuple[List[BoneMeshData], List[BoneSection], Tuple[float, float, float], Dict[int, List[float]]]:
    sections, bbox = parse_pmf2_sections(pmf2_data)
    if not sections:
        return [], [], (1.0, 1.0, 1.0), {}

    world_mats = compute_world_matrices(sections)
    sx = bbox[0] / 32768.0
    sy = bbox[1] / 32768.0
    sz_scale = bbox[2] / 32768.0

    bone_meshes: List[BoneMeshData] = []

    for sec in sections:
        if not sec.has_mesh or sec.origin_offset is None:
            continue

        origin = sec.origin_offset
        cmds = scan_ge_display_list(pmf2_data, origin, min(origin + 0x800, len(pmf2_data)))
        origin_off = origin
        for c in cmds:
            if c["cmd"] == GE_CMD_ORIGIN:
                origin_off = c["offset"]
                break

        calls = extract_draw_calls(cmds, origin_file_offset=origin_off)
        if not calls:
            continue

        wm = world_mats[sec.index]
        part_verts: List[ParsedVertex] = []
        local_verts: List[ParsedVertex] = []
        part_faces: List[Tuple[int, int, int]] = []
        has_uv = False
        has_nrm = False
        vtypes_used = []

        for dc in calls:
            vt = dc.vtype
            vs = vt.vertex_size()
            if vs <= 0 or not vt.pos_fmt:
                continue

            vaddr = dc.vaddr
            count = dc.vertex_count

            if vaddr + vs * count > len(pmf2_data):
                continue

            if vt.idx_fmt and dc.iaddr > 0:
                idx_size = {0: 0, 1: 1, 2: 2, 3: 4}[vt.idx_fmt]
                indices = []
                for ii in range(count):
                    ioff = dc.iaddr + ii * idx_size
                    if ioff + idx_size > len(pmf2_data):
                        break
                    if idx_size == 1:
                        indices.append(ru8(pmf2_data, ioff))
                    elif idx_size == 2:
                        indices.append(ru16(pmf2_data, ioff))
                    else:
                        indices.append(ru32(pmf2_data, ioff))

                if len(indices) < 3:
                    continue
                max_idx = max(indices)
                if vaddr + (max_idx + 1) * vs > len(pmf2_data):
                    continue

                vtx_cache: Dict[int, ParsedVertex] = {}
                raw_verts = []
                valid = True
                for idx_val in indices:
                    if idx_val not in vtx_cache:
                        pv = decode_vertex(pmf2_data, vaddr + idx_val * vs, vt)
                        if pv is None:
                            valid = False
                            break
                        pv.x *= sx
                        pv.y *= sy
                        pv.z *= sz_scale
                        vtx_cache[idx_val] = pv
                    raw_verts.append(vtx_cache[idx_val])

                if not valid or len(raw_verts) < 3:
                    continue
            else:
                raw_verts = []
                valid = True
                for vi in range(count):
                    pv = decode_vertex(pmf2_data, vaddr + vi * vs, vt)
                    if pv is None:
                        valid = False
                        break
                    pv.x *= sx
                    pv.y *= sy
                    pv.z *= sz_scale
                    raw_verts.append(pv)

                if not valid or len(raw_verts) < 3:
                    continue

            if vt.tc_fmt:
                has_uv = True
            if vt.nrm_fmt:
                has_nrm = True
            vtypes_used.append(vt.raw)

            local_copies = []
            transformed = []
            for pv in raw_verts:
                lv = ParsedVertex()
                lv.x, lv.y, lv.z = pv.x, pv.y, pv.z
                lv.u, lv.v = pv.u, pv.v
                lv.nx, lv.ny, lv.nz = pv.nx, pv.ny, pv.nz
                local_copies.append(lv)

                tv = ParsedVertex()
                tv.x, tv.y, tv.z = _transform_pt(wm, pv.x, pv.y, pv.z)
                tv.u, tv.v = pv.u, pv.v
                if has_nrm:
                    tv.nx, tv.ny, tv.nz = _transform_dir(wm, pv.nx, pv.ny, pv.nz)
                if swap_yz:
                    tv.x, tv.y, tv.z = tv.x, tv.z, -tv.y
                    tv.nx, tv.ny, tv.nz = tv.nx, tv.nz, -tv.ny
                transformed.append(tv)

            base_idx = len(part_verts)
            part_verts.extend(transformed)
            local_verts.extend(local_copies)

            if dc.prim_type == PRIM_TRIANGLE_STRIP:
                local_faces = strip_to_triangles(transformed)
            elif dc.prim_type == PRIM_TRIANGLE_FAN:
                local_faces = fan_to_triangles(transformed)
            elif dc.prim_type == PRIM_TRIANGLES:
                local_faces = [(fi, fi + 1, fi + 2) for fi in range(0, len(transformed) - 2, 3)]
            else:
                continue

            for a, b, c in local_faces:
                part_faces.append((base_idx + a, base_idx + b, base_idx + c))

        if part_verts and part_faces:
            bone_meshes.append(BoneMeshData(
                bone_index=sec.index,
                bone_name=sec.name,
                vertices=part_verts,
                faces=part_faces,
                local_vertices=local_verts,
                has_uv=has_uv,
                has_normals=has_nrm,
                draw_call_vtypes=vtypes_used,
            ))

    return bone_meshes, sections, bbox, world_mats


def _transform_pt(m, x, y, z):
    return (
        x * m[0] + y * m[4] + z * m[8] + m[12],
        x * m[1] + y * m[5] + z * m[9] + m[13],
        x * m[2] + y * m[6] + z * m[10] + m[14],
    )


def _transform_dir(m, x, y, z):
    return (
        x * m[0] + y * m[4] + z * m[8],
        x * m[1] + y * m[5] + z * m[9],
        x * m[2] + y * m[6] + z * m[10],
    )


def _mat4_to_fbx_str(m: List[float]) -> str:
    return ",".join(f"{v:.10f}" for v in m)


def _identity_mat4() -> List[float]:
    return [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]


class FbxWriter:
    def __init__(self):
        self.lines: List[str] = []
        self._id_counter = 100000

    def next_id(self) -> int:
        self._id_counter += 1
        return self._id_counter

    def _w(self, line: str):
        self.lines.append(line)

    def get_text(self) -> str:
        return "\n".join(self.lines) + "\n"


def _mat4_to_fbx_str(m: List[float]) -> str:
    return ",".join(f"{v:.10f}" for v in m)


def _fbx_world_matrix(wm: List[float]) -> List[float]:
    S = [1.0, 0.0, 0.0, 0.0,
         0.0, 0.0, -1.0, 0.0,
         0.0, 1.0, 0.0, 0.0,
         0.0, 0.0, 0.0, 1.0]
    S_inv = [1.0, 0.0, 0.0, 0.0,
             0.0, 0.0, 1.0, 0.0,
             0.0, -1.0, 0.0, 0.0,
             0.0, 0.0, 0.0, 1.0]
    return _mat4_mul(_mat4_mul(S_inv, wm), S)


def _mat4_mul(a: List[float], b: List[float]) -> List[float]:
    r = [0.0] * 16
    for i in range(4):
        for j in range(4):
            r[i * 4 + j] = sum(a[i * 4 + k] * b[k * 4 + j] for k in range(4))
    return r


def export_pmf2_to_fbx(
    pmf2_data: bytes,
    fbx_path: Path,
    meta_path: Path,
    model_name: str = "model",
    texture_file: Optional[str] = None,
) -> Dict[str, Any]:
    bone_meshes, sections, bbox, world_mats = extract_per_bone_meshes(pmf2_data, swap_yz=True)

    if not bone_meshes:
        return {"error": "no_mesh_data"}

    total_verts = []
    total_faces = []
    total_uvs = []
    total_normals = []
    bone_vertex_ranges = {}
    vert_offset = 0
    has_uv = any(bm.has_uv for bm in bone_meshes)
    has_nrm = any(bm.has_normals for bm in bone_meshes)

    for bm in bone_meshes:
        start = vert_offset
        for pv in bm.vertices:
            total_verts.extend([pv.x, pv.y, pv.z])
            if has_uv:
                total_uvs.extend([pv.u, 1.0 - pv.v])
            if has_nrm:
                total_normals.extend([pv.nx, pv.ny, pv.nz])
        for a, b, c in bm.faces:
            total_faces.extend([a + vert_offset, b + vert_offset, -(c + vert_offset) - 1])
        bone_vertex_ranges[bm.bone_index] = (start, start + len(bm.vertices))
        vert_offset += len(bm.vertices)

    w = FbxWriter()
    w._w("; FBX 7.4.0 project file")
    w._w("; Created by pmf2_fbx.py")
    w._w("")
    w._w("FBXHeaderExtension:  {")
    w._w("\tFBXHeaderVersion: 1003")
    w._w("\tFBXVersion: 7400")
    w._w('\tCreator: "pmf2_fbx.py"')
    w._w("}")
    w._w("")
    w._w("GlobalSettings:  {")
    w._w("\tVersion: 1000")
    w._w("\tProperties70:  {")
    w._w('\t\tP: "UpAxis", "int", "Integer", "",1')
    w._w('\t\tP: "UpAxisSign", "int", "Integer", "",1')
    w._w('\t\tP: "FrontAxis", "int", "Integer", "",2')
    w._w('\t\tP: "FrontAxisSign", "int", "Integer", "",1')
    w._w('\t\tP: "CoordAxis", "int", "Integer", "",0')
    w._w('\t\tP: "CoordAxisSign", "int", "Integer", "",1')
    w._w('\t\tP: "UnitScaleFactor", "double", "Number", "",1')
    w._w("\t}")
    w._w("}")
    w._w("")
    w._w("Documents:  {")
    w._w("\tCount: 1")
    w._w('\tDocument: 1000000, "", "Scene" {')
    w._w("\t\tProperties70:  {")
    w._w('\t\t\tP: "SourceObject", "object", "", ""')
    w._w('\t\t\tP: "ActiveAnimStackName", "KString", "", "", ""')
    w._w("\t\t}")
    w._w("\t\tRootNode: 0")
    w._w("\t}")
    w._w("}")
    w._w("")
    w._w("References:  {")
    w._w("}")
    w._w("")

    geom_id = w.next_id()
    mesh_model_id = w.next_id()
    mat_id = w.next_id()

    num_models = 1
    num_geometry = 1
    num_material = 1
    num_deformer = 0
    num_node_attr = 0

    w._w("Definitions:  {")
    w._w("\tVersion: 100")
    total_count = num_models + num_geometry + num_material
    w._w(f"\tCount: {total_count}")
    w._w('\tObjectType: "GlobalSettings" {')
    w._w("\t\tCount: 1")
    w._w("\t}")
    w._w('\tObjectType: "Model" {')
    w._w(f"\t\tCount: {num_models}")
    w._w("\t}")
    w._w('\tObjectType: "Geometry" {')
    w._w(f"\t\tCount: {num_geometry}")
    w._w("\t}")
    w._w('\tObjectType: "Material" {')
    w._w(f"\t\tCount: {num_material}")
    w._w("\t}")
    w._w("}")
    w._w("")

    w._w("Objects:  {")

    nf_raw = len(total_faces)
    w._w(f'\tGeometry: {geom_id}, "Geometry::{model_name}", "Mesh" {{')
    w._w(f"\t\tVertices: *{len(total_verts)} {{")
    w._w(f"\t\t\ta: {','.join(f'{v:.6f}' for v in total_verts)}")
    w._w("\t\t}")
    w._w(f"\t\tPolygonVertexIndex: *{nf_raw} {{")
    w._w(f"\t\t\ta: {','.join(str(f) for f in total_faces)}")
    w._w("\t\t}")

    if has_nrm and total_normals:
        w._w('\t\tLayerElementNormal: 0 {')
        w._w('\t\t\tVersion: 101')
        w._w('\t\t\tName: ""')
        w._w('\t\t\tMappingInformationType: "ByVertice"')
        w._w('\t\t\tReferenceInformationType: "Direct"')
        w._w(f'\t\t\tNormals: *{len(total_normals)} {{')
        w._w(f'\t\t\t\ta: {",".join(f"{n:.6f}" for n in total_normals)}')
        w._w('\t\t\t}')
        w._w('\t\t}')

    if has_uv and total_uvs:
        w._w('\t\tLayerElementUV: 0 {')
        w._w('\t\t\tVersion: 101')
        w._w('\t\t\tName: "UVMap"')
        w._w('\t\t\tMappingInformationType: "ByVertice"')
        w._w('\t\t\tReferenceInformationType: "Direct"')
        w._w(f'\t\t\tUV: *{len(total_uvs)} {{')
        w._w(f'\t\t\t\ta: {",".join(f"{u:.6f}" for u in total_uvs)}')
        w._w('\t\t\t}')
        w._w('\t\t}')

    w._w('\t\tLayer: 0 {')
    w._w('\t\t\tVersion: 100')
    if has_nrm:
        w._w('\t\t\tLayerElement:  {')
        w._w('\t\t\t\tType: "LayerElementNormal"')
        w._w('\t\t\t\tTypedIndex: 0')
        w._w('\t\t\t}')
    if has_uv:
        w._w('\t\t\tLayerElement:  {')
        w._w('\t\t\t\tType: "LayerElementUV"')
        w._w('\t\t\t\tTypedIndex: 0')
        w._w('\t\t\t}')
    w._w('\t\t}')
    w._w("\t}")

    w._w(f'\tModel: {mesh_model_id}, "Model::{model_name}", "Mesh" {{')
    w._w('\t\tVersion: 232')
    w._w('\t\tProperties70:  {')
    w._w('\t\t\tP: "Lcl Translation", "Lcl Translation", "", "A",0,0,0')
    w._w('\t\t\tP: "Lcl Rotation", "Lcl Rotation", "", "A",0,0,0')
    w._w('\t\t\tP: "Lcl Scaling", "Lcl Scaling", "", "A",1,1,1')
    w._w('\t\t}')
    w._w('\t}')

    tex_name = texture_file if texture_file else ""
    w._w(f'\tMaterial: {mat_id}, "Material::{model_name}_mat", "" {{')
    w._w('\t\tVersion: 102')
    w._w('\t\tShadingModel: "phong"')
    w._w('\t\tProperties70:  {')
    w._w('\t\t\tP: "DiffuseColor", "Color", "", "A",0.8,0.8,0.8')
    w._w('\t\t\tP: "AmbientColor", "Color", "", "A",0.2,0.2,0.2')
    w._w('\t\t}')
    w._w('\t}')

    w._w("}")
    w._w("")

    w._w("Connections:  {")
    w._w(f'\tC: "OO",{mesh_model_id},0')
    w._w(f'\tC: "OO",{geom_id},{mesh_model_id}')
    w._w(f'\tC: "OO",{mat_id},{mesh_model_id}')
    w._w("}")
    w._w("")

    fbx_path.parent.mkdir(parents=True, exist_ok=True)
    fbx_path.write_text(w.get_text(), encoding="utf-8")

    meta = {
        "model_name": model_name,
        "bbox": list(bbox),
        "section_count": len(sections),
        "sections": [],
        "bone_meshes": [],
        "vertex_format": "tc16_nrm16_pos16",
    }
    for sec in sections:
        meta["sections"].append({
            "index": sec.index,
            "name": sec.name,
            "offset": sec.offset,
            "size": sec.size,
            "parent": sec.parent,
            "category": sec.category,
            "has_mesh": sec.has_mesh,
            "local_matrix": sec.local_matrix,
        })
    for bm in bone_meshes:
        mesh_meta = {
            "bone_index": bm.bone_index,
            "bone_name": bm.bone_name,
            "vertex_count": len(bm.vertices),
            "face_count": len(bm.faces),
            "has_uv": bm.has_uv,
            "has_normals": bm.has_normals,
            "draw_call_vtypes": bm.draw_call_vtypes,
            "local_vertices": [[lv.x, lv.y, lv.z, lv.u, lv.v, lv.nx, lv.ny, lv.nz]
                                for lv in bm.local_vertices],
            "faces": bm.faces,
        }
        meta["bone_meshes"].append(mesh_meta)

    meta_path.write_text(json.dumps(meta, ensure_ascii=False, indent=2), encoding="utf-8")

    return {
        "fbx_file": str(fbx_path),
        "meta_file": str(meta_path),
        "total_vertices": vert_offset,
        "total_faces": len(total_faces) // 3,
        "bone_count": len(sections),
        "mesh_parts": len(bone_meshes),
    }


def _invert_mat4(m: List[float]) -> Optional[List[float]]:
    inv = [0.0] * 16

    inv[0] = m[5]*m[10]*m[15] - m[5]*m[11]*m[14] - m[9]*m[6]*m[15] + m[9]*m[7]*m[14] + m[13]*m[6]*m[11] - m[13]*m[7]*m[10]
    inv[4] = -m[4]*m[10]*m[15] + m[4]*m[11]*m[14] + m[8]*m[6]*m[15] - m[8]*m[7]*m[14] - m[12]*m[6]*m[11] + m[12]*m[7]*m[10]
    inv[8] = m[4]*m[9]*m[15] - m[4]*m[11]*m[13] - m[8]*m[5]*m[15] + m[8]*m[7]*m[13] + m[12]*m[5]*m[11] - m[12]*m[7]*m[9]
    inv[12] = -m[4]*m[9]*m[14] + m[4]*m[10]*m[13] + m[8]*m[5]*m[14] - m[8]*m[6]*m[13] - m[12]*m[5]*m[10] + m[12]*m[6]*m[9]

    inv[1] = -m[1]*m[10]*m[15] + m[1]*m[11]*m[14] + m[9]*m[2]*m[15] - m[9]*m[3]*m[14] - m[13]*m[2]*m[11] + m[13]*m[3]*m[10]
    inv[5] = m[0]*m[10]*m[15] - m[0]*m[11]*m[14] - m[8]*m[2]*m[15] + m[8]*m[3]*m[14] + m[12]*m[2]*m[11] - m[12]*m[3]*m[10]
    inv[9] = -m[0]*m[9]*m[15] + m[0]*m[11]*m[13] + m[8]*m[1]*m[15] - m[8]*m[3]*m[13] - m[12]*m[1]*m[11] + m[12]*m[3]*m[9]
    inv[13] = m[0]*m[9]*m[14] - m[0]*m[10]*m[13] - m[8]*m[1]*m[14] + m[8]*m[2]*m[13] + m[12]*m[1]*m[10] - m[12]*m[2]*m[9]

    inv[2] = m[1]*m[6]*m[15] - m[1]*m[7]*m[14] - m[5]*m[2]*m[15] + m[5]*m[3]*m[14] + m[13]*m[2]*m[7] - m[13]*m[3]*m[6]
    inv[6] = -m[0]*m[6]*m[15] + m[0]*m[7]*m[14] + m[4]*m[2]*m[15] - m[4]*m[3]*m[14] - m[12]*m[2]*m[7] + m[12]*m[3]*m[6]
    inv[10] = m[0]*m[5]*m[15] - m[0]*m[7]*m[13] - m[4]*m[1]*m[15] + m[4]*m[3]*m[13] + m[12]*m[1]*m[7] - m[12]*m[3]*m[5]
    inv[14] = -m[0]*m[5]*m[14] + m[0]*m[6]*m[13] + m[4]*m[1]*m[14] - m[4]*m[2]*m[13] - m[12]*m[1]*m[6] + m[12]*m[2]*m[5]

    inv[3] = -m[1]*m[6]*m[11] + m[1]*m[7]*m[10] + m[5]*m[2]*m[11] - m[5]*m[3]*m[10] - m[9]*m[2]*m[7] + m[9]*m[3]*m[6]
    inv[7] = m[0]*m[6]*m[11] - m[0]*m[7]*m[10] - m[4]*m[2]*m[11] + m[4]*m[3]*m[10] + m[8]*m[2]*m[7] - m[8]*m[3]*m[6]
    inv[11] = -m[0]*m[5]*m[11] + m[0]*m[7]*m[9] + m[4]*m[1]*m[11] - m[4]*m[3]*m[9] - m[8]*m[1]*m[7] + m[8]*m[3]*m[5]
    inv[15] = m[0]*m[5]*m[10] - m[0]*m[6]*m[9] - m[4]*m[1]*m[10] + m[4]*m[2]*m[9] + m[8]*m[1]*m[6] - m[8]*m[2]*m[5]

    det = m[0]*inv[0] + m[1]*inv[4] + m[2]*inv[8] + m[3]*inv[12]
    if abs(det) < 1e-12:
        return None

    det_inv = 1.0 / det
    return [x * det_inv for x in inv]


def import_fbx_to_pmf2(
    meta_path: Path,
) -> bytes:
    meta = json.loads(meta_path.read_text(encoding="utf-8"))
    bbox = tuple(meta["bbox"])
    sections_meta = meta["sections"]
    bone_meshes_meta = meta["bone_meshes"]

    num_sec = len(sections_meta)
    sx = bbox[0] / 32768.0
    sy = bbox[1] / 32768.0
    sz = bbox[2] / 32768.0

    section_data_list = []
    total_size_estimate = 0x20 + num_sec * 4

    for sm in sections_meta:
        sec_buf = bytearray(0x100)

        lm = sm["local_matrix"]
        for i in range(16):
            struct.pack_into("<f", sec_buf, i * 4, lm[i])

        name_bytes = sm["name"].encode("ascii")[:15]
        sec_buf[0x60:0x60 + len(name_bytes)] = name_bytes

        parent = sm["parent"]
        if parent < 0:
            struct.pack_into("<I", sec_buf, 0x7C, 0xFFFFFFFF)
        else:
            struct.pack_into("<I", sec_buf, 0x7C, parent)

        for i in range(0xC0, 0x100):
            sec_buf[i] = 0xFF

        mesh_meta = None
        for bm in bone_meshes_meta:
            if bm["bone_index"] == sm["index"]:
                mesh_meta = bm
                break

        if mesh_meta and mesh_meta["local_vertices"]:
            ge_data = _build_ge_commands(mesh_meta, bbox)
            sec_buf.extend(ge_data)

        while len(sec_buf) % 16 != 0:
            sec_buf.append(0)

        section_data_list.append(bytes(sec_buf))

    header_size = 0x20 + num_sec * 4
    while header_size % 16 != 0:
        header_size += 4

    offsets = []
    current_offset = header_size
    for sd in section_data_list:
        offsets.append(current_offset)
        current_offset += len(sd)

    total_size = current_offset
    pmf2 = bytearray(total_size)

    pmf2[0:4] = b"PMF2"
    struct.pack_into("<I", pmf2, 4, num_sec)
    struct.pack_into("<I", pmf2, 8, header_size)
    struct.pack_into("<f", pmf2, 0x10, bbox[0])
    struct.pack_into("<f", pmf2, 0x14, bbox[1])
    struct.pack_into("<f", pmf2, 0x18, bbox[2])

    for i, off in enumerate(offsets):
        struct.pack_into("<I", pmf2, 0x20 + i * 4, off)

    for i, sd in enumerate(section_data_list):
        pmf2[offsets[i]:offsets[i] + len(sd)] = sd

    return bytes(pmf2)


def _build_ge_commands(mesh_meta: Dict, bbox: Tuple[float, ...]) -> bytearray:
    local_verts_raw = mesh_meta["local_vertices"]
    faces = mesh_meta["faces"]
    has_uv = mesh_meta.get("has_uv", False)
    has_nrm = mesh_meta.get("has_normals", False)

    sx = bbox[0] / 32768.0
    sy = bbox[1] / 32768.0
    sz = bbox[2] / 32768.0

    verts_i16 = []
    for lv in local_verts_raw:
        x, y, z = lv[0], lv[1], lv[2]
        u, v = lv[3], lv[4]
        nx, ny, nz = lv[5], lv[6], lv[7]

        px = _clamp_i16(round(x / sx)) if sx > 0 else 0
        py = _clamp_i16(round(y / sy)) if sy > 0 else 0
        pz = _clamp_i16(round(z / sz)) if sz > 0 else 0

        tu = _clamp_i16(round(u * 32768.0))
        tv = _clamp_i16(round(v * 32768.0))

        nnx = _clamp_i16(round(nx * 32767.0))
        nny = _clamp_i16(round(ny * 32767.0))
        nnz = _clamp_i16(round(nz * 32767.0))

        verts_i16.append((tu, tv, nnx, nny, nnz, px, py, pz))

    seq_verts = []
    for a, b, c in faces:
        if a < len(verts_i16) and b < len(verts_i16) and c < len(verts_i16):
            seq_verts.append(verts_i16[a])
            seq_verts.append(verts_i16[b])
            seq_verts.append(verts_i16[c])

    if not seq_verts:
        return bytearray()

    vtype = 0
    stride = 0
    if has_uv:
        vtype |= 2
        stride += 4
    if has_nrm:
        vtype |= (2 << 5)
        stride += 6
    vtype |= (2 << 7)
    stride += 6

    while stride % 2 != 0:
        stride += 1

    MAX_VERTS_PER_PRIM = 65535
    chunks = []
    for i in range(0, len(seq_verts), MAX_VERTS_PER_PRIM):
        chunks.append(seq_verts[i:i + MAX_VERTS_PER_PRIM])

    num_cmds = 2 + len(chunks) * 3 + 1
    cmd_block_size = num_cmds * 4
    while cmd_block_size % 4 != 0:
        cmd_block_size += 4

    vert_buf = bytearray()
    for vtx in seq_verts:
        tu, tv, nnx, nny, nnz, px, py, pz = vtx
        if has_uv:
            vert_buf.extend(struct.pack("<hh", tu, tv))
        if has_nrm:
            vert_buf.extend(struct.pack("<hhh", nnx, nny, nnz))
        vert_buf.extend(struct.pack("<hhh", px, py, pz))

    while len(vert_buf) % 4 != 0:
        vert_buf.append(0)

    vert_buf_start = cmd_block_size

    ge_cmds = bytearray()
    _append_ge_cmd(ge_cmds, 0x14, 0x000000)
    _append_ge_cmd(ge_cmds, 0x10, 0x000000)

    vert_offset = 0
    for chunk in chunks:
        chunk_vaddr = vert_buf_start + vert_offset * stride
        _append_ge_cmd(ge_cmds, 0x01, chunk_vaddr)
        _append_ge_cmd(ge_cmds, 0x12, vtype)
        prim_param = (PRIM_TRIANGLES << 16) | len(chunk)
        _append_ge_cmd(ge_cmds, 0x04, prim_param)
        vert_offset += len(chunk)

    _append_ge_cmd(ge_cmds, 0x0B, 0x000000)

    while len(ge_cmds) < cmd_block_size:
        ge_cmds.extend(b'\x00\x00\x00\x00')

    result = bytearray()
    result.extend(ge_cmds)
    result.extend(vert_buf)

    return result


def _clamp_i16(val: int) -> int:
    return max(-32768, min(32767, val))


def _append_ge_cmd(buf: bytearray, cmd: int, param: int):
    word = ((cmd & 0xFF) << 24) | (param & 0xFFFFFF)
    buf.extend(struct.pack("<I", word))


def export_pzz_to_fbx(
    pzz_data: bytes,
    out_dir: Path,
    model_name: str = "model",
) -> Dict[str, Any]:
    from gim_converter import gim_to_png

    streams = extract_pzz_streams(pzz_data)
    if not streams:
        return {"error": "no_streams"}

    out_dir.mkdir(parents=True, exist_ok=True)

    all_stream_data = []
    for i, s in enumerate(streams):
        ct = classify_stream(s)
        stream_path = out_dir / f"stream{i:03d}.{'pmf2' if ct == 'pmf2' else ct if ct != 'unknown' else 'bin'}"
        stream_path.write_bytes(s)
        all_stream_data.append({"index": i, "type": ct, "size": len(s), "file": str(stream_path)})

    pmf2_streams = [(i, s) for i, s in enumerate(streams) if classify_stream(s) == "pmf2"]
    gim_streams = [(i, s) for i, s in enumerate(streams) if classify_stream(s) == "gim"]

    result = {
        "model_name": model_name,
        "stream_count": len(streams),
        "streams": all_stream_data,
        "exported_fbx": [],
        "exported_textures": [],
    }

    for gi, (gim_idx, gim_data) in enumerate(gim_streams):
        gim_path = out_dir / f"stream{gim_idx:03d}.gim"
        gim_path.write_bytes(gim_data)
        png_path = gim_path.with_suffix(".png")
        try:
            if gim_to_png(gim_data, png_path):
                result["exported_textures"].append({
                    "index": gim_idx, "gim": str(gim_path), "png": str(png_path)
                })
        except Exception:
            pass

    for pi, (pmf2_idx, pmf2_data) in enumerate(pmf2_streams):
        fbx_name = f"{model_name}_stream{pmf2_idx:03d}"
        fbx_path = out_dir / f"{fbx_name}.fbx"
        meta_path = out_dir / f"{fbx_name}.pmf2meta.json"

        info = export_pmf2_to_fbx(
            pmf2_data, fbx_path, meta_path,
            model_name=fbx_name,
            texture_file=result["exported_textures"][0]["png"] if result["exported_textures"] else None,
        )
        info["pmf2_stream_index"] = pmf2_idx
        result["exported_fbx"].append(info)

    streams_meta = out_dir / "streams_manifest.json"
    streams_meta.write_text(json.dumps(result, ensure_ascii=False, indent=2, default=str), encoding="utf-8")
    return result


def rebuild_pmf2_from_meta(meta_path: Path) -> bytes:
    return import_fbx_to_pmf2(meta_path)


if __name__ == "__main__":
    import argparse

    ap = argparse.ArgumentParser(description="PMF2 <-> FBX converter")
    sub = ap.add_subparsers(dest="cmd")

    p_export = sub.add_parser("export", help="Export PZZ to FBX")
    p_export.add_argument("pzz", help="Input PZZ file or raw bytes file")
    p_export.add_argument("--out", default=None, help="Output directory")
    p_export.add_argument("--name", default=None, help="Model name")

    p_import = sub.add_parser("import", help="Import FBX metadata back to PMF2")
    p_import.add_argument("meta", help="Input .pmf2meta.json file")
    p_import.add_argument("--out", default=None, help="Output PMF2 file")

    args = ap.parse_args()

    if args.cmd == "export":
        pzz_path = Path(args.pzz)
        name = args.name or pzz_path.stem
        out_dir = Path(args.out) if args.out else pzz_path.parent / f"{name}_fbx"
        pzz_data = pzz_path.read_bytes()
        result = export_pzz_to_fbx(pzz_data, out_dir, model_name=name)
        print(json.dumps(result, indent=2, default=str))

    elif args.cmd == "import":
        meta_path = Path(args.meta)
        pmf2_data = rebuild_pmf2_from_meta(meta_path)
        out_path = Path(args.out) if args.out else meta_path.with_suffix(".pmf2")
        out_path.write_bytes(pmf2_data)
        print(f"Rebuilt PMF2: {out_path} ({len(pmf2_data)} bytes)")

#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import math
import struct
import zlib
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

try:
    from gim_converter import gim_to_png
except ImportError:
    gim_to_png = None

GE_CMD_VADDR = 0x01
GE_CMD_IADDR = 0x02
GE_CMD_PRIM = 0x04
GE_CMD_BASE = 0x10
GE_CMD_VERTEXTYPE = 0x12
GE_CMD_ORIGIN = 0x14
GE_CMD_BONEMATRIXNUMBER = 0x2A
GE_CMD_BONEMATRIXDATA = 0x2B
GE_CMD_TEXADDR0 = 0xA0
GE_CMD_TEXSIZE0 = 0xB8
GE_CMD_TEXFMT = 0xC7
GE_CMD_CLUTADDR = 0xDD
GE_CMD_CLUTFORMAT = 0xDE
GE_CMD_END = 0x0C
GE_CMD_FINISH = 0x0F
GE_CMD_SIGNAL = 0x0E
GE_CMD_RET = 0x0B

PRIM_POINTS = 0
PRIM_LINES = 1
PRIM_LINE_STRIP = 2
PRIM_TRIANGLES = 3
PRIM_TRIANGLE_STRIP = 4
PRIM_TRIANGLE_FAN = 5
PRIM_RECTANGLES = 6

PRIM_NAMES = {0: "points", 1: "lines", 2: "line_strip", 3: "triangles",
               4: "tri_strip", 5: "tri_fan", 6: "rects"}


def ru32(d: bytes, o: int) -> int:
    return struct.unpack_from("<I", d, o)[0] if o + 4 <= len(d) else 0

def ru16(d: bytes, o: int) -> int:
    return struct.unpack_from("<H", d, o)[0] if o + 2 <= len(d) else 0

def ri16(d: bytes, o: int) -> int:
    return struct.unpack_from("<h", d, o)[0] if o + 2 <= len(d) else 0

def ri8(d: bytes, o: int) -> int:
    return struct.unpack_from("<b", d, o)[0] if o + 1 <= len(d) else 0

def ru8(d: bytes, o: int) -> int:
    return d[o] if o < len(d) else 0

def rf32(d: bytes, o: int) -> float:
    return struct.unpack_from("<f", d, o)[0] if o + 4 <= len(d) else 0.0

def cstr(d: bytes, o: int, mx: int = 32) -> str:
    end = min(o + mx, len(d))
    raw = d[o:end]
    p = raw.find(b"\x00")
    if p >= 0:
        raw = raw[:p]
    return raw.decode("ascii", errors="replace")


@dataclass
class VtypeInfo:
    raw: int = 0
    tc_fmt: int = 0
    col_fmt: int = 0
    nrm_fmt: int = 0
    pos_fmt: int = 0
    wt_fmt: int = 0
    idx_fmt: int = 0
    wt_count: int = 0
    morph_count: int = 0
    through: bool = False

    @staticmethod
    def decode(vtype: int) -> VtypeInfo:
        return VtypeInfo(
            raw=vtype,
            tc_fmt=vtype & 3,
            col_fmt=(vtype >> 2) & 7,
            nrm_fmt=(vtype >> 5) & 3,
            pos_fmt=(vtype >> 7) & 3,
            wt_fmt=(vtype >> 9) & 3,
            idx_fmt=(vtype >> 11) & 3,
            wt_count=((vtype >> 14) & 7) + 1 if (vtype >> 9) & 3 else 0,
            morph_count=(vtype >> 18) & 7,
            through=bool(vtype & (1 << 23)),
        )

    def vertex_size(self) -> int:
        sz = 0
        component_sizes = {0: 0, 1: 1, 2: 2, 3: 4}

        if self.wt_fmt:
            wt_bytes = component_sizes[self.wt_fmt] * self.wt_count
            sz = _align(sz, component_sizes[self.wt_fmt]) + wt_bytes

        if self.tc_fmt:
            tc_align = component_sizes[self.tc_fmt]
            sz = _align(sz, tc_align) + tc_align * 2

        if self.col_fmt:
            col_bytes = {0: 0, 4: 2, 5: 2, 6: 2, 7: 4}.get(self.col_fmt, 0)
            if col_bytes:
                sz = _align(sz, col_bytes) + col_bytes

        if self.nrm_fmt:
            nrm_align = component_sizes[self.nrm_fmt]
            sz = _align(sz, nrm_align) + nrm_align * 3

        if self.pos_fmt:
            pos_align = component_sizes[self.pos_fmt]
            sz = _align(sz, pos_align) + pos_align * 3

        sz = _align(sz, max(component_sizes.get(self.pos_fmt, 1), 1))
        return sz

    def describe(self) -> str:
        parts = []
        fmt_names = {0: "none", 1: "8bit", 2: "16bit", 3: "float"}
        if self.wt_fmt:
            parts.append(f"wt={fmt_names[self.wt_fmt]}x{self.wt_count}")
        if self.tc_fmt:
            parts.append(f"tc={fmt_names[self.tc_fmt]}")
        if self.col_fmt:
            col_names = {0: "none", 4: "565", 5: "5551", 6: "4444", 7: "8888"}
            parts.append(f"col={col_names.get(self.col_fmt, str(self.col_fmt))}")
        if self.nrm_fmt:
            parts.append(f"nrm={fmt_names[self.nrm_fmt]}")
        if self.pos_fmt:
            parts.append(f"pos={fmt_names[self.pos_fmt]}")
        if self.through:
            parts.append("THROUGH")
        return " ".join(parts) if parts else "empty"


def _align(v: int, a: int) -> int:
    if a <= 1:
        return v
    return (v + a - 1) & ~(a - 1)


@dataclass
class DrawCall:
    prim_type: int = 0
    vertex_count: int = 0
    vtype: VtypeInfo = field(default_factory=VtypeInfo)
    vaddr: int = 0
    iaddr: int = 0
    base_addr: int = 0


@dataclass
class ParsedVertex:
    x: float = 0.0
    y: float = 0.0
    z: float = 0.0
    u: float = 0.0
    v: float = 0.0
    nx: float = 0.0
    ny: float = 0.0
    nz: float = 0.0
    r: float = 1.0
    g: float = 1.0
    b: float = 1.0
    a: float = 1.0


def decode_vertex(data: bytes, offset: int, vt: VtypeInfo) -> Optional[ParsedVertex]:
    if offset + vt.vertex_size() > len(data):
        return None

    v = ParsedVertex()
    o = offset
    component_sizes = {0: 0, 1: 1, 2: 2, 3: 4}

    if vt.wt_fmt:
        wt_bytes = component_sizes[vt.wt_fmt] * vt.wt_count
        o = _align(o, component_sizes[vt.wt_fmt]) + wt_bytes

    if vt.tc_fmt:
        tc_sz = component_sizes[vt.tc_fmt]
        o = _align(o, tc_sz)
        if vt.tc_fmt == 1:
            v.u = ru8(data, o) / 128.0
            v.v = ru8(data, o + 1) / 128.0
        elif vt.tc_fmt == 2:
            v.u = ri16(data, o) / 32768.0
            v.v = ri16(data, o + 2) / 32768.0
        elif vt.tc_fmt == 3:
            v.u = rf32(data, o)
            v.v = rf32(data, o + 4)
        o += tc_sz * 2

    if vt.col_fmt:
        col_bytes = {0: 0, 4: 2, 5: 2, 6: 2, 7: 4}.get(vt.col_fmt, 0)
        if col_bytes:
            o = _align(o, col_bytes)
            if vt.col_fmt == 7:
                rgba = ru32(data, o)
                v.r = (rgba & 0xFF) / 255.0
                v.g = ((rgba >> 8) & 0xFF) / 255.0
                v.b = ((rgba >> 16) & 0xFF) / 255.0
                v.a = ((rgba >> 24) & 0xFF) / 255.0
            elif vt.col_fmt == 5:
                c16 = ru16(data, o)
                v.r = (c16 & 0x1F) / 31.0
                v.g = ((c16 >> 5) & 0x1F) / 31.0
                v.b = ((c16 >> 10) & 0x1F) / 31.0
                v.a = float((c16 >> 15) & 1)
            elif vt.col_fmt == 4:
                c16 = ru16(data, o)
                v.r = (c16 & 0x1F) / 31.0
                v.g = ((c16 >> 5) & 0x3F) / 63.0
                v.b = ((c16 >> 11) & 0x1F) / 31.0
            elif vt.col_fmt == 6:
                c16 = ru16(data, o)
                v.r = (c16 & 0xF) / 15.0
                v.g = ((c16 >> 4) & 0xF) / 15.0
                v.b = ((c16 >> 8) & 0xF) / 15.0
                v.a = ((c16 >> 12) & 0xF) / 15.0
            o += col_bytes

    if vt.nrm_fmt:
        nrm_sz = component_sizes[vt.nrm_fmt]
        o = _align(o, nrm_sz)
        if vt.nrm_fmt == 1:
            v.nx = ri8(data, o) / 127.0
            v.ny = ri8(data, o + 1) / 127.0
            v.nz = ri8(data, o + 2) / 127.0
        elif vt.nrm_fmt == 2:
            v.nx = ri16(data, o) / 32767.0
            v.ny = ri16(data, o + 2) / 32767.0
            v.nz = ri16(data, o + 4) / 32767.0
        elif vt.nrm_fmt == 3:
            v.nx = rf32(data, o)
            v.ny = rf32(data, o + 4)
            v.nz = rf32(data, o + 8)
        o += nrm_sz * 3

    if vt.pos_fmt:
        pos_sz = component_sizes[vt.pos_fmt]
        o = _align(o, pos_sz)
        if vt.pos_fmt == 1:
            v.x = ri8(data, o) / 127.0
            v.y = ri8(data, o + 1) / 127.0
            v.z = ri8(data, o + 2) / 127.0
        elif vt.pos_fmt == 2:
            v.x = ri16(data, o)
            v.y = ri16(data, o + 2)
            v.z = ri16(data, o + 4)
        elif vt.pos_fmt == 3:
            v.x = rf32(data, o)
            v.y = rf32(data, o + 4)
            v.z = rf32(data, o + 8)

    return v


def scan_ge_display_list(data: bytes, start: int, end: int) -> List[Dict]:
    cmds = []
    off = start
    while off + 4 <= end:
        word = ru32(data, off)
        cmd = (word >> 24) & 0xFF
        param = word & 0xFFFFFF
        cmds.append({"offset": off, "cmd": cmd, "param": param, "raw": word})
        off += 4
        if cmd in (GE_CMD_END, GE_CMD_FINISH, GE_CMD_RET):
            break
    return cmds


def extract_draw_calls(ge_cmds: List[Dict], origin_file_offset: int = 0) -> List[DrawCall]:
    calls = []
    cur_vtype = VtypeInfo()
    cur_vaddr = 0
    cur_iaddr = 0
    cur_base = 0
    idx_advance = 0

    for c in ge_cmds:
        cmd = c["cmd"]
        param = c["param"]

        if cmd == GE_CMD_BASE:
            cur_base = (param & 0x0F0000) << 8
        elif cmd == GE_CMD_VERTEXTYPE:
            cur_vtype = VtypeInfo.decode(param)
            idx_advance = 0
        elif cmd == GE_CMD_VADDR:
            cur_vaddr = param
        elif cmd == GE_CMD_IADDR:
            cur_iaddr = param
            idx_advance = 0
        elif cmd == GE_CMD_PRIM:
            prim_type = (param >> 16) & 7
            vert_count = param & 0xFFFF
            if vert_count > 0 and cur_vtype.pos_fmt:
                file_vaddr = origin_file_offset + cur_vaddr
                file_iaddr = origin_file_offset + cur_iaddr + idx_advance
                calls.append(DrawCall(
                    prim_type=prim_type,
                    vertex_count=vert_count,
                    vtype=VtypeInfo.decode(cur_vtype.raw),
                    vaddr=file_vaddr,
                    iaddr=file_iaddr,
                    base_addr=cur_base,
                ))
                if cur_vtype.idx_fmt:
                    idx_size = {0: 0, 1: 1, 2: 2, 3: 4}[cur_vtype.idx_fmt]
                    idx_advance += vert_count * idx_size
                else:
                    vs = cur_vtype.vertex_size()
                    cur_vaddr += vs * vert_count

    return calls


def strip_to_triangles(verts: List[ParsedVertex]) -> List[Tuple[int, int, int]]:
    faces = []
    flip = False
    for i in range(len(verts) - 2):
        a, b, c = i, i + 1, i + 2
        va, vb, vc = verts[a], verts[b], verts[c]
        if (va.x == vb.x and va.y == vb.y and va.z == vb.z) or \
           (vb.x == vc.x and vb.y == vc.y and vb.z == vc.z) or \
           (va.x == vc.x and va.y == vc.y and va.z == vc.z):
            flip = False
            continue
        if flip:
            faces.append((b, a, c))
        else:
            faces.append((a, b, c))
        flip = not flip
    return faces


def fan_to_triangles(verts: List[ParsedVertex]) -> List[Tuple[int, int, int]]:
    faces = []
    for i in range(1, len(verts) - 1):
        faces.append((0, i, i + 1))
    return faces


@dataclass
class PMF2Model:
    name: str = ""
    sections: int = 0
    bbox: Tuple[float, ...] = ()
    offset_table: List[int] = field(default_factory=list)
    draw_calls: List[DrawCall] = field(default_factory=list)
    ge_cmd_regions: List[Tuple[int, int]] = field(default_factory=list)
    model_names: List[str] = field(default_factory=list)


def find_ge_regions_in_pmf2(data: bytes) -> List[Tuple[int, int]]:
    regions = []
    sz = len(data)
    i = 0
    while i + 20 <= sz:
        if ru32(data, i) != 0x14000000:
            i += 4
            continue
        w1 = ru32(data, i + 4)
        if (w1 >> 24) & 0xFF != GE_CMD_BASE:
            i += 4
            continue
        w2 = ru32(data, i + 8)
        w3 = ru32(data, i + 12)
        cmd2 = (w2 >> 24) & 0xFF
        cmd3 = (w3 >> 24) & 0xFF
        if cmd2 not in (GE_CMD_IADDR, GE_CMD_VADDR) or cmd3 not in (GE_CMD_IADDR, GE_CMD_VADDR, GE_CMD_VERTEXTYPE):
            i += 4
            continue

        end = i + 4
        for j in range(i + 4, min(i + 0x800, sz), 4):
            w = ru32(data, j)
            c = (w >> 24) & 0xFF
            end = j + 4
            if c == GE_CMD_RET:
                break
            if c == GE_CMD_END or c == GE_CMD_FINISH:
                break
        regions.append((i, end))
        i = end
    return regions


def parse_pmf2(data: bytes) -> PMF2Model:
    model = PMF2Model()
    if len(data) < 0x20 or data[:4] != b"PMF2":
        return model

    model.sections = ru32(data, 4)
    hdr_size = ru32(data, 8)
    model.bbox = tuple(rf32(data, 0x10 + i * 4) for i in range(3))

    for off in range(0x20, min(hdr_size, len(data)), 4):
        v = ru32(data, off)
        if v > 0 and v < len(data):
            model.offset_table.append(v)

    for off in range(0, len(data) - 16):
        if data[off:off+1].isalpha() and data[off+1:off+2].isalpha():
            s = cstr(data, off, 32)
            if len(s) >= 4 and '_' in s and s.isascii() and all(c.isalnum() or c in '_.' for c in s):
                if s not in model.model_names:
                    model.model_names.append(s)

    regions = find_ge_regions_in_pmf2(data)
    model.ge_cmd_regions = regions

    for start, end in regions:
        cmds = scan_ge_display_list(data, start, end)
        origin_offset = start
        for c in cmds:
            if c["cmd"] == GE_CMD_ORIGIN:
                origin_offset = c["offset"]
                break
        calls = extract_draw_calls(cmds, origin_file_offset=origin_offset)
        model.draw_calls.extend(calls)

    return model


@dataclass
class SADHeader:
    magic: str = ""
    total_size: int = 0
    bone_count: int = 0
    data_offset: int = 0
    mesh_count: int = 0
    vertex_data_offset: int = 0


def parse_sad_header(data: bytes) -> SADHeader:
    hdr = SADHeader()
    if len(data) < 0x20:
        return hdr

    magic = data[:4]
    if magic == b"SAD ":
        hdr.magic = "SAD"
    else:
        hdr.magic = magic.decode("ascii", errors="replace")
        return hdr

    hdr.total_size = ru32(data, 4)
    hdr.bone_count = ru32(data, 8)
    hdr.data_offset = ru32(data, 12)
    hdr.mesh_count = ru32(data, 16)
    hdr.vertex_data_offset = ru32(data, 20)

    return hdr


def xor_dec(data: bytes, key: int) -> bytes:
    out = bytearray(len(data))
    kb = struct.pack("<I", key)
    for i in range(0, len(data) - 3, 4):
        out[i] = data[i] ^ kb[0]
        out[i + 1] = data[i + 1] ^ kb[1]
        out[i + 2] = data[i + 2] ^ kb[2]
        out[i + 3] = data[i + 3] ^ kb[3]
    for i in range((len(data) // 4) * 4, len(data)):
        out[i] = data[i] ^ kb[i % 4]
    return bytes(out)


def find_pzz_key(raw: bytes, sz: int) -> Optional[int]:
    raw_w0 = ru32(raw, 0)
    for fc in range(2, 200):
        key = raw_w0 ^ fc
        dec_partial = xor_dec(raw[:min(sz, 0x4000)], key)
        d0 = ru32(dec_partial, 0)
        if d0 != fc:
            continue
        table_bytes = (1 + fc) * 4
        padding_end = min((table_bytes + 0x7FF) & ~0x7FF, sz)
        ok = True
        for off in range(table_bytes, padding_end, 4):
            if off + 4 > len(dec_partial):
                break
            if ru32(dec_partial, off) != 0:
                ok = False
                break
        if ok:
            return key
    return None


def harvest_zlib(dec: bytes) -> List[bytes]:
    results = []
    headers = [b"\x78\x9c", b"\x78\x01", b"\x78\xda", b"\x78\x5e"]
    offsets = set()
    for hdr in headers:
        start = 0
        while True:
            idx = dec.find(hdr, start)
            if idx < 0:
                break
            offsets.add(idx)
            start = idx + 1

    for off in sorted(offsets):
        try:
            dobj = zlib.decompressobj(wbits=15)
            out = dobj.decompress(dec[off:], 16 * 1024 * 1024)
            out += dobj.flush()
            if len(out) >= 16:
                results.append(out)
        except Exception:
            pass
    return results


def extract_pzz_streams(pzz_data: bytes) -> List[bytes]:
    key = find_pzz_key(pzz_data, len(pzz_data))
    if key is None:
        return []
    dec = xor_dec(pzz_data, key)
    return harvest_zlib(dec)


KNOWN_MAGICS = {
    b"PMF2": "pmf2",
    b"SAD ": "sad",
    b"MIG.": "gim",
    b"GIM\x00": "gim",
}


def classify_stream(data: bytes) -> str:
    if len(data) < 4:
        return "unknown"
    magic4 = data[:4]
    if magic4 in KNOWN_MAGICS:
        return KNOWN_MAGICS[magic4]
    if len(data) >= 11 and data[:11] == b"MIG.00.1PSP":
        return "gim"
    return "unknown"


@dataclass
class BoneSection:
    index: int
    name: str
    offset: int
    size: int
    local_matrix: List[float]
    parent: int
    has_mesh: bool
    origin_offset: Optional[int]
    category: str


def _mat4_mul(a: List[float], b: List[float]) -> List[float]:
    r = [0.0] * 16
    for i in range(4):
        for j in range(4):
            r[i * 4 + j] = sum(a[i * 4 + k] * b[k * 4 + j] for k in range(4))
    return r


def _transform_pt(m: List[float], x: float, y: float, z: float) -> Tuple[float, float, float]:
    return (
        x * m[0] + y * m[4] + z * m[8] + m[12],
        x * m[1] + y * m[5] + z * m[9] + m[13],
        x * m[2] + y * m[6] + z * m[10] + m[14],
    )


def _transform_dir(m: List[float], x: float, y: float, z: float) -> Tuple[float, float, float]:
    return (
        x * m[0] + y * m[4] + z * m[8],
        x * m[1] + y * m[5] + z * m[9],
        x * m[2] + y * m[6] + z * m[10],
    )


def parse_pmf2_sections(data: bytes) -> Tuple[List[BoneSection], Tuple[float, float, float]]:
    if len(data) < 0x20 or data[:4] != b"PMF2":
        return [], (1.0, 1.0, 1.0)

    num_sec = ru32(data, 4)
    bbox = (rf32(data, 0x10), rf32(data, 0x14), rf32(data, 0x18))
    offsets = [ru32(data, 0x20 + i * 4) for i in range(num_sec)]

    sections = []
    for si in range(num_sec):
        so = offsets[si]
        se = offsets[si + 1] if si + 1 < num_sec else len(data)

        mat = [rf32(data, so + j * 4) for j in range(16)]
        name = cstr(data, so + 0x60, 16)
        parent_raw = ru32(data, so + 0x7C)
        parent = parent_raw if parent_raw < num_sec else -1

        cat = ""
        if '_m' in name:
            cat = "body"
        elif '_o' in name:
            cat = "ornament"
        elif '_w' in name:
            cat = "weapon"
        elif '_z' in name:
            cat = "effect"

        origin = None
        for off in range(so + 0x100, min(se, so + 0x200), 4):
            if off + 4 <= len(data) and ru32(data, off) == 0x14000000:
                origin = off
                break

        sections.append(BoneSection(
            index=si, name=name, offset=so, size=se - so,
            local_matrix=mat, parent=parent,
            has_mesh=origin is not None, origin_offset=origin,
            category=cat,
        ))

    return sections, bbox


def compute_world_matrices(sections: List[BoneSection]) -> Dict[int, List[float]]:
    world: Dict[int, List[float]] = {}

    def compute(idx: int) -> List[float]:
        if idx in world:
            return world[idx]
        s = sections[idx]
        if s.parent < 0:
            world[idx] = list(s.local_matrix)
            return world[idx]
        parent_world = compute(s.parent)
        world[idx] = _mat4_mul(s.local_matrix, parent_world)
        return world[idx]

    for i in range(len(sections)):
        compute(i)
    return world


@dataclass
class MeshPart:
    name: str
    vertices: List[ParsedVertex]
    faces: List[Tuple[int, int, int]]
    has_uv: bool = False
    has_normals: bool = False


def build_assembled_meshes(
    pmf2_data: bytes,
    categories: Optional[set] = None,
    swap_yz: bool = True,
) -> List[MeshPart]:
    sections, bbox = parse_pmf2_sections(pmf2_data)
    if not sections:
        return []

    world_mats = compute_world_matrices(sections)
    sx = bbox[0] / 32768.0
    sy = bbox[1] / 32768.0
    sz = bbox[2] / 32768.0

    parts: List[MeshPart] = []

    for sec in sections:
        if not sec.has_mesh or sec.origin_offset is None:
            continue
        if categories and sec.category not in categories:
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
        part_faces: List[Tuple[int, int, int]] = []
        has_uv = False
        has_nrm = False

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
                        pv.z *= sz
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
                    pv.z *= sz
                    raw_verts.append(pv)

                if not valid or len(raw_verts) < 3:
                    continue

            if vt.tc_fmt:
                has_uv = True
            if vt.nrm_fmt:
                has_nrm = True

            transformed = []
            for pv in raw_verts:
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
            parts.append(MeshPart(
                name=sec.name,
                vertices=part_verts,
                faces=part_faces,
                has_uv=has_uv,
                has_normals=has_nrm,
            ))

    return parts


def build_meshes_from_pmf2(
    pmf2_data: bytes,
    vertex_data: Optional[bytes] = None,
    scale: float = 1.0,
) -> List[MeshPart]:
    model = parse_pmf2(pmf2_data)
    if not model.draw_calls:
        return []

    parts: List[MeshPart] = []
    global_verts: List[ParsedVertex] = []
    global_faces: List[Tuple[int, int, int]] = []
    has_uv = False
    has_nrm = False

    for i, dc in enumerate(model.draw_calls):
        vt = dc.vtype
        vs = vt.vertex_size()
        if vs <= 0:
            continue

        vaddr = dc.vaddr
        prim = dc.prim_type
        count = dc.vertex_count

        src = pmf2_data
        if vaddr + vs * count > len(pmf2_data):
            if vertex_data and vaddr + vs * count <= len(vertex_data):
                src = vertex_data
            else:
                continue

        if vt.idx_fmt and dc.iaddr > 0:
            idx_size = {0: 0, 1: 1, 2: 2, 3: 4}[vt.idx_fmt]
            indices = []
            for ii in range(count):
                ioff = dc.iaddr + ii * idx_size
                if ioff + idx_size > len(src):
                    break
                if idx_size == 1:
                    indices.append(ru8(src, ioff))
                elif idx_size == 2:
                    indices.append(ru16(src, ioff))
                else:
                    indices.append(ru32(src, ioff))

            if len(indices) < 3:
                continue

            max_idx = max(indices) if indices else 0
            if vaddr + (max_idx + 1) * vs > len(src):
                continue

            vtx_cache: Dict[int, ParsedVertex] = {}
            verts = []
            valid = True
            for idx_val in indices:
                if idx_val not in vtx_cache:
                    off = vaddr + idx_val * vs
                    pv = decode_vertex(src, off, vt)
                    if pv is None:
                        valid = False
                        break
                    if vt.pos_fmt == 2 and (abs(pv.x) > 18000 or abs(pv.y) > 18000 or abs(pv.z) > 18000):
                        valid = False
                        break
                    if scale != 1.0 and not vt.through:
                        pv.x *= scale
                        pv.y *= scale
                        pv.z *= scale
                    vtx_cache[idx_val] = pv
                verts.append(vtx_cache[idx_val])

            if not valid or len(verts) < 3:
                continue
        else:
            verts = []
            valid = True
            for vi in range(count):
                off = vaddr + vi * vs
                pv = decode_vertex(src, off, vt)
                if pv is None:
                    valid = False
                    break
                if vt.pos_fmt == 2:
                    if abs(pv.x) > 18000 or abs(pv.y) > 18000 or abs(pv.z) > 18000:
                        valid = False
                        break
                if scale != 1.0 and not vt.through:
                    pv.x *= scale
                    pv.y *= scale
                    pv.z *= scale
                verts.append(pv)

            if not valid or len(verts) < 3:
                continue

        if vt.tc_fmt:
            has_uv = True
        if vt.nrm_fmt:
            has_nrm = True

        base_idx = len(global_verts)
        global_verts.extend(verts)

        if prim == PRIM_TRIANGLE_STRIP:
            local_faces = strip_to_triangles(verts)
        elif prim == PRIM_TRIANGLE_FAN:
            local_faces = fan_to_triangles(verts)
        elif prim == PRIM_TRIANGLES:
            local_faces = []
            for fi in range(0, len(verts) - 2, 3):
                local_faces.append((fi, fi + 1, fi + 2))
        else:
            continue

        for a, b, c in local_faces:
            global_faces.append((base_idx + a, base_idx + b, base_idx + c))

    if global_verts and global_faces:
        name = model.model_names[0] if model.model_names else "mesh"
        parts.append(MeshPart(
            name=name,
            vertices=global_verts,
            faces=global_faces,
            has_uv=has_uv,
            has_normals=has_nrm,
        ))

    return parts


def _write_mtl(path: Path, material_name: str, texture_file: Optional[str] = None):
    lines = [f"newmtl {material_name}", "Ka 0.2 0.2 0.2", "Kd 0.8 0.8 0.8", "Ks 0.1 0.1 0.1", "d 1.0"]
    if texture_file:
        lines.append(f"map_Kd {texture_file}")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def write_obj(path: Path, parts: List[MeshPart], scale: float = 1.0,
              mtl_file: Optional[str] = None, material_name: Optional[str] = None) -> int:
    lines = []

    if mtl_file:
        lines.append(f"mtllib {mtl_file}")

    v_base = 0
    vt_base = 0
    vn_base = 0
    total_faces = 0

    for part in parts:
        lines.append(f"o {part.name}")
        if material_name:
            lines.append(f"usemtl {material_name}")

        for pv in part.vertices:
            lines.append(f"v {pv.x:.6f} {pv.y:.6f} {pv.z:.6f}")

        if part.has_uv:
            for pv in part.vertices:
                lines.append(f"vt {pv.u:.6f} {1.0 - pv.v:.6f}")

        if part.has_normals:
            for pv in part.vertices:
                lines.append(f"vn {pv.nx:.6f} {pv.ny:.6f} {pv.nz:.6f}")

        for a, b, c in part.faces:
            i1 = a + v_base + 1
            i2 = b + v_base + 1
            i3 = c + v_base + 1

            if part.has_uv and part.has_normals:
                vt1 = a + vt_base + 1
                vt2 = b + vt_base + 1
                vt3 = c + vt_base + 1
                vn1 = a + vn_base + 1
                vn2 = b + vn_base + 1
                vn3 = c + vn_base + 1
                lines.append(f"f {i1}/{vt1}/{vn1} {i2}/{vt2}/{vn2} {i3}/{vt3}/{vn3}")
            elif part.has_uv:
                vt1 = a + vt_base + 1
                vt2 = b + vt_base + 1
                vt3 = c + vt_base + 1
                lines.append(f"f {i1}/{vt1} {i2}/{vt2} {i3}/{vt3}")
            elif part.has_normals:
                vn1 = a + vn_base + 1
                vn2 = b + vn_base + 1
                vn3 = c + vn_base + 1
                lines.append(f"f {i1}//{vn1} {i2}//{vn2} {i3}//{vn3}")
            else:
                lines.append(f"f {i1} {i2} {i3}")

            total_faces += 1

        v_base += len(part.vertices)
        if part.has_uv:
            vt_base += len(part.vertices)
        if part.has_normals:
            vn_base += len(part.vertices)

    path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return total_faces


def convert_pzz(pzz_path: Path, out_dir: Path, scale: float = 1.0) -> Dict[str, Any]:
    pzz_data = pzz_path.read_bytes()
    streams = extract_pzz_streams(pzz_data)
    if not streams:
        return {"error": "no_streams", "file": str(pzz_path)}

    classified = []
    for i, s in enumerate(streams):
        ct = classify_stream(s)
        classified.append({"index": i, "type": ct, "size": len(s)})

    pmf2_streams = [(i, s) for i, s in enumerate(streams) if classify_stream(s) == "pmf2"]
    sad_streams = [(i, s) for i, s in enumerate(streams) if classify_stream(s) == "sad"]
    gim_streams = [(i, s) for i, s in enumerate(streams) if classify_stream(s) == "gim"]

    result: Dict[str, Any] = {
        "file": str(pzz_path),
        "stream_count": len(streams),
        "streams": classified,
        "exported_models": [],
        "exported_textures": [],
    }

    out_dir.mkdir(parents=True, exist_ok=True)

    texture_pngs: List[str] = []
    for gi, (gim_idx, gim_data) in enumerate(gim_streams):
        gim_path = out_dir / f"stream{gim_idx:03d}.gim"
        gim_path.write_bytes(gim_data)
        tex_info: Dict[str, Any] = {
            "index": gim_idx,
            "file": str(gim_path),
            "size": len(gim_data),
        }
        if gim_to_png is not None:
            png_path = gim_path.with_suffix('.png')
            if gim_to_png(gim_data, png_path):
                tex_info["png_file"] = str(png_path)
                texture_pngs.append(png_path.name)
        result["exported_textures"].append(tex_info)

    for pi, (pmf2_idx, pmf2_data) in enumerate(pmf2_streams):
        model = parse_pmf2(pmf2_data)
        sections, bbox = parse_pmf2_sections(pmf2_data)
        model_info: Dict[str, Any] = {
            "index": pmf2_idx,
            "sections": model.sections,
            "bbox": bbox,
            "names": model.model_names[:10],
            "ge_regions": len(model.ge_cmd_regions),
            "draw_calls": len(model.draw_calls),
        }

        if sections:
            body_parts = build_assembled_meshes(pmf2_data, categories={"body", "ornament"})
            if body_parts:
                name = model.model_names[0] if model.model_names else f"model_{pi}"
                obj_path = out_dir / f"{name}.obj"

                mtl_name = f"{name}.mtl"
                if texture_pngs and pi == 0:
                    _write_mtl(out_dir / mtl_name, name, texture_pngs[0] if texture_pngs else None)
                    nfaces = write_obj(obj_path, body_parts, mtl_file=mtl_name, material_name=name)
                else:
                    nfaces = write_obj(obj_path, body_parts)

                model_info["obj_file"] = str(obj_path)
                model_info["exported_faces"] = nfaces
                model_info["exported_vertices"] = sum(len(p.vertices) for p in body_parts)
                model_info["exported_parts"] = len(body_parts)

            weapon_parts = build_assembled_meshes(pmf2_data, categories={"weapon"})
            if weapon_parts:
                wpn_name = model.model_names[0].split('_')[0] + "_weapons" if model.model_names else f"weapons_{pi}"
                wpn_path = out_dir / f"{wpn_name}.obj"
                write_obj(wpn_path, weapon_parts)
                model_info["weapon_obj_file"] = str(wpn_path)

        result["exported_models"].append(model_info)

    report_path = out_dir / "conversion_report.json"
    report_path.write_text(json.dumps(result, ensure_ascii=False, indent=2, default=str), encoding="utf-8")
    return result


def analyze_pmf2_detail(pmf2_path: Path) -> Dict[str, Any]:
    data = pmf2_path.read_bytes()
    model = parse_pmf2(data)

    result: Dict[str, Any] = {
        "file": str(pmf2_path),
        "size": len(data),
        "sections": model.sections,
        "bbox": model.bbox,
        "offset_table": [hex(o) for o in model.offset_table[:20]],
        "model_names": model.model_names[:20],
        "ge_regions": [],
        "draw_calls_detail": [],
    }

    for start, end in model.ge_cmd_regions:
        cmds = scan_ge_display_list(data, start, end)
        region_info = {
            "start": hex(start),
            "end": hex(end),
            "cmd_count": len(cmds),
            "commands": [],
        }
        for c in cmds[:50]:
            cmd_name = {
                0x00: "NOP", 0x01: "VADDR", 0x02: "IADDR", 0x04: "PRIM",
                0x10: "BASE", 0x12: "VERTEXTYPE", 0x14: "ORIGIN",
                0x0C: "END", 0x0F: "FINISH",
                0x2A: "BONEMATRIX#", 0x2B: "BONEMATRIXDATA",
            }.get(c["cmd"], f"CMD_{c['cmd']:02X}")
            extra = ""
            if c["cmd"] == GE_CMD_VERTEXTYPE:
                vt = VtypeInfo.decode(c["param"])
                extra = f" [{vt.describe()} stride={vt.vertex_size()}]"
            elif c["cmd"] == GE_CMD_PRIM:
                pt = (c["param"] >> 16) & 7
                vc = c["param"] & 0xFFFF
                extra = f" [{PRIM_NAMES.get(pt, str(pt))} count={vc}]"
            elif c["cmd"] == GE_CMD_VADDR:
                extra = f" [0x{c['param']:06X}]"
            region_info["commands"].append(f"0x{c['offset']:06X}: {cmd_name}{extra}")
        result["ge_regions"].append(region_info)

    for i, dc in enumerate(model.draw_calls[:50]):
        result["draw_calls_detail"].append({
            "index": i,
            "prim": PRIM_NAMES.get(dc.prim_type, str(dc.prim_type)),
            "vertex_count": dc.vertex_count,
            "vtype": f"0x{dc.vtype.raw:06X}",
            "vtype_desc": dc.vtype.describe(),
            "vertex_size": dc.vtype.vertex_size(),
            "vaddr": hex(dc.vaddr),
            "iaddr": hex(dc.iaddr),
        })

    return result


def main() -> int:
    ap = argparse.ArgumentParser(description="GVG Next Plus model/texture converter")
    sub = ap.add_subparsers(dest="cmd")

    p_conv = sub.add_parser("convert", help="Convert PZZ file to OBJ + textures")
    p_conv.add_argument("pzz", help="Input PZZ file path")
    p_conv.add_argument("--out", default=None, help="Output directory")
    p_conv.add_argument("--scale", type=float, default=1.0, help="Vertex position scale")

    p_analyze = sub.add_parser("analyze", help="Analyze a PMF2 file in detail")
    p_analyze.add_argument("pmf2", help="Input PMF2 file path")

    p_batch = sub.add_parser("batch", help="Batch convert all PZZ in Z_DATA.BIN inventory")
    p_batch.add_argument("z_bin", help="Path to Z_DATA.BIN")
    p_batch.add_argument("inventory", help="Path to Z_DATA.BIN.inventory.json")
    p_batch.add_argument("--out", default="converted_out", help="Output base directory")
    p_batch.add_argument("--scale", type=float, default=1.0, help="Vertex position scale")
    p_batch.add_argument("--filter", default="pl", help="Name filter (e.g. 'pl' for player models)")

    args = ap.parse_args()

    if args.cmd == "convert":
        pzz_path = Path(args.pzz)
        out_dir = Path(args.out) if args.out else pzz_path.parent / pzz_path.stem
        result = convert_pzz(pzz_path, out_dir, args.scale)
        print(json.dumps(result, indent=2, default=str))
        return 0

    elif args.cmd == "analyze":
        pmf2_path = Path(args.pmf2)
        result = analyze_pmf2_detail(pmf2_path)
        print(json.dumps(result, indent=2, default=str))
        return 0

    elif args.cmd == "batch":
        z_bin = Path(args.z_bin)
        inv_path = Path(args.inventory)
        out_base = Path(args.out)

        inv = json.loads(inv_path.read_text(encoding="utf-8"))
        entries = inv.get("entries", [])

        for entry in entries:
            name = entry.get("name", "")
            if not name.lower().endswith(".pzz"):
                continue
            if args.filter and args.filter.lower() not in name.lower():
                continue

            off = entry["offset"]
            sz = entry["size"]
            pzz_name = name.replace(".pzz", "")

            print(f"Converting {name}...")
            with z_bin.open("rb") as f:
                f.seek(off)
                pzz_data = f.read(sz)

            out_dir = out_base / pzz_name
            try:
                result = convert_pzz_from_bytes(pzz_data, name, out_dir, args.scale)
                models = result.get("exported_models", [])
                for m in models:
                    if m.get("obj_file"):
                        print(f"  -> {m['obj_file']} ({m.get('exported_faces', 0)} faces)")
            except Exception as e:
                print(f"  ERROR: {e}")

        return 0

    ap.print_help()
    return 1


def convert_pzz_from_bytes(pzz_data: bytes, name: str, out_dir: Path, scale: float = 1.0) -> Dict[str, Any]:
    streams = extract_pzz_streams(pzz_data)
    if not streams:
        return {"error": "no_streams", "name": name}

    classified = []
    for i, s in enumerate(streams):
        ct = classify_stream(s)
        classified.append({"index": i, "type": ct, "size": len(s)})

    pmf2_streams = [(i, s) for i, s in enumerate(streams) if classify_stream(s) == "pmf2"]
    gim_streams = [(i, s) for i, s in enumerate(streams) if classify_stream(s) == "gim"]

    result: Dict[str, Any] = {
        "name": name,
        "stream_count": len(streams),
        "streams": classified,
        "exported_models": [],
        "exported_textures": [],
    }

    out_dir.mkdir(parents=True, exist_ok=True)

    tex_pngs: List[str] = []
    for gi, (gim_idx, gim_data) in enumerate(gim_streams):
        gim_path = out_dir / f"stream{gim_idx:03d}.gim"
        gim_path.write_bytes(gim_data)
        tex_info2: Dict[str, Any] = {"index": gim_idx, "file": str(gim_path), "size": len(gim_data)}
        if gim_to_png is not None:
            png_path2 = gim_path.with_suffix('.png')
            if gim_to_png(gim_data, png_path2):
                tex_info2["png_file"] = str(png_path2)
                tex_pngs.append(png_path2.name)
        result["exported_textures"].append(tex_info2)

    for pi, (pmf2_idx, pmf2_data) in enumerate(pmf2_streams):
        model = parse_pmf2(pmf2_data)
        sections, bbox = parse_pmf2_sections(pmf2_data)
        model_info2: Dict[str, Any] = {
            "index": pmf2_idx,
            "sections": model.sections,
            "bbox": bbox,
            "names": model.model_names[:10],
            "draw_calls": len(model.draw_calls),
        }

        if sections:
            body_parts = build_assembled_meshes(pmf2_data, categories={"body", "ornament"})
            if body_parts:
                mname = model.model_names[0] if model.model_names else f"model_{pi}"
                obj_path = out_dir / f"{mname}.obj"
                mtl_name2 = f"{mname}.mtl"
                if tex_pngs and pi == 0:
                    _write_mtl(out_dir / mtl_name2, mname, tex_pngs[0])
                    nfaces = write_obj(obj_path, body_parts, mtl_file=mtl_name2, material_name=mname)
                else:
                    nfaces = write_obj(obj_path, body_parts)
                model_info2["obj_file"] = str(obj_path)
                model_info2["exported_faces"] = nfaces
                model_info2["exported_vertices"] = sum(len(p.vertices) for p in body_parts)

        result["exported_models"].append(model_info2)

    report_path = out_dir / "conversion_report.json"
    report_path.write_text(json.dumps(result, ensure_ascii=False, indent=2, default=str), encoding="utf-8")
    return result


if __name__ == "__main__":
    raise SystemExit(main())

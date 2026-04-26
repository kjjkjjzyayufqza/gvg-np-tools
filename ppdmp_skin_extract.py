#!/usr/bin/env python3
"""
Extract skinned meshes from a PPSSPP GE dump (.ppdmp) using the official record format.

Goal:
  - If Gundam is rendered as many small skinned parts, point clouds and non-skinned exports
    will look like "map/framework", while the skinned vertices reveal the actual model.

This script:
  - Parses the .ppdmp container (zstd-compressed commands + pushbuf).
  - Replays recorded REGISTERS ops to track:
      - VTYPE (GE_CMD_VERTEXTYPE 0x12)
      - PRIM  (GE_CMD_PRIM 0x04)
      - Bone matrices (GE_CMD_BONEMATRIXNUMBER 0x2A / GE_CMD_BONEMATRIXDATA 0x2B)
      - World matrix  (GE_CMD_WORLDMATRIXNUMBER 0x3A / GE_CMD_WORLDMATRIXDATA 0x3B) (optional)
  - For each VERTICES command, if VTYPE has weights, applies skinning using bone matrices.
  - Exports a single combined OBJ with one "o ..." per draw-object.

Notes:
  - PSP uses 24-bit floats for matrices: it's the top 24 bits of IEEE754 float (see PPSSPP getFloat24()).
  - PSP uses signed fixed point for positions when POS is S8/S16 (scaled by 1/128 or 1/32768).
"""

from __future__ import annotations

import argparse
import json
import math
import struct
from dataclasses import dataclass
from pathlib import Path
from typing import Dict, List, Optional, Tuple

import zstandard as zstd


MAGIC = b"PPSSPPGE"


class PPDMPError(RuntimeError):
    pass


def read_u32_le(data: bytes, off: int) -> int:
    if off + 4 > len(data):
        raise PPDMPError("Unexpected EOF while reading u32")
    return struct.unpack_from("<I", data, off)[0]


def get_float24(data24: int) -> float:
    u = (data24 & 0x00FFFFFF) << 8
    return struct.unpack("<f", struct.pack("<I", u))[0]


@dataclass(frozen=True)
class Header:
    version: int
    game_id: str


def parse_header(raw: bytes) -> Tuple[Header, int]:
    if len(raw) < 24:
        raise PPDMPError("File too small for header")
    if raw[:8] != MAGIC:
        raise PPDMPError("Bad magic (not a PPSSPP GE dump)")
    version = read_u32_le(raw, 8)
    game_id = raw[12:21].decode("ascii", errors="replace").rstrip("\x00")
    return Header(version=version, game_id=game_id), 24


@dataclass(frozen=True)
class Command:
    type_u8: int
    type_name: str
    sz: int
    ptr: int


def command_type_name(t: int) -> str:
    if t == 0:
        return "INIT"
    if t == 1:
        return "REGISTERS"
    if t == 2:
        return "VERTICES"
    if t == 3:
        return "INDICES"
    if t == 4:
        return "CLUT"
    if t == 5:
        return "TRANSFERSRC"
    if t == 6:
        return "MEMSET"
    if t == 7:
        return "MEMCPYDEST"
    if t == 8:
        return "MEMCPYDATA"
    if t == 9:
        return "DISPLAY"
    if t == 10:
        return "CLUTADDR"
    if t == 11:
        return "EDRAMTRANS"
    if 0x10 <= t <= 0x17:
        return "TEXTURE"
    if 0x18 <= t <= 0x1F:
        return "FRAMEBUF"
    return "UNKNOWN"


def parse_commands(buf: bytes, command_count: int) -> List[Command]:
    expected = command_count * 9  # packed u8 + u32 + u32
    if len(buf) != expected:
        raise PPDMPError(f"commands buffer size mismatch: got {len(buf)} expected {expected}")
    cmds: List[Command] = []
    off = 0
    for _ in range(command_count):
        t = buf[off]
        sz = struct.unpack_from("<I", buf, off + 1)[0]
        ptr = struct.unpack_from("<I", buf, off + 5)[0]
        cmds.append(Command(type_u8=t, type_name=command_type_name(t), sz=sz, ptr=ptr))
        off += 9
    return cmds


def read_compressed_block(raw: bytes, off: int) -> Tuple[bytes, int]:
    comp_size = read_u32_le(raw, off)
    off += 4
    end = off + comp_size
    if end > len(raw):
        raise PPDMPError("Compressed block out of range")
    return raw[off:end], end


def zstd_decompress_exact(blob: bytes, expected: int) -> bytes:
    out = zstd.ZstdDecompressor().decompress(blob)
    if len(out) != expected:
        raise PPDMPError(f"Decompressed size mismatch: got {len(out)} expected {expected}")
    return out


def load_ppdmp(path: Path) -> Tuple[Header, List[Command], bytes]:
    raw = path.read_bytes()
    header, off = parse_header(raw)
    if header.version < 5:
        raise PPDMPError(f"Unsupported dump version {header.version} (expected >= 5)")

    if off + 8 > len(raw):
        raise PPDMPError("Missing command_count/pushbuf_size")
    command_count = read_u32_le(raw, off)
    pushbuf_size = read_u32_le(raw, off + 4)
    off += 8

    comp_cmds, off = read_compressed_block(raw, off)
    comp_push, off = read_compressed_block(raw, off)

    cmds_buf = zstd.ZstdDecompressor().decompress(comp_cmds)
    push_buf = zstd_decompress_exact(comp_push, expected=pushbuf_size)
    cmds = parse_commands(cmds_buf, command_count=command_count)
    return header, cmds, push_buf


def iter_register_ops(pushbuf: bytes, cmd: Command) -> List[int]:
    if cmd.type_name != "REGISTERS":
        return []
    if cmd.ptr + cmd.sz > len(pushbuf):
        return []
    data = pushbuf[cmd.ptr : cmd.ptr + cmd.sz]
    ops: List[int] = []
    for i in range(0, len(data) & ~3, 4):
        ops.append(struct.unpack_from("<I", data, i)[0])
    return ops


def align(n: int, a: int) -> int:
    if a <= 1:
        return n
    return (n + (a - 1)) & ~(a - 1)


@dataclass(frozen=True)
class Layout:
    through: bool
    tc: int
    col: int
    nrm: int
    pos: int
    weighttype: int
    idx: int
    morphcount: int
    nweights: int
    onesize: int
    stride: int
    weightoff: Optional[int]
    posoff: Optional[int]


def compute_layout(vtype: int) -> Layout:
    through = bool(vtype & (1 << 23))
    tc = vtype & 0x3
    col = (vtype >> 2) & 0x7
    nrm = (vtype >> 5) & 0x3
    pos = (vtype >> 7) & 0x3
    weighttype = (vtype >> 9) & 0x3
    idx = (vtype >> 11) & 0x3
    morphcount = ((vtype >> 18) & 0x7) + 1
    nweights = ((vtype >> 14) & 0x7) + 1

    tcsize = [0, 2, 4, 8]
    tcalign = [0, 1, 2, 4]
    colsize = [0, 0, 0, 0, 2, 2, 2, 4]
    colalign = [0, 0, 0, 0, 2, 2, 2, 4]
    nrmsize = [0, 3, 6, 12]
    nrmalign = [0, 1, 2, 4]
    possize = [3, 3, 6, 12]
    posalign = [1, 1, 2, 4]
    wtsize = [0, 1, 2, 4]
    wtalign = [0, 1, 2, 4]

    size = 0
    biggest = 0
    weightoff: Optional[int] = None
    posoff: Optional[int] = None

    if weighttype:
        weightoff = size
        size += wtsize[weighttype] * nweights
        biggest = max(biggest, wtalign[weighttype])

    if tc:
        size = align(size, tcalign[tc])
        size += tcsize[tc]
        biggest = max(biggest, tcalign[tc])

    if col:
        size = align(size, colalign[col])
        size += colsize[col]
        biggest = max(biggest, colalign[col])

    if nrm:
        size = align(size, nrmalign[nrm])
        size += nrmsize[nrm]
        biggest = max(biggest, nrmalign[nrm])

    if pos:
        size = align(size, posalign[pos])
        posoff = size
        size += possize[pos]
        biggest = max(biggest, posalign[pos])

    if biggest == 0:
        biggest = 1
    onesize = align(size, biggest)
    stride = onesize * morphcount

    return Layout(
        through=through,
        tc=tc,
        col=col,
        nrm=nrm,
        pos=pos,
        weighttype=weighttype,
        idx=idx,
        morphcount=morphcount,
        nweights=nweights,
        onesize=onesize,
        stride=stride,
        weightoff=weightoff,
        posoff=posoff,
    )


def decode_prim(op: int, prev_prim: int) -> Tuple[int, int]:
    prim = (op >> 16) & 7
    count = op & 0xFFFF
    if prim == 7:
        prim = prev_prim
    return prim, count


def parse_indices(blob: bytes, idx: int) -> Optional[List[int]]:
    if idx == 0:
        return None
    if idx == 1:
        return list(blob)
    if idx == 2:
        if len(blob) % 2 != 0:
            return None
        return list(struct.unpack_from("<" + "H" * (len(blob) // 2), blob, 0))
    if idx == 3:
        if len(blob) % 4 != 0:
            return None
        return list(struct.unpack_from("<" + "I" * (len(blob) // 4), blob, 0))
    return None


def build_triangles(prim: int, indices: List[int]) -> List[Tuple[int, int, int]]:
    faces: List[Tuple[int, int, int]] = []
    if prim == 3:
        for i in range(0, len(indices) - 2, 3):
            a, b, c = indices[i], indices[i + 1], indices[i + 2]
            if a == b or b == c or a == c:
                continue
            faces.append((a, b, c))
        return faces
    if prim == 4:
        flip = False
        for i in range(len(indices) - 2):
            a, b, c = indices[i], indices[i + 1], indices[i + 2]
            if a == b or b == c or a == c:
                flip = False
                continue
            faces.append((b, a, c) if flip else (a, b, c))
            flip = not flip
        return faces
    if prim == 5:
        if len(indices) < 3:
            return faces
        center = indices[0]
        for i in range(1, len(indices) - 1):
            a, b, c = center, indices[i], indices[i + 1]
            if a == b or b == c or a == c:
                continue
            faces.append((a, b, c))
        return faces
    return faces


def read_pos(ptr: bytes, off: int, layout: Layout) -> Optional[Tuple[float, float, float]]:
    if layout.posoff is None:
        return None
    o = off + layout.posoff
    if layout.pos == 1:
        if o + 3 > len(ptr):
            return None
        x, y, z = struct.unpack_from("<bbb", ptr, o)
        return x * (1.0 / 128.0), y * (1.0 / 128.0), z * (1.0 / 128.0)
    if layout.pos == 2:
        if o + 6 > len(ptr):
            return None
        x, y, z = struct.unpack_from("<hhh", ptr, o)
        return x * (1.0 / 32768.0), y * (1.0 / 32768.0), z * (1.0 / 32768.0)
    if layout.pos == 3:
        if o + 12 > len(ptr):
            return None
        x, y, z = struct.unpack_from("<fff", ptr, o)
        if not (math.isfinite(x) and math.isfinite(y) and math.isfinite(z)):
            return None
        return x, y, z
    return None


def read_weights(ptr: bytes, off: int, layout: Layout) -> Optional[List[float]]:
    if layout.weightoff is None or layout.weighttype == 0:
        return None
    o = off + layout.weightoff
    n = layout.nweights
    if layout.weighttype == 1:
        if o + n > len(ptr):
            return None
        ws = list(ptr[o : o + n])
        return [w * (1.0 / 128.0) for w in ws]
    if layout.weighttype == 2:
        if o + n * 2 > len(ptr):
            return None
        ws = struct.unpack_from("<" + "H" * n, ptr, o)
        return [w * (1.0 / 32768.0) for w in ws]
    if layout.weighttype == 3:
        if o + n * 4 > len(ptr):
            return None
        ws = struct.unpack_from("<" + "f" * n, ptr, o)
        return [float(w) for w in ws]
    return None


def mat43_mul_vec3(m: List[float], v: Tuple[float, float, float]) -> Tuple[float, float, float]:
    x, y, z = v
    return (
        m[0] * x + m[1] * y + m[2] * z + m[3],
        m[4] * x + m[5] * y + m[6] * z + m[7],
        m[8] * x + m[9] * y + m[10] * z + m[11],
    )


def skin_pos(pos: Tuple[float, float, float], weights: List[float], bone_mtx: List[float]) -> Tuple[float, float, float]:
    # bone_mtx is 8 * 12 floats (row-major 3x4 per bone).
    m = [0.0] * 12
    for j, w in enumerate(weights[:8]):
        if w == 0.0:
            continue
        base = j * 12
        for i in range(12):
            m[i] += w * bone_mtx[base + i]
    return mat43_mul_vec3(m, pos)


def write_combined_obj(path: Path, parts: List[Tuple[str, List[Tuple[float, float, float]], List[Tuple[int, int, int]]]], header: str) -> None:
    lines: List[str] = [header.rstrip()]
    v_base = 0
    for name, verts, faces in parts:
        lines.append(f"o {name}")
        for x, y, z in verts:
            lines.append(f"v {x:.6f} {y:.6f} {z:.6f}")
        for a, b, c in faces:
            lines.append(f"f {a + 1 + v_base} {b + 1 + v_base} {c + 1 + v_base}")
        v_base += len(verts)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> int:
    ap = argparse.ArgumentParser(description="Extract skinned meshes from PPSSPP .ppdmp and export a combined OBJ.")
    ap.add_argument("--ppdmp", default="test/ppsspp_dump/NPJH50107_0001.ppdmp")
    ap.add_argument("--out-dir", default="test/ppsspp_dump/skin_out")
    ap.add_argument("--min-faces", type=int, default=5)
    ap.add_argument("--apply-world", action="store_true", help="Apply world matrix after skinning (float24).")
    ap.add_argument("--max-parts", type=int, default=2000)
    args = ap.parse_args()

    ppdmp_path = Path(args.ppdmp)
    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)

    header, cmds, pushbuf = load_ppdmp(ppdmp_path)

    # State tracked from register ops.
    current_vtype = 0
    prev_prim = 4
    pending_prim: Optional[Tuple[int, int]] = None

    bone_mtx = [0.0] * (12 * 8)
    bone_num = 0
    world_mtx = [0.0] * 12
    world_num = 0

    last_indices: Optional[Command] = None
    last_indices_i: Optional[int] = None

    parts: List[Tuple[str, List[Tuple[float, float, float]], List[Tuple[int, int, int]]]] = []
    index_rows: List[Dict[str, object]] = []

    for ci, cmd in enumerate(cmds):
        if cmd.type_name == "REGISTERS":
            ops = iter_register_ops(pushbuf, cmd)
            for op in ops:
                opc = (op >> 24) & 0xFF
                arg = op & 0x00FFFFFF
                if opc == 0x12:
                    current_vtype = arg
                elif opc == 0x04:
                    prim, count = decode_prim(op, prev_prim=prev_prim)
                    prev_prim = prim
                    pending_prim = (prim, count)
                elif opc == 0x2A:
                    bone_num = arg & 0x7F
                elif opc == 0x2B:
                    if bone_num < len(bone_mtx):
                        bone_mtx[bone_num] = get_float24(arg)
                    bone_num += 1
                    if bone_num >= len(bone_mtx):
                        bone_num = 0
                elif opc == 0x3A:
                    world_num = arg & 0x0F
                elif opc == 0x3B:
                    if world_num < len(world_mtx):
                        world_mtx[world_num] = get_float24(arg)
                    world_num += 1
                    if world_num >= len(world_mtx):
                        world_num = 0

        elif cmd.type_name == "INDICES":
            last_indices = cmd
            last_indices_i = ci

        elif cmd.type_name == "VERTICES":
            vtype = current_vtype
            layout = compute_layout(vtype)
            if layout.weighttype == 0 or layout.weightoff is None:
                last_indices = None
                last_indices_i = None
                pending_prim = None
                continue
            if layout.stride <= 0 or layout.posoff is None:
                last_indices = None
                last_indices_i = None
                pending_prim = None
                continue
            if cmd.ptr + cmd.sz > len(pushbuf):
                continue

            prim, _count = pending_prim if pending_prim is not None else (prev_prim, 0)
            pending_prim = None

            vblob = pushbuf[cmd.ptr : cmd.ptr + cmd.sz]
            vert_count = cmd.sz // layout.stride
            if vert_count <= 0:
                continue

            idx_list: List[int]
            if layout.idx != 0 and last_indices is not None and last_indices_i == ci - 1:
                icmd = last_indices
                if icmd.ptr + icmd.sz > len(pushbuf):
                    continue
                iblob = pushbuf[icmd.ptr : icmd.ptr + icmd.sz]
                parsed = parse_indices(iblob, layout.idx)
                if parsed is None:
                    continue
                idx_list = parsed
            else:
                idx_list = list(range(vert_count))

            faces = build_triangles(prim, idx_list)
            if len(faces) < args.min_faces:
                last_indices = None
                last_indices_i = None
                continue

            used_set = set()
            for a, b, c in faces:
                used_set.add(a)
                used_set.add(b)
                used_set.add(c)
            used = [i for i in sorted(used_set) if 0 <= i < vert_count]
            if len(used) < 3:
                last_indices = None
                last_indices_i = None
                continue

            remap = {old: new for new, old in enumerate(used)}
            compact_faces: List[Tuple[int, int, int]] = []
            for a, b, c in faces:
                if a not in remap or b not in remap or c not in remap:
                    continue
                compact_faces.append((remap[a], remap[b], remap[c]))

            compact_verts: List[Tuple[float, float, float]] = []
            for vi in used:
                base = vi * layout.stride
                p = read_pos(vblob, base, layout)
                w = read_weights(vblob, base, layout)
                if p is None or w is None:
                    compact_verts.append((0.0, 0.0, 0.0))
                    continue
                sp = skin_pos(p, w, bone_mtx)
                if args.apply_world:
                    sp = mat43_mul_vec3(world_mtx, sp)
                compact_verts.append(sp)

            name = f"cmd{ci:05d}_prim{prim}_faces{len(compact_faces)}_wt{layout.weighttype}_nw{layout.nweights}"
            parts.append((name, compact_verts, compact_faces))
            index_rows.append(
                {
                    "name": name,
                    "cmd_index": ci,
                    "prim": prim,
                    "faces": len(compact_faces),
                    "verts": len(compact_verts),
                    "vtype": vtype,
                    "weighttype": layout.weighttype,
                    "nweights": layout.nweights,
                    "stride": layout.stride,
                }
            )

            if len(parts) >= args.max_parts:
                break

            last_indices = None
            last_indices_i = None

    index_rows.sort(key=lambda r: int(r["faces"]), reverse=True)
    out_obj = out_dir / "skinned_combined.obj"
    hdr = f"# ppdmp={ppdmp_path.name} game_id={header.game_id} version={header.version}\n# parts={len(parts)} apply_world={args.apply_world}\n"
    write_combined_obj(out_obj, parts, header=hdr)

    out_index = out_dir / "skinned_index.json"
    out_index.write_text(
        json.dumps(
            {
                "ppdmp": str(ppdmp_path),
                "game_id": header.game_id,
                "version": header.version,
                "apply_world": args.apply_world,
                "parts": len(parts),
                "min_faces": args.min_faces,
                "max_parts": args.max_parts,
                "skinned_obj": str(out_obj),
                "objects_sorted": index_rows[: min(2000, len(index_rows))],
            },
            ensure_ascii=False,
            indent=2,
        ),
        encoding="utf-8",
    )

    print(f"Wrote: {out_obj}")
    print(f"Wrote: {out_index}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())


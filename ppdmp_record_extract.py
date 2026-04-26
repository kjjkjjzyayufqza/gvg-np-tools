#!/usr/bin/env python3
"""
Parse a PPSSPP GE frame dump (.ppdmp) and export vertex point clouds to OBJ.

This follows PPSSPP's official record format:
  GPU/Debugger/RecordFormat.h and GPU/Debugger/Record.cpp

File layout (VERSION >= 5 uses zstd):
  Header (24 bytes)
  u32 command_count
  u32 pushbuf_size
  WriteCompressed(commands): u32 compressed_size + zstd(compressed bytes)
  WriteCompressed(pushbuf):  u32 compressed_size + zstd(compressed bytes)

The command stream includes:
  - INIT: initial GE state dump (512 u32 registers, 2048 bytes)
  - REGISTERS: raw GE command words (u32), excluding pointer-like commands
  - VERTICES / INDICES: exact RAM bytes used by a draw call

We track the current VTYPE (0x12) by replaying recorded REGISTERS words, and decode positions
from VERTICES blobs using the PSP GE VTYPE bitfield.
"""

from __future__ import annotations

import argparse
import json
import math
import struct
from dataclasses import dataclass
from pathlib import Path
from typing import Dict, Iterable, List, Literal, Optional, Tuple

import zstandard as zstd


MAGIC = b"PPSSPPGE"


class PPDMPError(RuntimeError):
    pass


def read_u32_le(data: bytes, off: int) -> int:
    if off + 4 > len(data):
        raise PPDMPError("Unexpected EOF while reading u32")
    return struct.unpack_from("<I", data, off)[0]


def zstd_decompress_exact(blob: bytes, expected: int) -> bytes:
    out = zstd.ZstdDecompressor().decompress(blob)
    if len(out) != expected:
        raise PPDMPError(f"Decompressed size mismatch: got {len(out)} expected {expected}")
    return out


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


CommandType = Literal[
    "INIT",
    "REGISTERS",
    "VERTICES",
    "INDICES",
    "CLUT",
    "TRANSFERSRC",
    "MEMSET",
    "MEMCPYDEST",
    "MEMCPYDATA",
    "DISPLAY",
    "CLUTADDR",
    "EDRAMTRANS",
    "TEXTURE",
    "FRAMEBUF",
    "UNKNOWN",
]


def command_type_name(t: int) -> CommandType:
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


@dataclass(frozen=True)
class Command:
    type_u8: int
    type_name: CommandType
    sz: int
    ptr: int


def parse_commands(buf: bytes, command_count: int) -> List[Command]:
    # GPURecord::Command is packed: u8 type; u32 sz; u32 ptr => 9 bytes
    expected = command_count * 9
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


def load_ppdmp(path: Path) -> Dict[str, object]:
    raw = path.read_bytes()
    header, off = parse_header(raw)

    if off + 8 > len(raw):
        raise PPDMPError("Missing command_count/pushbuf_size")
    command_count = read_u32_le(raw, off)
    pushbuf_size = read_u32_le(raw, off + 4)
    off += 8

    comp_cmds, off = read_compressed_block(raw, off)
    comp_push, off = read_compressed_block(raw, off)

    # PPSSPP uses zstd for version >= 5.
    if header.version < 5:
        raise PPDMPError(f"Unsupported dump version {header.version} (expected >= 5)")

    cmds_buf = zstd.ZstdDecompressor().decompress(comp_cmds)
    push_buf = zstd_decompress_exact(comp_push, expected=pushbuf_size)
    cmds = parse_commands(cmds_buf, command_count=command_count)

    return {
        "header": header,
        "command_count": command_count,
        "pushbuf_size": pushbuf_size,
        "commands": cmds,
        "pushbuf": push_buf,
        "raw_size": len(raw),
        "trailing_bytes": len(raw) - off,
    }


VFmt = Literal["none", "8", "16", "32f"]


@dataclass(frozen=True)
class VType:
    raw: int
    through: bool
    morph_count: int
    weight_count: int
    index_fmt: str
    weight_fmt: VFmt
    tex_fmt: VFmt
    color_fmt: str
    normal_fmt: VFmt
    pos_fmt: VFmt


def decode_vtype(vtype_arg: int) -> VType:
    through = bool((vtype_arg >> 23) & 1)
    morph_count = ((vtype_arg >> 18) & 0x7) + 1
    weight_count = ((vtype_arg >> 14) & 0x7) + 1

    idx_bits = (vtype_arg >> 11) & 0x3
    index_fmt = {0: "none", 1: "u8", 2: "u16", 3: "u32"}.get(idx_bits, "unknown")

    def fmt2(bits: int) -> VFmt:
        return {0: "none", 1: "8", 2: "16", 3: "32f"}.get(bits, "none")  # type: ignore[return-value]

    weight_fmt = fmt2((vtype_arg >> 9) & 0x3)
    pos_fmt = fmt2((vtype_arg >> 7) & 0x3)
    normal_fmt = fmt2((vtype_arg >> 5) & 0x3)
    tex_fmt = fmt2((vtype_arg >> 0) & 0x3)

    color_bits = (vtype_arg >> 2) & 0x7
    color_fmt = {
        0: "none",
        4: "bgr5650",
        5: "abgr5551",
        6: "abgr4444",
        7: "abgr8888",
    }.get(color_bits, "other")

    return VType(
        raw=vtype_arg,
        through=through,
        morph_count=morph_count,
        weight_count=weight_count,
        index_fmt=index_fmt,
        weight_fmt=weight_fmt,
        tex_fmt=tex_fmt,
        color_fmt=color_fmt,
        normal_fmt=normal_fmt,
        pos_fmt=pos_fmt,
    )


def fmt_size(fmt: VFmt) -> int:
    if fmt == "none":
        return 0
    if fmt == "8":
        return 1
    if fmt == "16":
        return 2
    if fmt == "32f":
        return 4
    raise PPDMPError(f"Unknown fmt {fmt!r}")


def vertex_stride(vt: VType) -> int:
    # Approximate stride following common PSP packing rules.
    stride = 0
    stride += 2 * fmt_size(vt.tex_fmt)
    if vt.color_fmt == "none":
        stride += 0
    elif vt.color_fmt in ("bgr5650", "abgr5551", "abgr4444"):
        stride += 2
    elif vt.color_fmt == "abgr8888":
        stride += 4
    else:
        return 0
    stride += 3 * fmt_size(vt.normal_fmt)
    stride += (vt.weight_count * fmt_size(vt.weight_fmt)) if vt.weight_fmt != "none" else 0
    stride += 3 * fmt_size(vt.pos_fmt)
    if vt.morph_count > 1:
        stride *= vt.morph_count
    return stride


def pos_offset(vt: VType) -> Optional[int]:
    if vt.pos_fmt == "none":
        return None
    off = 0
    off += 2 * fmt_size(vt.tex_fmt)
    if vt.color_fmt == "none":
        off += 0
    elif vt.color_fmt in ("bgr5650", "abgr5551", "abgr4444"):
        off += 2
    elif vt.color_fmt == "abgr8888":
        off += 4
    else:
        return None
    off += 3 * fmt_size(vt.normal_fmt)
    off += (vt.weight_count * fmt_size(vt.weight_fmt)) if vt.weight_fmt != "none" else 0
    return off


def read_pos(data: bytes, off: int, vt: VType, scale16: float) -> Optional[Tuple[float, float, float]]:
    po = pos_offset(vt)
    if po is None:
        return None
    o = off + po
    if vt.pos_fmt == "32f":
        if o + 12 > len(data):
            return None
        x, y, z = struct.unpack_from("<fff", data, o)
        if not (math.isfinite(x) and math.isfinite(y) and math.isfinite(z)):
            return None
        return x, y, z
    if vt.pos_fmt == "16":
        if o + 6 > len(data):
            return None
        x, y, z = struct.unpack_from("<hhh", data, o)
        return x * scale16, y * scale16, z * scale16
    if vt.pos_fmt == "8":
        if o + 3 > len(data):
            return None
        x, y, z = struct.unpack_from("<bbb", data, o)
        return float(x) * scale16, float(y) * scale16, float(z) * scale16
    return None


def iter_register_ops(pushbuf: bytes, cmd: Command) -> Iterable[int]:
    if cmd.type_name != "REGISTERS":
        return
    if cmd.ptr + cmd.sz > len(pushbuf):
        return
    data = pushbuf[cmd.ptr : cmd.ptr + cmd.sz]
    for i in range(0, len(data) & ~3, 4):
        yield struct.unpack_from("<I", data, i)[0]


def export_obj(path: Path, points: List[Tuple[float, float, float]], header: str) -> None:
    lines = [header.rstrip()]
    for x, y, z in points:
        lines.append(f"v {x:.6f} {y:.6f} {z:.6f}")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def export_obj_mesh(path: Path, verts: List[Tuple[float, float, float]], faces: List[Tuple[int, int, int]], header: str) -> None:
    lines = [header.rstrip()]
    for x, y, z in verts:
        lines.append(f"v {x:.6f} {y:.6f} {z:.6f}")
    for a, b, c in faces:
        # OBJ indices are 1-based.
        lines.append(f"f {a + 1} {b + 1} {c + 1}")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def export_obj_mesh_combined(
    path: Path,
    parts: List[Tuple[str, List[Tuple[float, float, float]], List[Tuple[int, int, int]]]],
    header: str,
) -> None:
    """
    Export a single OBJ containing multiple objects (o name) with independent vertex lists.
    Each part is appended with a running vertex index offset.
    """
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


def decode_prim(op: int, prev_prim: int) -> Tuple[int, int]:
    """
    Decode GE_CMD_PRIM.
    Format (from PPSSPP): prim = (op >> 16) & 7, count = op & 0xFFFF.
    prim==7 means KEEP_PREVIOUS.
    """
    prim = (op >> 16) & 7
    count = op & 0xFFFF
    if prim == 7:
        prim = prev_prim
    return prim, count


def read_indices(blob: bytes, index_fmt: str, count: int) -> Optional[List[int]]:
    if count <= 0:
        return []
    if index_fmt == "none":
        return None
    if index_fmt == "u8":
        if len(blob) < count:
            return None
        return list(blob[:count])
    if index_fmt == "u16":
        need = count * 2
        if len(blob) < need:
            return None
        return list(struct.unpack_from("<" + "H" * count, blob, 0))
    if index_fmt == "u32":
        need = count * 4
        if len(blob) < need:
            return None
        return list(struct.unpack_from("<" + "I" * count, blob, 0))
    return None


def build_triangles(prim: int, indices: List[int]) -> List[Tuple[int, int, int]]:
    """
    Build triangle faces from indices based on GEPrimitiveType.
      3: TRIANGLES
      4: TRIANGLE_STRIP
      5: TRIANGLE_FAN
    Other types are ignored for now (points/lines/rectangles).
    """
    faces: List[Tuple[int, int, int]] = []
    if prim == 3:  # TRIANGLES
        for i in range(0, len(indices) - 2, 3):
            a, b, c = indices[i], indices[i + 1], indices[i + 2]
            if a == b or b == c or a == c:
                continue
            faces.append((a, b, c))
        return faces
    if prim == 4:  # TRIANGLE_STRIP
        flip = False
        for i in range(len(indices) - 2):
            a, b, c = indices[i], indices[i + 1], indices[i + 2]
            if a == b or b == c or a == c:
                flip = False
                continue
            if flip:
                faces.append((b, a, c))
            else:
                faces.append((a, b, c))
            flip = not flip
        return faces
    if prim == 5:  # TRIANGLE_FAN
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


@dataclass
class Draw:
    prim: int
    count: int
    vtype_arg: int
    vcmd: Command
    vcmd_index: int
    icmd: Optional[Command]
    icmd_index: Optional[int]
    prim_op: int
    registers_cmd_index: int


def main() -> int:
    ap = argparse.ArgumentParser(description="Extract OBJ point clouds from PPSSPP .ppdmp (record format).")
    ap.add_argument("--ppdmp", default="test/ppsspp_dump/NPJH50107_0001.ppdmp", help="Path to .ppdmp")
    ap.add_argument("--out-dir", default="test/ppsspp_dump/record_out", help="Output directory")
    ap.add_argument("--max-draws", type=int, default=50, help="Max draw calls to export (largest first)")
    ap.add_argument("--min-bytes", type=int, default=2048, help="Minimum VERTICES blob size to export")
    ap.add_argument("--min-faces", type=int, default=256, help="Minimum triangle faces to export")
    ap.add_argument("--scale16", type=float, default=1.0 / 256.0, help="Scale for fixed-point positions")
    ap.add_argument("--write-combined", action="store_true", help="Also write a combined OBJ for all exported draws")
    args = ap.parse_args()

    ppdmp_path = Path(args.ppdmp)
    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)

    dump = load_ppdmp(ppdmp_path)
    header: Header = dump["header"]  # type: ignore[assignment]
    cmds: List[Command] = dump["commands"]  # type: ignore[assignment]
    pushbuf: bytes = dump["pushbuf"]  # type: ignore[assignment]

    current_vtype = 0
    exported = 0
    report: Dict[str, object] = {
        "ppdmp": str(ppdmp_path),
        "version": header.version,
        "game_id": header.game_id,
        "command_count": dump["command_count"],
        "pushbuf_size": dump["pushbuf_size"],
        "exported_draws": [],
    }

    draws: List[Draw] = []
    last_indices: Optional[Command] = None
    last_indices_cmd_index: Optional[int] = None
    pending_prim: Optional[Tuple[int, int, int, int]] = None  # (prim, count, op, registers_cmd_index)
    prev_prim = 4  # TRIANGLE_STRIP is the most common in this dump.

    for idx, cmd in enumerate(cmds):
        if cmd.type_name == "REGISTERS":
            for op in iter_register_ops(pushbuf, cmd):
                opc = (op >> 24) & 0xFF
                if opc == 0x12:  # GE_CMD_VERTEXTYPE
                    current_vtype = op & 0x00FFFFFF
                elif opc == 0x04:  # GE_CMD_PRIM
                    prim, count = decode_prim(op, prev_prim=prev_prim)
                    prev_prim = prim
                    pending_prim = (prim, count, op, idx)
        elif cmd.type_name == "VERTICES":
            if cmd.ptr + cmd.sz > len(pushbuf):
                continue

            icmd = last_indices if (last_indices_cmd_index == idx - 1) else None
            icmd_i = last_indices_cmd_index if (last_indices_cmd_index == idx - 1) else None

            if pending_prim is not None:
                prim, count, prim_op, reg_i = pending_prim
            else:
                prim, count, prim_op, reg_i = prev_prim, 0, 0, -1

            draws.append(
                Draw(
                    prim=prim,
                    count=count,
                    vtype_arg=current_vtype,
                    vcmd=cmd,
                    vcmd_index=idx,
                    icmd=icmd,
                    icmd_index=icmd_i,
                    prim_op=prim_op,
                    registers_cmd_index=reg_i,
                )
            )

            last_indices = None
            last_indices_cmd_index = None
            pending_prim = None
        elif cmd.type_name == "INDICES":
            last_indices = cmd
            last_indices_cmd_index = idx

    # We'll compute faces and then sort by face count later.
    # Keep draws in original order until then.

    exported_draws: List[Tuple[int, Draw, List[Tuple[float, float, float]], List[Tuple[int, int, int]], VType, int]] = []

    for d in draws:
        vcmd = d.vcmd
        if vcmd.sz < args.min_bytes:
            continue
        if vcmd.ptr + vcmd.sz > len(pushbuf):
            continue

        vt = decode_vtype(d.vtype_arg)
        stride = vertex_stride(vt)
        if stride <= 0:
            continue
        vert_count = vcmd.sz // stride
        if vert_count <= 0:
            continue

        vblob = pushbuf[vcmd.ptr : vcmd.ptr + vcmd.sz]
        verts: List[Tuple[float, float, float]] = []
        for i in range(vert_count):
            p = read_pos(vblob, i * stride, vt, scale16=args.scale16)
            if not p:
                verts.append((0.0, 0.0, 0.0))
            else:
                verts.append(p)

        # Indices (if any) are stored as a separate recorded blob.
        idx_list: List[int]
        if d.icmd is not None:
            icmd = d.icmd
            if icmd.ptr + icmd.sz > len(pushbuf):
                continue
            iblob = pushbuf[icmd.ptr : icmd.ptr + icmd.sz]
            # Prefer count from the indices blob size (more reliable association than PRIM count.)
            if vt.index_fmt == "u16":
                count = icmd.sz // 2
            elif vt.index_fmt == "u8":
                count = icmd.sz
            elif vt.index_fmt == "u32":
                count = icmd.sz // 4
            else:
                count = d.count
            d.count = count
            parsed = read_indices(iblob, vt.index_fmt, count=d.count)
            if parsed is None:
                continue
            idx_list = parsed
        else:
            # Non-indexed draw: count is implied by VERTICES size.
            if d.count <= 0 or d.count > len(verts):
                d.count = min(len(verts), 0xFFFF)
            idx_list = list(range(d.count))

        faces = build_triangles(d.prim, idx_list)
        if not faces:
            continue

        # Compact: keep only vertices referenced by faces to reduce scattered noise.
        used_set = set()
        for a, b, c in faces:
            used_set.add(a)
            used_set.add(b)
            used_set.add(c)
        used = [i for i in sorted(used_set) if 0 <= i < len(verts)]
        if len(used) < 3:
            continue
        remap = {old: new for new, old in enumerate(used)}
        compact_faces: List[Tuple[int, int, int]] = []
        for a, b, c in faces:
            if a not in remap or b not in remap or c not in remap:
                continue
            compact_faces.append((remap[a], remap[b], remap[c]))
        compact_verts = [verts[i] for i in used]

        if len(compact_faces) < args.min_faces:
            continue

        exported_draws.append((len(compact_faces), d, compact_verts, compact_faces, vt, stride))

    # Export largest meshes first by face count.
    exported_draws.sort(key=lambda x: x[0], reverse=True)

    if args.write_combined:
        parts: List[Tuple[str, List[Tuple[float, float, float]], List[Tuple[int, int, int]]]] = []
        for face_count, d, compact_verts, compact_faces, vt, stride in exported_draws:
            name = f"cmd{d.vcmd_index:05d}_prim{d.prim}_faces{face_count}"
            parts.append((name, compact_verts, compact_faces))
        combined_path = out_dir / "frame_combined.obj"
        combined_hdr = (
            f"# ppdmp={ppdmp_path.name} game_id={header.game_id} version={header.version}\n"
            f"# combined_parts={len(parts)} min_faces={args.min_faces} min_bytes={args.min_bytes} scale16={args.scale16}\n"
        )
        export_obj_mesh_combined(combined_path, parts, header=combined_hdr)
        report["combined_obj"] = str(combined_path)
        report["combined_parts"] = len(parts)

    for face_count, d, compact_verts, compact_faces, vt, stride in exported_draws[: args.max_draws]:
        vcmd = d.vcmd

        obj_name = (
            f"draw_{exported:04d}_cmd{d.vcmd_index:05d}_prim{d.prim}_count{d.count}"
            f"_vptr{vcmd.ptr:08x}_vsz{vcmd.sz:08x}_vtype{d.vtype_arg:06x}_stride{stride}.obj"
        )
        obj_path = out_dir / obj_name
        hdr = (
            f"# ppdmp={ppdmp_path.name} game_id={header.game_id} version={header.version}\n"
            f"# draw_export_index={exported} vertices_cmd_index={d.vcmd_index} indices_cmd_index={d.icmd_index} registers_cmd_index={d.registers_cmd_index}\n"
            f"# prim={d.prim} count={d.count} vtype=0x{d.vtype_arg:06X} stride={stride} vert_count={len(compact_verts)} faces={face_count}\n"
            f"# decoded pos_fmt={vt.pos_fmt} normal_fmt={vt.normal_fmt} tex_fmt={vt.tex_fmt} color_fmt={vt.color_fmt} weight_fmt={vt.weight_fmt} idx_fmt={vt.index_fmt}\n"
        )
        export_obj_mesh(obj_path, compact_verts, compact_faces, header=hdr)

        report["exported_draws"].append(
            {
                "export_index": exported,
                "vertices_cmd_index": d.vcmd_index,
                "registers_cmd_index": d.registers_cmd_index,
                "prim": d.prim,
                "count": d.count,
                "vtype": d.vtype_arg,
                "stride": stride,
                "vert_count": len(compact_verts),
                "faces": face_count,
                "vertices_ptr": vcmd.ptr,
                "vertices_sz": vcmd.sz,
                "indices_ptr": (d.icmd.ptr if d.icmd else None),
                "indices_sz": (d.icmd.sz if d.icmd else None),
                "obj": str(obj_path),
            }
        )

        exported += 1

    (out_dir / "ppdmp_record_extract_report.json").write_text(
        json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    print(f"Wrote {exported} OBJ files to: {out_dir}")
    print(f"Wrote report: {out_dir / 'ppdmp_record_extract_report.json'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())


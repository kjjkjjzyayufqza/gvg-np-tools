use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

fn ru32(d: &[u8], o: usize) -> u32 {
    if o + 4 > d.len() {
        return 0;
    }
    u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
}
fn ru16(d: &[u8], o: usize) -> u16 {
    if o + 2 > d.len() {
        return 0;
    }
    u16::from_le_bytes([d[o], d[o + 1]])
}
fn ri16(d: &[u8], o: usize) -> i16 {
    if o + 2 > d.len() {
        return 0;
    }
    i16::from_le_bytes([d[o], d[o + 1]])
}
fn ri8(d: &[u8], o: usize) -> i8 {
    if o >= d.len() {
        return 0;
    }
    d[o] as i8
}
fn ru8(d: &[u8], o: usize) -> u8 {
    if o >= d.len() {
        return 0;
    }
    d[o]
}
fn rf32(d: &[u8], o: usize) -> f32 {
    if o + 4 > d.len() {
        return 0.0;
    }
    f32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
}
fn cstr(d: &[u8], o: usize, mx: usize) -> String {
    let end = (o + mx).min(d.len());
    let raw = &d[o..end];
    let p = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..p]).to_string()
}

const GE_CMD_VADDR: u8 = 0x01;
const GE_CMD_IADDR: u8 = 0x02;
const GE_CMD_PRIM: u8 = 0x04;
const GE_CMD_BASE: u8 = 0x10;
const GE_CMD_VERTEXTYPE: u8 = 0x12;
const GE_CMD_ORIGIN: u8 = 0x14;
const GE_CMD_END: u8 = 0x0C;
const GE_CMD_FINISH: u8 = 0x0F;
const GE_CMD_RET: u8 = 0x0B;

const PRIM_TRIANGLES: u8 = 3;
const PRIM_TRIANGLE_STRIP: u8 = 4;
const PRIM_TRIANGLE_FAN: u8 = 5;

const SECTION_MESH_FLAG_OFFSET: usize = 0x70;
const SECTION_MATERIAL_INDEX_OFFSET: usize = 0x74;

const SECTION_RENDER_MASKS: [u16; 57] = [
    0x0002, // 00: root/control, traverse only
    0x0003, 0x0003, 0x0003, 0x0003, 0x0003, 0x0003, 0x0003, 0x0003, 0x0003, 0x0003, 0x0003, 0x0003,
    0x0003, // 01..13: draw + traverse
    0x0002, 0x0002, 0x0002, 0x0002, // 14..17: traverse only
    0x0003, // 18: draw + traverse
    0x0002, 0x0002, 0x0002, // 19..21: traverse only
    0x0003, // 22: draw + traverse
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, // 23..31
    0x0003, 0x0003, 0x0003, 0x0003, 0x0003, 0x0003, 0x0003, 0x0003, 0x0003, 0x0003,
    0x0003, // 32..42: draw + traverse
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, // 43..56
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SectionRenderPolicy {
    pub flags: Option<u16>,
    pub draws: bool,
    pub traverses: bool,
    pub safe_mesh_target: bool,
    pub label: &'static str,
}

pub fn section_render_policy(index: usize) -> SectionRenderPolicy {
    let Some(flags) = SECTION_RENDER_MASKS.get(index).copied() else {
        return SectionRenderPolicy {
            flags: None,
            draws: false,
            traverses: false,
            safe_mesh_target: false,
            label: "unknown",
        };
    };
    let draws = flags & 1 != 0;
    let traverses = flags & 2 != 0;
    let label = match (draws, traverses) {
        (true, true) => "draw + traverse",
        (true, false) => "draw only",
        (false, true) => "traverse only",
        (false, false) => "not main-rendered",
    };
    SectionRenderPolicy {
        flags: Some(flags),
        draws,
        traverses,
        safe_mesh_target: draws,
        label,
    }
}

fn is_probable_control_root_section(section: &BoneSection) -> bool {
    if section.parent >= 0 {
        return false;
    }
    let name = section.name.to_ascii_lowercase();
    name == "m00" || name.ends_with("_m00")
}

#[derive(Clone, Debug, Default)]
pub struct VtypeInfo {
    pub raw: u32,
    pub tc_fmt: u8,
    pub col_fmt: u8,
    pub nrm_fmt: u8,
    pub pos_fmt: u8,
    pub wt_fmt: u8,
    pub idx_fmt: u8,
    pub wt_count: u8,
    pub through: bool,
}

impl VtypeInfo {
    pub fn decode(vtype: u32) -> Self {
        let wt_fmt = ((vtype >> 9) & 3) as u8;
        Self {
            raw: vtype,
            tc_fmt: (vtype & 3) as u8,
            col_fmt: ((vtype >> 2) & 7) as u8,
            nrm_fmt: ((vtype >> 5) & 3) as u8,
            pos_fmt: ((vtype >> 7) & 3) as u8,
            wt_fmt,
            idx_fmt: ((vtype >> 11) & 3) as u8,
            wt_count: if wt_fmt > 0 {
                (((vtype >> 14) & 7) + 1) as u8
            } else {
                0
            },
            through: vtype & (1 << 23) != 0,
        }
    }

    pub fn vertex_size(&self) -> usize {
        let cs = |f: u8| -> usize { [0, 1, 2, 4][f as usize & 3] };
        let align = |v: usize, a: usize| -> usize {
            if a <= 1 {
                v
            } else {
                (v + a - 1) & !(a - 1)
            }
        };
        let mut sz = 0usize;
        if self.wt_fmt > 0 {
            let w = cs(self.wt_fmt);
            sz = align(sz, w) + w * self.wt_count as usize;
        }
        if self.tc_fmt > 0 {
            let a = cs(self.tc_fmt);
            sz = align(sz, a) + a * 2;
        }
        if self.col_fmt > 0 {
            let cb = match self.col_fmt {
                4 => 2,
                5 => 2,
                6 => 2,
                7 => 4,
                _ => 0,
            };
            if cb > 0 {
                sz = align(sz, cb) + cb;
            }
        }
        if self.nrm_fmt > 0 {
            let a = cs(self.nrm_fmt);
            sz = align(sz, a) + a * 3;
        }
        if self.pos_fmt > 0 {
            let a = cs(self.pos_fmt);
            sz = align(sz, a) + a * 3;
        }
        align(sz, cs(self.pos_fmt).max(1))
    }
}

#[derive(Clone, Debug, Default)]
pub struct ParsedVertex {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub u: f32,
    pub v: f32,
    pub nx: f32,
    pub ny: f32,
    pub nz: f32,
}

fn decode_vertex(data: &[u8], offset: usize, vt: &VtypeInfo) -> Option<ParsedVertex> {
    let vs = vt.vertex_size();
    if offset + vs > data.len() {
        return None;
    }
    let mut pv = ParsedVertex::default();
    let cs = |f: u8| -> usize { [0, 1, 2, 4][f as usize & 3] };
    let align = |v: usize, a: usize| -> usize {
        if a <= 1 {
            v
        } else {
            (v + a - 1) & !(a - 1)
        }
    };
    let mut o = offset;

    if vt.wt_fmt > 0 {
        let w = cs(vt.wt_fmt);
        o = align(o, w) + w * vt.wt_count as usize;
    }
    if vt.tc_fmt > 0 {
        let tc_sz = cs(vt.tc_fmt);
        o = align(o, tc_sz);
        match vt.tc_fmt {
            1 => {
                pv.u = ru8(data, o) as f32 / 128.0;
                pv.v = ru8(data, o + 1) as f32 / 128.0;
            }
            2 => {
                pv.u = ri16(data, o) as f32 / 32768.0;
                pv.v = ri16(data, o + 2) as f32 / 32768.0;
            }
            3 => {
                pv.u = rf32(data, o);
                pv.v = rf32(data, o + 4);
            }
            _ => {}
        }
        o += tc_sz * 2;
    }
    if vt.col_fmt > 0 {
        let cb = match vt.col_fmt {
            4 => 2,
            5 => 2,
            6 => 2,
            7 => 4,
            _ => 0,
        };
        if cb > 0 {
            o = align(o, cb) + cb;
        }
    }
    if vt.nrm_fmt > 0 {
        let ns = cs(vt.nrm_fmt);
        o = align(o, ns);
        match vt.nrm_fmt {
            1 => {
                pv.nx = ri8(data, o) as f32 / 127.0;
                pv.ny = ri8(data, o + 1) as f32 / 127.0;
                pv.nz = ri8(data, o + 2) as f32 / 127.0;
            }
            2 => {
                pv.nx = ri16(data, o) as f32 / 32767.0;
                pv.ny = ri16(data, o + 2) as f32 / 32767.0;
                pv.nz = ri16(data, o + 4) as f32 / 32767.0;
            }
            3 => {
                pv.nx = rf32(data, o);
                pv.ny = rf32(data, o + 4);
                pv.nz = rf32(data, o + 8);
            }
            _ => {}
        }
        o += ns * 3;
    }
    if vt.pos_fmt > 0 {
        let ps = cs(vt.pos_fmt);
        o = align(o, ps);
        match vt.pos_fmt {
            1 => {
                pv.x = ri8(data, o) as f32 / 127.0;
                pv.y = ri8(data, o + 1) as f32 / 127.0;
                pv.z = ri8(data, o + 2) as f32 / 127.0;
            }
            2 => {
                pv.x = ri16(data, o) as f32;
                pv.y = ri16(data, o + 2) as f32;
                pv.z = ri16(data, o + 4) as f32;
            }
            3 => {
                pv.x = rf32(data, o);
                pv.y = rf32(data, o + 4);
                pv.z = rf32(data, o + 8);
            }
            _ => {}
        }
    }
    Some(pv)
}

fn vertex_position_field_offset(offset: usize, vt: &VtypeInfo) -> Option<(usize, usize)> {
    let cs = |f: u8| -> usize { [0, 1, 2, 4][f as usize & 3] };
    let align = |v: usize, a: usize| -> usize {
        if a <= 1 {
            v
        } else {
            (v + a - 1) & !(a - 1)
        }
    };
    let mut o = offset;

    if vt.wt_fmt > 0 {
        let w = cs(vt.wt_fmt);
        o = align(o, w) + w * vt.wt_count as usize;
    }
    if vt.tc_fmt > 0 {
        let tc_sz = cs(vt.tc_fmt);
        o = align(o, tc_sz) + tc_sz * 2;
    }
    if vt.col_fmt > 0 {
        let cb = match vt.col_fmt {
            4 => 2,
            5 => 2,
            6 => 2,
            7 => 4,
            _ => 0,
        };
        if cb > 0 {
            o = align(o, cb) + cb;
        }
    }
    if vt.nrm_fmt > 0 {
        let ns = cs(vt.nrm_fmt);
        o = align(o, ns) + ns * 3;
    }
    if vt.pos_fmt == 0 {
        return None;
    }
    let ps = cs(vt.pos_fmt);
    Some((align(o, ps), ps))
}

fn rewrite_vertex_position_for_bbox(
    data: &mut [u8],
    vertex_offset: usize,
    vt: &VtypeInfo,
    old_bbox: &[f32; 3],
    new_bbox: &[f32; 3],
) -> Option<()> {
    let (pos_off, pos_size) = vertex_position_field_offset(vertex_offset, vt)?;
    if pos_off + pos_size * 3 > data.len() {
        return None;
    }
    for axis in 0..3 {
        let old_scale = old_bbox[axis] / 32768.0;
        let new_scale = new_bbox[axis] / 32768.0;
        if new_scale <= 0.0 {
            return None;
        }
        let off = pos_off + axis * pos_size;
        match vt.pos_fmt {
            1 => {
                let value = ri8(data, off) as f32 / 127.0 * old_scale;
                let raw = (value / new_scale * 127.0).round().clamp(-128.0, 127.0) as i8;
                data[off] = raw as u8;
            }
            2 => {
                let value = ri16(data, off) as f32 * old_scale;
                let raw = clamp_i16((value / new_scale).round() as i32);
                data[off..off + 2].copy_from_slice(&raw.to_le_bytes());
            }
            3 => {
                let value = rf32(data, off) * old_scale;
                let raw = value / new_scale;
                data[off..off + 4].copy_from_slice(&raw.to_le_bytes());
            }
            _ => return None,
        }
    }
    Some(())
}

#[derive(Clone, Debug)]
struct GeCmd {
    offset: usize,
    cmd: u8,
    param: u32,
}

fn scan_ge_display_list(data: &[u8], start: usize, end: usize) -> Vec<GeCmd> {
    let mut cmds = Vec::new();
    let mut off = start;
    while off + 4 <= end {
        let word = ru32(data, off);
        let cmd = ((word >> 24) & 0xFF) as u8;
        let param = word & 0xFFFFFF;
        cmds.push(GeCmd {
            offset: off,
            cmd,
            param,
        });
        off += 4;
        if cmd == GE_CMD_END || cmd == GE_CMD_FINISH || cmd == GE_CMD_RET {
            break;
        }
    }
    cmds
}

#[derive(Clone, Debug)]
struct DrawCall {
    prim_type: u8,
    vertex_count: usize,
    vtype: VtypeInfo,
    vaddr: usize,
    iaddr: usize,
}

fn extract_draw_calls(cmds: &[GeCmd], origin_file_offset: usize) -> Vec<DrawCall> {
    let mut calls = Vec::new();
    let mut cur_vtype = VtypeInfo::default();
    let mut cur_vaddr: u32 = 0;
    let mut cur_iaddr: u32 = 0;
    let mut idx_advance: u32 = 0;

    for c in cmds {
        match c.cmd {
            GE_CMD_BASE => {}
            GE_CMD_VERTEXTYPE => {
                cur_vtype = VtypeInfo::decode(c.param);
                idx_advance = 0;
            }
            GE_CMD_VADDR => {
                cur_vaddr = c.param;
            }
            GE_CMD_IADDR => {
                cur_iaddr = c.param;
                idx_advance = 0;
            }
            GE_CMD_PRIM => {
                let prim_type = ((c.param >> 16) & 7) as u8;
                let vert_count = (c.param & 0xFFFF) as usize;
                if vert_count > 0 && cur_vtype.pos_fmt > 0 {
                    let file_vaddr = origin_file_offset + cur_vaddr as usize;
                    let file_iaddr = origin_file_offset + cur_iaddr as usize + idx_advance as usize;
                    calls.push(DrawCall {
                        prim_type,
                        vertex_count: vert_count,
                        vtype: VtypeInfo::decode(cur_vtype.raw),
                        vaddr: file_vaddr,
                        iaddr: file_iaddr,
                    });
                    if cur_vtype.idx_fmt > 0 {
                        let idx_size = [0, 1, 2, 4][cur_vtype.idx_fmt as usize & 3];
                        idx_advance += (vert_count * idx_size) as u32;
                    } else {
                        cur_vaddr += (cur_vtype.vertex_size() * vert_count) as u32;
                    }
                }
            }
            _ => {}
        }
    }
    calls
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BoneSection {
    pub index: usize,
    pub name: String,
    pub offset: usize,
    pub size: usize,
    pub local_matrix: Vec<f32>,
    pub parent: i32,
    pub has_mesh: bool,
    pub origin_offset: Option<usize>,
    pub category: String,
}

pub fn parse_pmf2_sections(data: &[u8]) -> (Vec<BoneSection>, [f32; 3]) {
    if data.len() < 0x20 || &data[..4] != b"PMF2" {
        return (vec![], [1.0, 1.0, 1.0]);
    }
    let num_sec = ru32(data, 4) as usize;
    let bbox = [rf32(data, 0x10), rf32(data, 0x14), rf32(data, 0x18)];
    let offsets: Vec<usize> = (0..num_sec)
        .map(|i| ru32(data, 0x20 + i * 4) as usize)
        .collect();

    let mut sections = Vec::new();
    for si in 0..num_sec {
        let so = offsets[si];
        let se = if si + 1 < num_sec {
            offsets[si + 1]
        } else {
            data.len()
        };
        let mat: Vec<f32> = (0..16).map(|j| rf32(data, so + j * 4)).collect();
        let name = cstr(data, so + 0x60, 16);
        let parent_raw = ru32(data, so + 0x7C);
        let parent = if (parent_raw as usize) < num_sec {
            parent_raw as i32
        } else {
            -1
        };

        let cat = if name.contains("_m") {
            "body"
        } else if name.contains("_o") {
            "ornament"
        } else if name.contains("_w") {
            "weapon"
        } else if name.contains("_z") {
            "effect"
        } else {
            ""
        };

        let mut origin = None;
        let scan_end = se.min(so + 0x200);
        let mut off = so + 0x100;
        while off + 4 <= scan_end {
            if ru32(data, off) == 0x14000000 {
                origin = Some(off);
                break;
            }
            off += 4;
        }

        sections.push(BoneSection {
            index: si,
            name,
            offset: so,
            size: se - so,
            local_matrix: mat,
            parent,
            has_mesh: origin.is_some(),
            origin_offset: origin,
            category: cat.to_string(),
        });
    }
    (sections, bbox)
}

fn mat4_mul(a: &[f32], b: &[f32]) -> Vec<f32> {
    let mut r = vec![0.0f32; 16];
    for i in 0..4 {
        for j in 0..4 {
            r[i * 4 + j] = (0..4).map(|k| a[i * 4 + k] * b[k * 4 + j]).sum();
        }
    }
    r
}

fn transform_pt(m: &[f32], x: f32, y: f32, z: f32) -> (f32, f32, f32) {
    (
        x * m[0] + y * m[4] + z * m[8] + m[12],
        x * m[1] + y * m[5] + z * m[9] + m[13],
        x * m[2] + y * m[6] + z * m[10] + m[14],
    )
}

fn transform_dir(m: &[f32], x: f32, y: f32, z: f32) -> (f32, f32, f32) {
    (
        x * m[0] + y * m[4] + z * m[8],
        x * m[1] + y * m[5] + z * m[9],
        x * m[2] + y * m[6] + z * m[10],
    )
}

pub fn compute_world_matrices(sections: &[BoneSection]) -> HashMap<usize, Vec<f32>> {
    let mut world: HashMap<usize, Vec<f32>> = HashMap::new();
    fn compute(
        idx: usize,
        sections: &[BoneSection],
        world: &mut HashMap<usize, Vec<f32>>,
    ) -> Vec<f32> {
        if let Some(w) = world.get(&idx) {
            return w.clone();
        }
        let s = &sections[idx];
        if s.parent < 0 {
            let m = s.local_matrix.clone();
            world.insert(idx, m.clone());
            return m;
        }
        let pw = compute(s.parent as usize, sections, world);
        let m = mat4_mul(&s.local_matrix, &pw);
        world.insert(idx, m.clone());
        m
    }
    for i in 0..sections.len() {
        compute(i, sections, &mut world);
    }
    world
}

fn strip_to_triangles(verts: &[ParsedVertex]) -> Vec<(usize, usize, usize)> {
    let mut faces = Vec::new();
    let mut flip = false;
    for i in 0..verts.len().saturating_sub(2) {
        let (va, vb, vc) = (&verts[i], &verts[i + 1], &verts[i + 2]);
        let degen = (va.x == vb.x && va.y == vb.y && va.z == vb.z)
            || (vb.x == vc.x && vb.y == vc.y && vb.z == vc.z)
            || (va.x == vc.x && va.y == vc.y && va.z == vc.z);
        if degen {
            flip = false;
            continue;
        }
        if flip {
            faces.push((i + 1, i, i + 2));
        } else {
            faces.push((i, i + 1, i + 2));
        }
        flip = !flip;
    }
    faces
}

fn fan_to_triangles(verts: &[ParsedVertex]) -> Vec<(usize, usize, usize)> {
    (1..verts.len().saturating_sub(1))
        .map(|i| (0, i, i + 1))
        .collect()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BoneMeshMeta {
    pub bone_index: usize,
    pub bone_name: String,
    pub vertex_count: usize,
    pub face_count: usize,
    pub has_uv: bool,
    pub has_normals: bool,
    pub draw_call_vtypes: Vec<u32>,
    pub local_vertices: Vec<[f32; 8]>,
    pub faces: Vec<[usize; 3]>,
}

pub struct BoneMeshData {
    pub bone_index: usize,
    pub bone_name: String,
    pub vertices: Vec<ParsedVertex>,
    pub faces: Vec<(usize, usize, usize)>,
    pub local_vertices: Vec<ParsedVertex>,
    pub has_uv: bool,
    pub has_normals: bool,
    pub vtypes: Vec<u32>,
}

pub type ExtractedPmf2Meshes = (
    Vec<BoneMeshData>,
    Vec<BoneSection>,
    [f32; 3],
    HashMap<usize, Vec<f32>>,
);

pub fn extract_per_bone_meshes(pmf2_data: &[u8], swap_yz: bool) -> ExtractedPmf2Meshes {
    let (sections, bbox) = parse_pmf2_sections(pmf2_data);
    if sections.is_empty() {
        return (vec![], vec![], [1.0, 1.0, 1.0], HashMap::new());
    }

    let world_mats = compute_world_matrices(&sections);
    let sx = bbox[0] / 32768.0;
    let sy = bbox[1] / 32768.0;
    let sz = bbox[2] / 32768.0;
    let mut bone_meshes = Vec::new();

    for sec in &sections {
        let origin = match sec.origin_offset {
            Some(o) => o,
            None => continue,
        };
        let scan_end = (origin + 0x800).min(pmf2_data.len());
        let cmds = scan_ge_display_list(pmf2_data, origin, scan_end);
        let mut origin_off = origin;
        for c in &cmds {
            if c.cmd == GE_CMD_ORIGIN {
                origin_off = c.offset;
                break;
            }
        }
        let calls = extract_draw_calls(&cmds, origin_off);
        if calls.is_empty() {
            continue;
        }

        let wm = &world_mats[&sec.index];
        let mut part_verts = Vec::new();
        let mut local_verts = Vec::new();
        let mut part_faces: Vec<(usize, usize, usize)> = Vec::new();
        let mut has_uv = false;
        let mut has_nrm = false;
        let mut vtypes_used = Vec::new();

        for dc in &calls {
            let vt = &dc.vtype;
            let vs = vt.vertex_size();
            if vs == 0 || vt.pos_fmt == 0 {
                continue;
            }
            let count = dc.vertex_count;
            if dc.vaddr + vs * count > pmf2_data.len() {
                continue;
            }

            let raw_verts: Vec<ParsedVertex>;
            if vt.idx_fmt > 0 && dc.iaddr > 0 {
                let idx_size = [0, 1, 2, 4][vt.idx_fmt as usize & 3];
                let mut indices = Vec::new();
                for ii in 0..count {
                    let ioff = dc.iaddr + ii * idx_size;
                    if ioff + idx_size > pmf2_data.len() {
                        break;
                    }
                    let idx = match idx_size {
                        1 => ru8(pmf2_data, ioff) as usize,
                        2 => ru16(pmf2_data, ioff) as usize,
                        _ => ru32(pmf2_data, ioff) as usize,
                    };
                    indices.push(idx);
                }
                if indices.len() < 3 {
                    continue;
                }
                let max_idx = *indices.iter().max().unwrap();
                if dc.vaddr + (max_idx + 1) * vs > pmf2_data.len() {
                    continue;
                }

                let mut cache: HashMap<usize, ParsedVertex> = HashMap::new();
                let mut rv = Vec::new();
                let mut valid = true;
                for &idx_val in &indices {
                    if let std::collections::hash_map::Entry::Vacant(entry) = cache.entry(idx_val) {
                        match decode_vertex(pmf2_data, dc.vaddr + idx_val * vs, vt) {
                            Some(mut pv) => {
                                pv.x *= sx;
                                pv.y *= sy;
                                pv.z *= sz;
                                entry.insert(pv);
                            }
                            None => {
                                valid = false;
                                break;
                            }
                        }
                    }
                    rv.push(cache[&idx_val].clone());
                }
                if !valid || rv.len() < 3 {
                    continue;
                }
                raw_verts = rv;
            } else {
                let mut rv = Vec::new();
                let mut valid = true;
                for vi in 0..count {
                    match decode_vertex(pmf2_data, dc.vaddr + vi * vs, vt) {
                        Some(mut pv) => {
                            pv.x *= sx;
                            pv.y *= sy;
                            pv.z *= sz;
                            rv.push(pv);
                        }
                        None => {
                            valid = false;
                            break;
                        }
                    }
                }
                if !valid || rv.len() < 3 {
                    continue;
                }
                raw_verts = rv;
            }

            if vt.tc_fmt > 0 {
                has_uv = true;
            }
            if vt.nrm_fmt > 0 {
                has_nrm = true;
            }
            vtypes_used.push(vt.raw);

            let base_idx = part_verts.len();
            for pv in &raw_verts {
                local_verts.push(pv.clone());
                let (tx, ty, tz) = transform_pt(wm, pv.x, pv.y, pv.z);
                let (tnx, tny, tnz) = if has_nrm {
                    transform_dir(wm, pv.nx, pv.ny, pv.nz)
                } else {
                    (0.0, 0.0, 0.0)
                };
                let mut tv = ParsedVertex {
                    x: tx,
                    y: ty,
                    z: tz,
                    u: pv.u,
                    v: pv.v,
                    nx: tnx,
                    ny: tny,
                    nz: tnz,
                };
                if swap_yz {
                    let (ox, oy, oz) = (tv.x, tv.y, tv.z);
                    tv.x = ox;
                    tv.y = oz;
                    tv.z = -oy;
                    let (onx, ony, onz) = (tv.nx, tv.ny, tv.nz);
                    tv.nx = onx;
                    tv.ny = onz;
                    tv.nz = -ony;
                }
                part_verts.push(tv);
            }

            let local_faces = match dc.prim_type {
                PRIM_TRIANGLE_STRIP => strip_to_triangles(&raw_verts),
                PRIM_TRIANGLE_FAN => fan_to_triangles(&raw_verts),
                PRIM_TRIANGLES => (0..raw_verts.len() / 3)
                    .map(|i| (i * 3, i * 3 + 1, i * 3 + 2))
                    .collect(),
                _ => continue,
            };
            for (a, b, c) in local_faces {
                part_faces.push((base_idx + a, base_idx + b, base_idx + c));
            }
        }

        if !part_verts.is_empty() && !part_faces.is_empty() {
            bone_meshes.push(BoneMeshData {
                bone_index: sec.index,
                bone_name: sec.name.clone(),
                vertices: part_verts,
                faces: part_faces,
                local_vertices: local_verts,
                has_uv,
                has_normals: has_nrm,
                vtypes: vtypes_used,
            });
        }
    }
    (bone_meshes, sections, bbox, world_mats)
}

#[derive(Serialize, Deserialize)]
pub struct Pmf2Meta {
    pub model_name: String,
    pub bbox: [f32; 3],
    pub section_count: usize,
    pub sections: Vec<BoneSection>,
    pub bone_meshes: Vec<BoneMeshMeta>,
}

#[derive(Clone, Debug)]
pub struct Pmf2SectionMetadataEdit {
    pub name: String,
    pub parent: i32,
    pub local_matrix: Vec<f32>,
}

#[derive(Clone, Debug)]
pub struct Pmf2MetadataEdit {
    pub bbox: [f32; 3],
    pub sections: Vec<Pmf2SectionMetadataEdit>,
}

impl Pmf2MetadataEdit {
    pub fn from_pmf2(data: &[u8]) -> Result<Self> {
        if data.len() < 0x20 || &data[..4] != b"PMF2" {
            bail!("not a PMF2 stream");
        }
        let (sections, bbox) = parse_pmf2_sections(data);
        if sections.is_empty() {
            bail!("PMF2 contains no editable sections");
        }
        Ok(Self {
            bbox,
            sections: sections
                .into_iter()
                .map(|section| Pmf2SectionMetadataEdit {
                    name: section.name,
                    parent: section.parent,
                    local_matrix: section.local_matrix,
                })
                .collect(),
        })
    }
}

const I16_POSITION_SATURATION_RATIO: f32 = 32767.0 / 32768.0;

pub fn compute_auto_bbox_from_bone_meshes(bone_meshes: &[BoneMeshMeta]) -> Option<[f32; 3]> {
    if bone_meshes.is_empty() {
        return None;
    }
    let mut bbox = [0.0f32; 3];
    let mut has_vertex = false;
    for bm in bone_meshes {
        for lv in &bm.local_vertices {
            has_vertex = true;
            bbox[0] = bbox[0].max(lv[0].abs());
            bbox[1] = bbox[1].max(lv[1].abs());
            bbox[2] = bbox[2].max(lv[2].abs());
        }
    }
    if !has_vertex {
        return None;
    }
    for axis in &mut bbox {
        if *axis < 1e-6 {
            *axis = 1.0;
        } else {
            *axis /= I16_POSITION_SATURATION_RATIO;
        }
    }
    Some(bbox)
}

pub fn build_meta(
    model_name: &str,
    sections: &[BoneSection],
    bbox: [f32; 3],
    bone_meshes: &[BoneMeshData],
) -> Pmf2Meta {
    let bm_meta: Vec<BoneMeshMeta> = bone_meshes
        .iter()
        .map(|bm| {
            let lvs: Vec<[f32; 8]> = bm
                .local_vertices
                .iter()
                .map(|lv| [lv.x, lv.y, lv.z, lv.u, lv.v, lv.nx, lv.ny, lv.nz])
                .collect();
            let fs: Vec<[usize; 3]> = bm.faces.iter().map(|&(a, b, c)| [a, b, c]).collect();
            BoneMeshMeta {
                bone_index: bm.bone_index,
                bone_name: bm.bone_name.clone(),
                vertex_count: bm.vertices.len(),
                face_count: bm.faces.len(),
                has_uv: bm.has_uv,
                has_normals: bm.has_normals,
                draw_call_vtypes: bm.vtypes.clone(),
                local_vertices: lvs,
                faces: fs,
            }
        })
        .collect();
    Pmf2Meta {
        model_name: model_name.to_string(),
        bbox,
        section_count: sections.len(),
        sections: sections.to_vec(),
        bone_meshes: bm_meta,
    }
}

fn clamp_i16(val: i32) -> i16 {
    val.clamp(-32768, 32767) as i16
}

fn wrap_texture_coord(value: f32) -> f32 {
    if value < 0.0 || value > 1.0 {
        value.rem_euclid(1.0)
    } else {
        value
    }
}

type EncodedVertex = (i16, i16, i16, i16, i16, i16, i16, i16);
const MAX_TRIANGLE_PRIM_VERTICES: usize = 0xFFFC;
const APPENDED_TRIANGLE_PRIM_VERTICES: usize = 96;

fn triangle_prim_chunks(vertex_count: usize) -> Vec<(usize, usize)> {
    triangle_prim_chunks_with_limit(vertex_count, MAX_TRIANGLE_PRIM_VERTICES)
}

fn triangle_prim_chunks_with_limit(
    vertex_count: usize,
    max_vertices: usize,
) -> Vec<(usize, usize)> {
    let mut chunks = Vec::new();
    let max_vertices = max_vertices.min(MAX_TRIANGLE_PRIM_VERTICES);
    let max_vertices = max_vertices - max_vertices % 3;
    if max_vertices == 0 {
        return chunks;
    }

    let mut start = 0usize;
    while start < vertex_count {
        let remaining = vertex_count - start;
        let mut count = remaining.min(max_vertices);
        if remaining > max_vertices {
            count -= count % 3;
        }
        if count == 0 {
            break;
        }
        chunks.push((start, count));
        start += count;
    }
    chunks
}

fn build_ge_commands(mesh: &BoneMeshMeta, bbox: &[f32; 3]) -> Vec<u8> {
    let sx = bbox[0] / 32768.0;
    let sy = bbox[1] / 32768.0;
    let sz = bbox[2] / 32768.0;

    let verts_i16: Vec<EncodedVertex> = mesh
        .local_vertices
        .iter()
        .map(|lv| {
            let px = if sx > 0.0 {
                clamp_i16((lv[0] / sx).round() as i32)
            } else {
                0
            };
            let py = if sy > 0.0 {
                clamp_i16((lv[1] / sy).round() as i32)
            } else {
                0
            };
            let pz = if sz > 0.0 {
                clamp_i16((lv[2] / sz).round() as i32)
            } else {
                0
            };
            let tu = clamp_i16((wrap_texture_coord(lv[3]) * 32768.0).round() as i32);
            let tv = clamp_i16((wrap_texture_coord(lv[4]) * 32768.0).round() as i32);
            let nnx = clamp_i16((lv[5] * 32767.0).round() as i32);
            let nny = clamp_i16((lv[6] * 32767.0).round() as i32);
            let nnz = clamp_i16((lv[7] * 32767.0).round() as i32);
            (tu, tv, nnx, nny, nnz, px, py, pz)
        })
        .collect();

    let mut seq_verts = Vec::new();
    for f in &mesh.faces {
        for &idx in &[f[0], f[1], f[2]] {
            if idx < verts_i16.len() {
                seq_verts.push(verts_i16[idx]);
            }
        }
    }
    if seq_verts.is_empty() {
        return Vec::new();
    }

    let mut vtype: u32 = 0;
    if mesh.has_uv {
        vtype |= 2;
    }
    if mesh.has_normals {
        vtype |= 2 << 5;
    }
    vtype |= 2 << 7;

    let chunks = triangle_prim_chunks(seq_verts.len());
    if chunks.is_empty() {
        return Vec::new();
    }
    let num_cmds = 2 + chunks.len() * 4 + 1;
    let cmd_block_size = num_cmds * 4;

    let mut vert_buf = Vec::new();
    for vtx in &seq_verts {
        if mesh.has_uv {
            vert_buf.extend_from_slice(&vtx.0.to_le_bytes());
            vert_buf.extend_from_slice(&vtx.1.to_le_bytes());
        }
        if mesh.has_normals {
            vert_buf.extend_from_slice(&vtx.2.to_le_bytes());
            vert_buf.extend_from_slice(&vtx.3.to_le_bytes());
            vert_buf.extend_from_slice(&vtx.4.to_le_bytes());
        }
        vert_buf.extend_from_slice(&vtx.5.to_le_bytes());
        vert_buf.extend_from_slice(&vtx.6.to_le_bytes());
        vert_buf.extend_from_slice(&vtx.7.to_le_bytes());
    }
    while vert_buf.len() % 4 != 0 {
        vert_buf.push(0);
    }

    let mut ge = Vec::new();
    let push_cmd = |buf: &mut Vec<u8>, cmd: u8, param: u32| {
        let word = ((cmd as u32) << 24) | (param & 0xFFFFFF);
        buf.extend_from_slice(&word.to_le_bytes());
    };
    push_cmd(&mut ge, 0x14, 0);
    push_cmd(&mut ge, 0x10, 0);
    let vertex_size = VtypeInfo::decode(vtype).vertex_size();
    for (start, count) in &chunks {
        let vaddr = cmd_block_size + start * vertex_size;
        push_cmd(&mut ge, 0x01, vaddr as u32);
        push_cmd(&mut ge, 0x12, vtype);
        push_cmd(&mut ge, 0x9B, 1);
        push_cmd(&mut ge, 0x04, (PRIM_TRIANGLES as u32) << 16 | *count as u32);
    }
    push_cmd(&mut ge, 0x0B, 0);

    while ge.len() < cmd_block_size {
        ge.extend_from_slice(&[0, 0, 0, 0]);
    }
    ge.extend_from_slice(&vert_buf);
    ge
}

fn build_mesh_suffix(mesh: &BoneMeshMeta, face_start: usize) -> Option<BoneMeshMeta> {
    if face_start >= mesh.faces.len() {
        return None;
    }

    let mut remap: HashMap<usize, usize> = HashMap::new();
    let mut local_vertices = Vec::new();
    let mut faces = Vec::new();
    for face in &mesh.faces[face_start..] {
        let mut out_face = [0usize; 3];
        for (dst, &src_idx) in out_face.iter_mut().zip(face.iter()) {
            if src_idx >= mesh.local_vertices.len() {
                return None;
            }
            let next_idx = remap.len();
            let mapped = *remap.entry(src_idx).or_insert_with(|| {
                local_vertices.push(mesh.local_vertices[src_idx]);
                next_idx
            });
            *dst = mapped;
        }
        faces.push(out_face);
    }

    Some(BoneMeshMeta {
        bone_index: mesh.bone_index,
        bone_name: mesh.bone_name.clone(),
        vertex_count: local_vertices.len(),
        face_count: faces.len(),
        has_uv: mesh.has_uv,
        has_normals: mesh.has_normals,
        draw_call_vtypes: Vec::new(),
        local_vertices,
        faces,
    })
}

type FaceSignature = [(i32, i32, i32); 3];

fn quantize_face_position(v: &[f32; 8]) -> (i32, i32, i32) {
    const SCALE: f32 = 1000.0;
    (
        (v[0] * SCALE).round() as i32,
        (v[1] * SCALE).round() as i32,
        (v[2] * SCALE).round() as i32,
    )
}

fn quantize_parsed_position(v: &ParsedVertex) -> (i32, i32, i32) {
    const SCALE: f32 = 1000.0;
    (
        (v.x * SCALE).round() as i32,
        (v.y * SCALE).round() as i32,
        (v.z * SCALE).round() as i32,
    )
}

fn face_signature_from_points(mut points: [(i32, i32, i32); 3]) -> FaceSignature {
    points.sort_unstable();
    points
}

fn collect_world_face_signatures(meshes: &[BoneMeshData]) -> HashSet<FaceSignature> {
    let mut signatures = HashSet::new();
    for mesh in meshes {
        for &(a, b, c) in &mesh.faces {
            if a >= mesh.vertices.len() || b >= mesh.vertices.len() || c >= mesh.vertices.len() {
                continue;
            }
            signatures.insert(face_signature_from_points([
                quantize_parsed_position(&mesh.vertices[a]),
                quantize_parsed_position(&mesh.vertices[b]),
                quantize_parsed_position(&mesh.vertices[c]),
            ]));
        }
    }
    signatures
}

fn mesh_face_world_signature(
    mesh: &BoneMeshMeta,
    face: &[usize; 3],
    world_matrix: &[f32],
) -> Option<FaceSignature> {
    let mut points = [(0, 0, 0); 3];
    for (dst, src_idx) in points.iter_mut().zip(face.iter()) {
        let v = mesh.local_vertices.get(*src_idx)?;
        let (x, y, z) = transform_pt(world_matrix, v[0], v[1], v[2]);
        let world_vertex = [x, y, z, v[3], v[4], v[5], v[6], v[7]];
        *dst = quantize_face_position(&world_vertex);
    }
    Some(face_signature_from_points(points))
}

fn build_mesh_suffix_without_existing_world_faces(
    mesh: &BoneMeshMeta,
    face_start: usize,
    world_matrix: &[f32],
    existing_faces: &HashSet<FaceSignature>,
) -> Option<BoneMeshMeta> {
    if face_start >= mesh.faces.len() {
        return None;
    }

    let mut remap: HashMap<usize, usize> = HashMap::new();
    let mut local_vertices = Vec::new();
    let mut faces = Vec::new();
    for face in &mesh.faces[face_start..] {
        let signature = mesh_face_world_signature(mesh, face, world_matrix)?;
        if existing_faces.contains(&signature) {
            continue;
        }
        let mut out_face = [0usize; 3];
        for (dst, &src_idx) in out_face.iter_mut().zip(face.iter()) {
            if src_idx >= mesh.local_vertices.len() {
                return None;
            }
            let next_idx = remap.len();
            let mapped = *remap.entry(src_idx).or_insert_with(|| {
                local_vertices.push(mesh.local_vertices[src_idx]);
                next_idx
            });
            *dst = mapped;
        }
        faces.push(out_face);
    }

    if faces.is_empty() {
        return None;
    }

    Some(BoneMeshMeta {
        bone_index: mesh.bone_index,
        bone_name: mesh.bone_name.clone(),
        vertex_count: local_vertices.len(),
        face_count: faces.len(),
        has_uv: mesh.has_uv,
        has_normals: mesh.has_normals,
        draw_call_vtypes: Vec::new(),
        local_vertices,
        faces,
    })
}

fn encode_mesh_vertices(mesh: &BoneMeshMeta, bbox: &[f32; 3]) -> Option<(u32, Vec<u8>, usize)> {
    let sx = bbox[0] / 32768.0;
    let sy = bbox[1] / 32768.0;
    let sz = bbox[2] / 32768.0;

    let verts_i16: Vec<EncodedVertex> = mesh
        .local_vertices
        .iter()
        .map(|lv| {
            let px = if sx > 0.0 {
                clamp_i16((lv[0] / sx).round() as i32)
            } else {
                0
            };
            let py = if sy > 0.0 {
                clamp_i16((lv[1] / sy).round() as i32)
            } else {
                0
            };
            let pz = if sz > 0.0 {
                clamp_i16((lv[2] / sz).round() as i32)
            } else {
                0
            };
            let tu = clamp_i16((wrap_texture_coord(lv[3]) * 32768.0).round() as i32);
            let tv = clamp_i16((wrap_texture_coord(lv[4]) * 32768.0).round() as i32);
            let nnx = clamp_i16((lv[5] * 32767.0).round() as i32);
            let nny = clamp_i16((lv[6] * 32767.0).round() as i32);
            let nnz = clamp_i16((lv[7] * 32767.0).round() as i32);
            (tu, tv, nnx, nny, nnz, px, py, pz)
        })
        .collect();

    let mut seq_verts = Vec::new();
    for f in &mesh.faces {
        for &idx in &[f[0], f[1], f[2]] {
            if idx < verts_i16.len() {
                seq_verts.push(verts_i16[idx]);
            }
        }
    }
    if seq_verts.is_empty() {
        return None;
    }

    let mut vtype: u32 = 0;
    if mesh.has_uv {
        vtype |= 2;
    }
    if mesh.has_normals {
        vtype |= 2 << 5;
    }
    vtype |= 2 << 7;

    let mut vert_buf = Vec::new();
    for vtx in &seq_verts {
        if mesh.has_uv {
            vert_buf.extend_from_slice(&vtx.0.to_le_bytes());
            vert_buf.extend_from_slice(&vtx.1.to_le_bytes());
        }
        if mesh.has_normals {
            vert_buf.extend_from_slice(&vtx.2.to_le_bytes());
            vert_buf.extend_from_slice(&vtx.3.to_le_bytes());
            vert_buf.extend_from_slice(&vtx.4.to_le_bytes());
        }
        vert_buf.extend_from_slice(&vtx.5.to_le_bytes());
        vert_buf.extend_from_slice(&vtx.6.to_le_bytes());
        vert_buf.extend_from_slice(&vtx.7.to_le_bytes());
    }
    while vert_buf.len() % 4 != 0 {
        vert_buf.push(0);
    }

    Some((vtype, vert_buf, seq_verts.len()))
}

fn append_mesh_draw_to_template_section(
    template_section: &[u8],
    origin_rel: usize,
    mesh: &BoneMeshMeta,
    bbox: &[f32; 3],
) -> Option<Vec<u8>> {
    let (vtype, vert_buf, vertex_count) = encode_mesh_vertices(mesh, bbox)?;
    if origin_rel + 4 > template_section.len() {
        return None;
    }
    let chunks = triangle_prim_chunks_with_limit(vertex_count, APPENDED_TRIANGLE_PRIM_VERTICES);
    if chunks.is_empty() {
        return None;
    }

    let mut ret_rel = None;
    let mut insert_rel = None;
    let mut off = origin_rel;
    while off + 4 <= template_section.len() {
        let word = ru32(template_section, off);
        let cmd = ((word >> 24) & 0xFF) as u8;
        if cmd == GE_CMD_RET || cmd == GE_CMD_END || cmd == GE_CMD_FINISH {
            ret_rel = Some(off);
            break;
        }
        if cmd == GE_CMD_PRIM {
            insert_rel = Some(off + 4);
        }
        off += 4;
    }
    let ret_rel = ret_rel?;
    let insert_rel = insert_rel.unwrap_or(ret_rel);

    let push_cmd = |buf: &mut Vec<u8>, cmd: u8, param: u32| {
        let word = ((cmd as u32) << 24) | (param & 0xFFFFFF);
        buf.extend_from_slice(&word.to_le_bytes());
    };

    let mut inserted_cmds = Vec::new();
    let mut inserted_vaddr_offsets = Vec::with_capacity(chunks.len());
    for (chunk_idx, (_, count)) in chunks.iter().enumerate() {
        inserted_vaddr_offsets.push(inserted_cmds.len());
        push_cmd(&mut inserted_cmds, GE_CMD_VADDR, 0);
        if chunk_idx == 0 {
            push_cmd(&mut inserted_cmds, GE_CMD_VERTEXTYPE, vtype);
            push_cmd(&mut inserted_cmds, 0x9B, 1);
        }
        push_cmd(
            &mut inserted_cmds,
            GE_CMD_PRIM,
            (PRIM_TRIANGLES as u32) << 16 | *count as u32,
        );
    }
    let inserted_len = inserted_cmds.len();

    let mut out = template_section.to_vec();
    let mut addr_off = origin_rel;
    while addr_off < ret_rel {
        let word = ru32(&out, addr_off);
        let cmd = ((word >> 24) & 0xFF) as u8;
        if cmd == GE_CMD_VADDR || cmd == GE_CMD_IADDR {
            let param = (word & 0xFFFFFF).checked_add(inserted_len as u32)?;
            if param > 0xFFFFFF {
                return None;
            }
            let patched = ((cmd as u32) << 24) | param;
            out[addr_off..addr_off + 4].copy_from_slice(&patched.to_le_bytes());
        }
        addr_off += 4;
    }

    out.splice(insert_rel..insert_rel, inserted_cmds);
    let extra_vaddr = out.len().checked_sub(origin_rel)?;
    let vertex_size = VtypeInfo::decode(vtype).vertex_size();
    for (chunk_idx, (start, _)) in chunks.iter().enumerate() {
        let chunk_vaddr = extra_vaddr.checked_add(start.checked_mul(vertex_size)?)?;
        if chunk_vaddr > 0xFFFFFF {
            return None;
        }
        let patched_vaddr = ((GE_CMD_VADDR as u32) << 24) | chunk_vaddr as u32;
        let vaddr_pos = insert_rel + inserted_vaddr_offsets[chunk_idx];
        out[vaddr_pos..vaddr_pos + 4].copy_from_slice(&patched_vaddr.to_le_bytes());
    }
    out.extend_from_slice(&vert_buf);
    Some(out)
}

fn retarget_template_section_bbox(
    template_section: &[u8],
    origin_rel: usize,
    old_bbox: &[f32; 3],
    new_bbox: &[f32; 3],
) -> Option<Vec<u8>> {
    if origin_rel + 4 > template_section.len() {
        return None;
    }

    let scan_end = (origin_rel + 0x800).min(template_section.len());
    let cmds = scan_ge_display_list(template_section, origin_rel, scan_end);
    let calls = extract_draw_calls(&cmds, origin_rel);
    if calls.is_empty() {
        return None;
    }

    let mut out = template_section.to_vec();
    let mut rewritten: HashMap<usize, ()> = HashMap::new();
    for dc in &calls {
        let vt = &dc.vtype;
        let vs = vt.vertex_size();
        if vs == 0 || vt.pos_fmt == 0 {
            continue;
        }
        if vt.idx_fmt > 0 && dc.iaddr > 0 {
            let idx_size = [0, 1, 2, 4][vt.idx_fmt as usize & 3];
            for ii in 0..dc.vertex_count {
                let ioff = dc.iaddr + ii * idx_size;
                if ioff + idx_size > out.len() {
                    return None;
                }
                let idx = match idx_size {
                    1 => ru8(&out, ioff) as usize,
                    2 => ru16(&out, ioff) as usize,
                    _ => ru32(&out, ioff) as usize,
                };
                let voff = dc.vaddr.checked_add(idx.checked_mul(vs)?)?;
                if rewritten.insert(voff, ()).is_none() {
                    rewrite_vertex_position_for_bbox(&mut out, voff, vt, old_bbox, new_bbox)?;
                }
            }
        } else {
            for vi in 0..dc.vertex_count {
                let voff = dc.vaddr.checked_add(vi.checked_mul(vs)?)?;
                if rewritten.insert(voff, ()).is_none() {
                    rewrite_vertex_position_for_bbox(&mut out, voff, vt, old_bbox, new_bbox)?;
                }
            }
        }
    }
    Some(out)
}

pub fn rebuild_pmf2(meta: &Pmf2Meta) -> Vec<u8> {
    rebuild_pmf2_with_bbox(
        meta,
        compute_auto_bbox_from_bone_meshes(&meta.bone_meshes).unwrap_or(meta.bbox),
    )
}

pub fn apply_pmf2_metadata_edit(template_pmf2: &[u8], edit: &Pmf2MetadataEdit) -> Result<Vec<u8>> {
    if template_pmf2.len() < 0x20 || &template_pmf2[..4] != b"PMF2" {
        bail!("not a PMF2 stream");
    }
    validate_pmf2_metadata_edit(edit)?;
    let (mut sections, _) = parse_pmf2_sections(template_pmf2);
    if sections.len() != edit.sections.len() {
        bail!(
            "metadata edit section count {} does not match PMF2 section count {}",
            edit.sections.len(),
            sections.len()
        );
    }
    let (bone_meshes, _, _, _) = extract_per_bone_meshes(template_pmf2, false);
    for (section, section_edit) in sections.iter_mut().zip(edit.sections.iter()) {
        section.name = section_edit.name.clone();
        section.parent = section_edit.parent;
        section.local_matrix = section_edit.local_matrix.clone();
    }
    let mut meta = build_meta("pmf2_metadata_edit", &sections, edit.bbox, &bone_meshes);
    meta.bbox = edit.bbox;
    Ok(rebuild_pmf2_with_bbox(&meta, edit.bbox))
}

fn validate_pmf2_metadata_edit(edit: &Pmf2MetadataEdit) -> Result<()> {
    if edit.sections.is_empty() {
        bail!("metadata edit contains no sections");
    }
    for (axis, value) in edit.bbox.iter().enumerate() {
        if !value.is_finite() || *value <= 0.0 {
            bail!("bbox axis {} must be finite and greater than zero", axis);
        }
    }
    let section_count = edit.sections.len();
    for (index, section) in edit.sections.iter().enumerate() {
        if !section.name.is_ascii() || section.name.len() > 15 {
            bail!("section {} name must fit in 15 ASCII bytes", index);
        }
        if section.name.bytes().any(|byte| byte < 0x20 || byte == 0x7F) {
            bail!("section {} name must not contain control characters", index);
        }
        if section.parent < -1 || (section.parent >= 0 && section.parent as usize >= section_count)
        {
            bail!(
                "section {} parent must be -1 or a valid section index",
                index
            );
        }
        if section.parent == index as i32 {
            bail!("section {} cannot be its own parent", index);
        }
        if section.local_matrix.len() != 16 {
            bail!("section {} local matrix must contain 16 values", index);
        }
        if section.local_matrix.iter().any(|value| !value.is_finite()) {
            bail!("section {} local matrix values must be finite", index);
        }
    }
    validate_parent_hierarchy_is_acyclic(edit)?;
    Ok(())
}

fn validate_parent_hierarchy_is_acyclic(edit: &Pmf2MetadataEdit) -> Result<()> {
    for start in 0..edit.sections.len() {
        let mut seen = vec![false; edit.sections.len()];
        let mut current = start;
        loop {
            let parent = edit.sections[current].parent;
            if parent < 0 {
                break;
            }
            let parent_index = parent as usize;
            if seen[parent_index] {
                bail!("section {} parent hierarchy contains a cycle", start);
            }
            seen[parent_index] = true;
            current = parent_index;
        }
    }
    Ok(())
}

fn rebuild_pmf2_with_bbox(meta: &Pmf2Meta, bbox: [f32; 3]) -> Vec<u8> {
    let num_sec = meta.sections.len();

    let mut section_data_list = Vec::new();
    for sm in &meta.sections {
        let mut sec_buf = vec![0u8; 0x100];
        for i in 0..16 {
            sec_buf[i * 4..i * 4 + 4].copy_from_slice(&sm.local_matrix[i].to_le_bytes());
        }
        let name_bytes = sm.name.as_bytes();
        let copy_len = name_bytes.len().min(15);
        sec_buf[0x60..0x60 + copy_len].copy_from_slice(&name_bytes[..copy_len]);

        if sm.parent < 0 {
            sec_buf[0x7C..0x80].copy_from_slice(&0xFFFFFFFFu32.to_le_bytes());
        } else {
            sec_buf[0x7C..0x80].copy_from_slice(&(sm.parent as u32).to_le_bytes());
        }
        for byte in sec_buf.iter_mut().take(0x100).skip(0xC0) {
            *byte = 0xFF;
        }

        let mesh_for_section = meta.bone_meshes.iter().find(|bm| bm.bone_index == sm.index);
        let has_mesh_data = mesh_for_section
            .map(|mesh| !mesh.local_vertices.is_empty())
            .unwrap_or(false);
        let has_mesh_flag = if has_mesh_data { 0u32 } else { 1u32 };
        sec_buf[SECTION_MESH_FLAG_OFFSET..SECTION_MESH_FLAG_OFFSET + 4]
            .copy_from_slice(&has_mesh_flag.to_le_bytes());

        if let Some(mesh) = mesh_for_section {
            if has_mesh_data {
                let ge_data = build_ge_commands(mesh, &bbox);
                sec_buf.extend_from_slice(&ge_data);
            }
        }
        while !sec_buf.len().is_multiple_of(16) {
            sec_buf.push(0);
        }
        section_data_list.push(sec_buf);
    }

    let mut header_size = 0x20 + num_sec * 4;
    while !header_size.is_multiple_of(16) {
        header_size += 4;
    }

    let mut offsets = Vec::new();
    let mut current_offset = header_size;
    for sd in &section_data_list {
        offsets.push(current_offset);
        current_offset += sd.len();
    }

    let mut pmf2 = vec![0u8; current_offset];
    pmf2[0..4].copy_from_slice(b"PMF2");
    pmf2[4..8].copy_from_slice(&(num_sec as u32).to_le_bytes());
    pmf2[8..12].copy_from_slice(&0x20u32.to_le_bytes());
    pmf2[0x10..0x14].copy_from_slice(&bbox[0].to_le_bytes());
    pmf2[0x14..0x18].copy_from_slice(&bbox[1].to_le_bytes());
    pmf2[0x18..0x1C].copy_from_slice(&bbox[2].to_le_bytes());

    for (i, &off) in offsets.iter().enumerate() {
        let pos = 0x20 + i * 4;
        pmf2[pos..pos + 4].copy_from_slice(&(off as u32).to_le_bytes());
    }
    for (i, sd) in section_data_list.iter().enumerate() {
        pmf2[offsets[i]..offsets[i] + sd.len()].copy_from_slice(sd);
    }
    pmf2
}

pub fn patch_pmf2_with_mesh_updates(
    template_pmf2: &[u8],
    meta: &Pmf2Meta,
    matrix_delta_threshold: f32,
) -> Option<Vec<u8>> {
    if template_pmf2.len() < 0x20 || &template_pmf2[..4] != b"PMF2" {
        return None;
    }
    let (template_sections, template_bbox) = parse_pmf2_sections(template_pmf2);
    if template_sections.is_empty() {
        return None;
    }

    let (template_meshes, _, _, _) = extract_per_bone_meshes(template_pmf2, false);
    let template_world_faces = collect_world_face_signatures(&template_meshes);
    let mut template_mesh_by_name: HashMap<String, &BoneMeshData> = HashMap::new();
    for bm in &template_meshes {
        template_mesh_by_name.insert(bm.bone_name.to_ascii_lowercase(), bm);
    }

    let mut source_sec_by_name: HashMap<String, &BoneSection> = HashMap::new();
    for s in &meta.sections {
        source_sec_by_name.insert(s.name.to_ascii_lowercase(), s);
    }
    let mut source_mesh_by_name: HashMap<String, &BoneMeshMeta> = HashMap::new();
    for bm in &meta.bone_meshes {
        source_mesh_by_name.insert(bm.bone_name.to_ascii_lowercase(), bm);
    }
    let source_world_mats = compute_world_matrices(&meta.sections);

    let mut bbox = compute_auto_bbox_from_bone_meshes(&meta.bone_meshes).unwrap_or(template_bbox);
    for axis in 0..3 {
        bbox[axis] = bbox[axis].max(template_bbox[axis]);
    }
    let bbox_changed = bbox
        .iter()
        .zip(template_bbox.iter())
        .any(|(new, old)| (new - old).abs() > f32::EPSILON);

    let threshold = matrix_delta_threshold.max(0.0);
    let num_sec = template_sections.len();

    let mut section_data_list: Vec<Vec<u8>> = Vec::with_capacity(num_sec);

    for tsec in &template_sections {
        let sec_start = tsec.offset;
        let sec_end = sec_start + tsec.size;
        let header_end = (sec_start + 0x100).min(sec_end);

        let key = tsec.name.to_ascii_lowercase();
        let src_sec = source_sec_by_name.get(&key);
        let src_mesh = source_mesh_by_name.get(&key);
        let tmpl_mesh = template_mesh_by_name.get(&key);

        let mut header = template_pmf2[sec_start..header_end].to_vec();
        while header.len() < 0x100 {
            header.push(0);
        }

        if let Some(src) = src_sec {
            if src.local_matrix.len() >= 16 {
                let max_delta = tsec
                    .local_matrix
                    .iter()
                    .zip(src.local_matrix.iter())
                    .take(16)
                    .fold(0.0f32, |acc, (a, b)| acc.max((a - b).abs()));
                if max_delta > threshold {
                    for i in 0..16 {
                        header[i * 4..i * 4 + 4]
                            .copy_from_slice(&src.local_matrix[i].to_le_bytes());
                    }
                }
            }
        }

        let orig_faces = tmpl_mesh.map(|m| m.faces.len()).unwrap_or(0);
        let dae_face_count = src_mesh.map(|m| m.faces.len()).unwrap_or(0);
        let template_had_mesh = tsec.has_mesh && orig_faces > 0;
        let template_section = &template_pmf2[sec_start..sec_end];
        if is_probable_control_root_section(tsec) && dae_face_count > orig_faces {
            eprintln!(
                "  [patch-mesh] WARNING: {} looks like a root/control m00 section; game runtime may skip meshes bound here",
                tsec.name
            );
        }
        let preserved_section = if bbox_changed {
            tsec.origin_offset.and_then(|origin| {
                let origin_rel = origin.checked_sub(sec_start)?;
                retarget_template_section_bbox(template_section, origin_rel, &template_bbox, &bbox)
            })
        } else {
            None
        };

        let mut sec_buf = header.clone();
        if let Some(mesh) = src_mesh.filter(|_| dae_face_count > 0) {
            if template_had_mesh && dae_face_count > orig_faces {
                let used_existing_face_filter = source_world_mats.contains_key(&mesh.bone_index);
                let extra_mesh = if let Some(world) = source_world_mats.get(&mesh.bone_index) {
                    build_mesh_suffix_without_existing_world_faces(
                        mesh,
                        orig_faces,
                        world,
                        &template_world_faces,
                    )
                } else {
                    build_mesh_suffix(mesh, orig_faces)
                };
                let appended = extra_mesh.as_ref().and_then(|extra| {
                    tsec.origin_offset.and_then(|origin| {
                        let origin_rel = origin.checked_sub(sec_start)?;
                        let append_source =
                            preserved_section.as_deref().unwrap_or(template_section);
                        append_mesh_draw_to_template_section(
                            append_source,
                            origin_rel,
                            extra,
                            &bbox,
                        )
                    })
                });
                if let Some(mut section) = appended {
                    section[..0x100].copy_from_slice(&header[..0x100]);
                    sec_buf = section;
                    eprintln!(
                        "  [patch-mesh] appended GE for {}: +{} faces, preserved template mesh (was {} faces)",
                        tsec.name,
                        extra_mesh
                            .as_ref()
                            .map(|mesh| mesh.face_count)
                            .unwrap_or(dae_face_count - orig_faces),
                        orig_faces
                    );
                } else if dae_face_count != orig_faces && !used_existing_face_filter {
                    let ge_data = build_ge_commands(mesh, &bbox);
                    if !ge_data.is_empty() {
                        sec_buf.extend_from_slice(&ge_data);
                        eprintln!(
                            "  [patch-mesh] rebuilt GE for {}: {} verts, {} faces (was {} faces)",
                            tsec.name,
                            mesh.local_vertices.len(),
                            dae_face_count,
                            orig_faces
                        );
                    } else if sec_end > header_end {
                        sec_buf.extend_from_slice(&template_pmf2[header_end..sec_end]);
                    }
                } else if let Some(mut section) = preserved_section {
                    section[..0x100].copy_from_slice(&header[..0x100]);
                    sec_buf = section;
                } else if sec_end > header_end {
                    sec_buf.extend_from_slice(&template_pmf2[header_end..sec_end]);
                }
            } else if template_had_mesh && dae_face_count < orig_faces {
                if let Some(mut section) = preserved_section {
                    section[..0x100].copy_from_slice(&header[..0x100]);
                    sec_buf = section;
                } else if sec_end > header_end {
                    sec_buf.extend_from_slice(&template_pmf2[header_end..sec_end]);
                }
                eprintln!(
                    "  [patch-mesh] preserved {} template mesh: source has fewer faces ({} < {})",
                    tsec.name, dae_face_count, orig_faces
                );
            } else if dae_face_count != orig_faces {
                let filtered_mesh = if !template_had_mesh {
                    source_world_mats.get(&mesh.bone_index).and_then(|world| {
                        build_mesh_suffix_without_existing_world_faces(
                            mesh,
                            0,
                            world,
                            &template_world_faces,
                        )
                    })
                } else {
                    None
                };
                let mesh_for_ge =
                    if !template_had_mesh && source_world_mats.contains_key(&mesh.bone_index) {
                        filtered_mesh.as_ref()
                    } else {
                        Some(*mesh)
                    };
                if let Some(mesh_for_ge) = mesh_for_ge {
                    let ge_data = build_ge_commands(mesh_for_ge, &bbox);
                    if !ge_data.is_empty() {
                        if !template_had_mesh {
                            sec_buf[SECTION_MESH_FLAG_OFFSET..SECTION_MESH_FLAG_OFFSET + 4]
                                .copy_from_slice(&0u32.to_le_bytes());
                            // IDA: section+0x74 indexes the 36-byte material table.
                            // No-mesh sections can contain stale values here; material 0
                            // is the safest default until callers expose a donor material.
                            sec_buf
                                [SECTION_MATERIAL_INDEX_OFFSET..SECTION_MATERIAL_INDEX_OFFSET + 4]
                                .copy_from_slice(&0u32.to_le_bytes());
                        }
                        sec_buf.extend_from_slice(&ge_data);
                        eprintln!(
                            "  [patch-mesh] rebuilt GE for {}: {} verts, {} faces (was {} faces)",
                            tsec.name,
                            mesh_for_ge.local_vertices.len(),
                            mesh_for_ge.face_count,
                            orig_faces
                        );
                    } else if sec_end > header_end {
                        sec_buf.extend_from_slice(&template_pmf2[header_end..sec_end]);
                    }
                } else if let Some(mut section) = preserved_section {
                    section[..0x100].copy_from_slice(&header[..0x100]);
                    sec_buf = section;
                } else if sec_end > header_end {
                    sec_buf.extend_from_slice(&template_pmf2[header_end..sec_end]);
                }
            } else if let Some(mut section) = preserved_section {
                section[..0x100].copy_from_slice(&header[..0x100]);
                sec_buf = section;
            } else if sec_end > header_end {
                sec_buf.extend_from_slice(&template_pmf2[header_end..sec_end]);
            }
        } else if template_had_mesh && (src_mesh.is_none() || dae_face_count == 0) {
            sec_buf[SECTION_MESH_FLAG_OFFSET..SECTION_MESH_FLAG_OFFSET + 4]
                .copy_from_slice(&1u32.to_le_bytes());
            sec_buf.truncate(0x100);
            eprintln!(
                "  [patch-mesh] REMOVED mesh for {}: was {} faces, set +0x70=1 (no-mesh flag)",
                tsec.name, orig_faces
            );
        } else if let Some(mut section) = preserved_section {
            section[..0x100].copy_from_slice(&header[..0x100]);
            sec_buf = section;
        } else if sec_end > header_end {
            sec_buf.extend_from_slice(&template_pmf2[header_end..sec_end]);
        }

        while !sec_buf.len().is_multiple_of(16) {
            sec_buf.push(0);
        }
        section_data_list.push(sec_buf);
    }

    let mut header_size = 0x20 + num_sec * 4;
    while !header_size.is_multiple_of(16) {
        header_size += 4;
    }

    let mut offsets = Vec::new();
    let mut current_offset = header_size;
    for sd in &section_data_list {
        offsets.push(current_offset);
        current_offset += sd.len();
    }

    let mut pmf2 = vec![0u8; current_offset];
    pmf2[0..4].copy_from_slice(b"PMF2");
    pmf2[4..8].copy_from_slice(&(num_sec as u32).to_le_bytes());
    pmf2[8..12].copy_from_slice(&template_pmf2[8..12]);
    pmf2[0x10..0x14].copy_from_slice(&bbox[0].to_le_bytes());
    pmf2[0x14..0x18].copy_from_slice(&bbox[1].to_le_bytes());
    pmf2[0x18..0x1C].copy_from_slice(&bbox[2].to_le_bytes());

    for (i, &off) in offsets.iter().enumerate() {
        let pos = 0x20 + i * 4;
        pmf2[pos..pos + 4].copy_from_slice(&(off as u32).to_le_bytes());
    }
    for (i, sd) in section_data_list.iter().enumerate() {
        pmf2[offsets[i]..offsets[i] + sd.len()].copy_from_slice(sd);
    }
    Some(pmf2)
}

pub fn patch_pmf2_transforms_from_meta_with_threshold(
    template_pmf2: &[u8],
    meta: &Pmf2Meta,
    matrix_delta_threshold: f32,
) -> Option<Vec<u8>> {
    if template_pmf2.len() < 0x20 || &template_pmf2[..4] != b"PMF2" {
        return None;
    }
    let (template_sections, _) = parse_pmf2_sections(template_pmf2);
    if template_sections.is_empty() {
        return None;
    }
    let mut source_by_name: HashMap<String, &BoneSection> = HashMap::new();
    for s in &meta.sections {
        source_by_name.insert(s.name.to_ascii_lowercase(), s);
    }
    let threshold = matrix_delta_threshold.max(0.0);
    let mut out = template_pmf2.to_vec();
    for target in &template_sections {
        let key = target.name.to_ascii_lowercase();
        let Some(src) = source_by_name.get(&key) else {
            continue;
        };
        if src.local_matrix.len() < 16 || target.offset + 16 * 4 > out.len() {
            continue;
        }
        let max_delta = target
            .local_matrix
            .iter()
            .zip(src.local_matrix.iter())
            .take(16)
            .fold(0.0f32, |acc, (a, b)| acc.max((a - b).abs()));
        if max_delta <= threshold {
            continue;
        }
        for i in 0..16 {
            out[target.offset + i * 4..target.offset + i * 4 + 4]
                .copy_from_slice(&src.local_matrix[i].to_le_bytes());
        }
    }
    Some(out)
}

pub fn patch_pmf2_transforms_from_meta(template_pmf2: &[u8], meta: &Pmf2Meta) -> Option<Vec<u8>> {
    patch_pmf2_transforms_from_meta_with_threshold(template_pmf2, meta, 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> Vec<f32> {
        vec![
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ]
    }

    fn test_mesh(name: &str, face_count: usize) -> BoneMeshMeta {
        let mut local_vertices = Vec::new();
        let mut faces = Vec::new();
        for i in 0..face_count {
            let base = local_vertices.len();
            let x = i as f32 * 2.0;
            local_vertices.push([x, 0.0, 0.0, 0.1, 0.2, 0.0, 1.0, 0.0]);
            local_vertices.push([x + 1.0, 0.0, 0.0, 0.3, 0.2, 0.0, 1.0, 0.0]);
            local_vertices.push([x, 1.0, 0.0, 0.1, 0.4, 0.0, 1.0, 0.0]);
            faces.push([base, base + 1, base + 2]);
        }
        BoneMeshMeta {
            bone_index: 0,
            bone_name: name.to_string(),
            vertex_count: local_vertices.len(),
            face_count,
            has_uv: true,
            has_normals: true,
            draw_call_vtypes: Vec::new(),
            local_vertices,
            faces,
        }
    }

    #[test]
    fn encode_mesh_vertices_wraps_negative_uvs_for_game_texture_repeat() {
        let mut mesh = test_mesh("root", 1);
        mesh.local_vertices[0][3] = -0.25;

        let (_, vert_buf, _) = encode_mesh_vertices(&mesh, &[2.0, 2.0, 2.0]).unwrap();
        let encoded_u = i16::from_le_bytes([vert_buf[0], vert_buf[1]]) as f32 / 32768.0;

        assert!((encoded_u - 0.75).abs() < 1.0 / 32768.0);
    }

    fn template_with_custom_mesh_command() -> Vec<u8> {
        let mesh = test_mesh("root", 1);
        let (_, vert_buf, vertex_count) = encode_mesh_vertices(&mesh, &[2.0, 2.0, 2.0]).unwrap();
        let mut section = vec![0u8; 0x100];
        for (i, value) in identity().iter().enumerate() {
            section[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        section[0x60..0x64].copy_from_slice(b"root");
        section[SECTION_MESH_FLAG_OFFSET..SECTION_MESH_FLAG_OFFSET + 4]
            .copy_from_slice(&0u32.to_le_bytes());
        section[0x7C..0x80].copy_from_slice(&0xFFFFFFFFu32.to_le_bytes());

        let push_cmd = |buf: &mut Vec<u8>, cmd: u8, param: u32| {
            let word = ((cmd as u32) << 24) | (param & 0xFFFFFF);
            buf.extend_from_slice(&word.to_le_bytes());
        };
        let vaddr = 7 * 4;
        push_cmd(&mut section, GE_CMD_ORIGIN, 0);
        push_cmd(&mut section, GE_CMD_BASE, 0);
        push_cmd(&mut section, GE_CMD_VADDR, vaddr);
        push_cmd(&mut section, GE_CMD_VERTEXTYPE, (2 << 7) | (2 << 5) | 2);
        push_cmd(&mut section, 0x9B, 1);
        push_cmd(
            &mut section,
            GE_CMD_PRIM,
            (PRIM_TRIANGLES as u32) << 16 | vertex_count as u32,
        );
        push_cmd(&mut section, GE_CMD_RET, 0);
        section.extend_from_slice(&vert_buf);
        while !section.len().is_multiple_of(16) {
            section.push(0);
        }

        let header_size = 0x30usize;
        let mut pmf2 = vec![0u8; header_size + section.len()];
        pmf2[0..4].copy_from_slice(b"PMF2");
        pmf2[4..8].copy_from_slice(&1u32.to_le_bytes());
        pmf2[8..12].copy_from_slice(&0x20u32.to_le_bytes());
        for (off, value) in [2.0f32, 2.0, 2.0].iter().enumerate() {
            pmf2[0x10 + off * 4..0x14 + off * 4].copy_from_slice(&value.to_le_bytes());
        }
        pmf2[0x20..0x24].copy_from_slice(&(header_size as u32).to_le_bytes());
        pmf2[header_size..].copy_from_slice(&section);
        pmf2
    }

    #[test]
    fn patch_appends_new_faces_without_dropping_existing_display_list_state() {
        let template_pmf2 = template_with_custom_mesh_command();
        let source_meta = Pmf2Meta {
            model_name: "test".to_string(),
            bbox: [2.0, 2.0, 2.0],
            section_count: 1,
            sections: vec![BoneSection {
                index: 0,
                name: "root".to_string(),
                offset: 0,
                size: 0,
                local_matrix: identity(),
                parent: -1,
                has_mesh: true,
                origin_offset: None,
                category: String::new(),
            }],
            bone_meshes: vec![test_mesh("root", 2)],
        };

        let patched = patch_pmf2_with_mesh_updates(&template_pmf2, &source_meta, 0.0).unwrap();
        let sec_off = ru32(&patched, 0x20) as usize;
        let section_end = patched.len();
        let section = &patched[sec_off..section_end];
        assert!(section
            .chunks_exact(4)
            .any(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]) == 0x9B000001));

        let (meshes, _, _, _) = extract_per_bone_meshes(&patched, false);
        assert_eq!(meshes[0].faces.len(), 2);
    }

    #[test]
    fn append_inserts_generated_draw_before_post_prim_state_cleanup() {
        let mesh = test_mesh("root", 1);
        let (_, vert_buf, vertex_count) = encode_mesh_vertices(&mesh, &[2.0, 2.0, 2.0]).unwrap();
        let mut section = vec![0u8; 0x100];
        let push_cmd = |buf: &mut Vec<u8>, cmd: u8, param: u32| {
            let word = ((cmd as u32) << 24) | (param & 0xFFFFFF);
            buf.extend_from_slice(&word.to_le_bytes());
        };

        let origin_rel = section.len();
        let vaddr = 8 * 4;
        push_cmd(&mut section, GE_CMD_ORIGIN, 0);
        push_cmd(&mut section, GE_CMD_BASE, 0);
        push_cmd(&mut section, GE_CMD_VADDR, vaddr);
        push_cmd(&mut section, GE_CMD_VERTEXTYPE, (2 << 7) | (2 << 5) | 2);
        push_cmd(&mut section, 0x9B, 1);
        push_cmd(
            &mut section,
            GE_CMD_PRIM,
            (PRIM_TRIANGLES as u32) << 16 | vertex_count as u32,
        );
        push_cmd(&mut section, GE_CMD_VERTEXTYPE, 0);
        push_cmd(&mut section, GE_CMD_RET, 0);
        section.extend_from_slice(&vert_buf);

        let extra = test_mesh("root", 2);
        let extra = build_mesh_suffix(&extra, 1).unwrap();
        let patched =
            append_mesh_draw_to_template_section(&section, origin_rel, &extra, &[2.0, 2.0, 2.0])
                .unwrap();
        let words = patched
            .chunks_exact(4)
            .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
            .collect::<Vec<_>>();
        let prim_positions = words
            .iter()
            .enumerate()
            .filter_map(|(idx, word)| (((word >> 24) & 0xFF) as u8 == GE_CMD_PRIM).then_some(idx))
            .collect::<Vec<_>>();
        let cleanup_pos = words
            .iter()
            .position(|word| *word == ((GE_CMD_VERTEXTYPE as u32) << 24))
            .unwrap();

        assert_eq!(prim_positions.len(), 2);
        assert!(prim_positions[1] < cleanup_pos);
    }

    #[test]
    fn append_chunks_generated_triangle_draws_into_small_batches() {
        let mesh = test_mesh("root", 1);
        let (_, vert_buf, vertex_count) = encode_mesh_vertices(&mesh, &[2.0, 2.0, 2.0]).unwrap();
        let mut section = vec![0u8; 0x100];
        let push_cmd = |buf: &mut Vec<u8>, cmd: u8, param: u32| {
            let word = ((cmd as u32) << 24) | (param & 0xFFFFFF);
            buf.extend_from_slice(&word.to_le_bytes());
        };

        let origin_rel = section.len();
        let vaddr = 8 * 4;
        push_cmd(&mut section, GE_CMD_ORIGIN, 0);
        push_cmd(&mut section, GE_CMD_BASE, 0);
        push_cmd(&mut section, GE_CMD_VADDR, vaddr);
        push_cmd(&mut section, GE_CMD_VERTEXTYPE, (2 << 7) | (2 << 5) | 2);
        push_cmd(&mut section, 0x9B, 1);
        push_cmd(
            &mut section,
            GE_CMD_PRIM,
            (PRIM_TRIANGLES as u32) << 16 | vertex_count as u32,
        );
        push_cmd(&mut section, GE_CMD_RET, 0);
        section.extend_from_slice(&vert_buf);

        let extra = test_mesh("root", 295);
        let extra = build_mesh_suffix(&extra, 1).unwrap();
        let patched =
            append_mesh_draw_to_template_section(&section, origin_rel, &extra, &[2.0, 2.0, 2.0])
                .unwrap();
        let prim_counts = patched
            .chunks_exact(4)
            .filter_map(|word| {
                let value = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
                (((value >> 24) & 0xFF) as u8 == GE_CMD_PRIM).then_some(value & 0xFFFF)
            })
            .collect::<Vec<_>>();
        let appended_counts = &prim_counts[1..];

        assert!(appended_counts.len() > 1);
        assert!(appended_counts.iter().all(|count| *count <= 96));
        assert_eq!(appended_counts.iter().sum::<u32>(), 294 * 3);
    }

    #[test]
    fn append_chunks_do_not_repeat_vertex_state_for_each_prim_batch() {
        let mesh = test_mesh("root", 1);
        let (_, vert_buf, vertex_count) = encode_mesh_vertices(&mesh, &[2.0, 2.0, 2.0]).unwrap();
        let mut section = vec![0u8; 0x100];
        let push_cmd = |buf: &mut Vec<u8>, cmd: u8, param: u32| {
            let word = ((cmd as u32) << 24) | (param & 0xFFFFFF);
            buf.extend_from_slice(&word.to_le_bytes());
        };

        let origin_rel = section.len();
        let vaddr = 8 * 4;
        push_cmd(&mut section, GE_CMD_ORIGIN, 0);
        push_cmd(&mut section, GE_CMD_BASE, 0);
        push_cmd(&mut section, GE_CMD_VADDR, vaddr);
        push_cmd(&mut section, GE_CMD_VERTEXTYPE, (2 << 7) | (2 << 5) | 2);
        push_cmd(&mut section, 0x9B, 1);
        push_cmd(
            &mut section,
            GE_CMD_PRIM,
            (PRIM_TRIANGLES as u32) << 16 | vertex_count as u32,
        );
        push_cmd(&mut section, GE_CMD_RET, 0);
        section.extend_from_slice(&vert_buf);

        let extra = test_mesh("root", 295);
        let extra = build_mesh_suffix(&extra, 1).unwrap();
        let patched =
            append_mesh_draw_to_template_section(&section, origin_rel, &extra, &[2.0, 2.0, 2.0])
                .unwrap();
        let words = patched
            .chunks_exact(4)
            .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
            .collect::<Vec<_>>();
        let prim_positions = words
            .iter()
            .enumerate()
            .filter_map(|(idx, word)| (((word >> 24) & 0xFF) as u8 == GE_CMD_PRIM).then_some(idx))
            .collect::<Vec<_>>();
        let ret_pos = words
            .iter()
            .position(|word| ((*word >> 24) & 0xFF) as u8 == GE_CMD_RET)
            .unwrap();
        let inserted_range = prim_positions[0] + 1..ret_pos;
        let bbox_state_count = words[inserted_range]
            .iter()
            .filter(|word| (((**word >> 24) & 0xFF) as u8) == 0x9B)
            .count();

        assert!(prim_positions.len() > 2);
        assert_eq!(bbox_state_count, 1);
    }

    #[test]
    fn append_chunked_draw_keeps_initial_vaddr_before_vertex_state() {
        let mesh = test_mesh("root", 1);
        let (_, vert_buf, vertex_count) = encode_mesh_vertices(&mesh, &[2.0, 2.0, 2.0]).unwrap();
        let mut section = vec![0u8; 0x100];
        let push_cmd = |buf: &mut Vec<u8>, cmd: u8, param: u32| {
            let word = ((cmd as u32) << 24) | (param & 0xFFFFFF);
            buf.extend_from_slice(&word.to_le_bytes());
        };

        let origin_rel = section.len();
        let vaddr = 8 * 4;
        push_cmd(&mut section, GE_CMD_ORIGIN, 0);
        push_cmd(&mut section, GE_CMD_BASE, 0);
        push_cmd(&mut section, GE_CMD_VADDR, vaddr);
        push_cmd(&mut section, GE_CMD_VERTEXTYPE, (2 << 7) | (2 << 5) | 2);
        push_cmd(&mut section, 0x9B, 1);
        push_cmd(
            &mut section,
            GE_CMD_PRIM,
            (PRIM_TRIANGLES as u32) << 16 | vertex_count as u32,
        );
        push_cmd(&mut section, GE_CMD_RET, 0);
        section.extend_from_slice(&vert_buf);

        let extra = test_mesh("root", 295);
        let extra = build_mesh_suffix(&extra, 1).unwrap();
        let patched =
            append_mesh_draw_to_template_section(&section, origin_rel, &extra, &[2.0, 2.0, 2.0])
                .unwrap();
        let words = patched
            .chunks_exact(4)
            .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
            .collect::<Vec<_>>();
        let prim_positions = words
            .iter()
            .enumerate()
            .filter_map(|(idx, word)| (((word >> 24) & 0xFF) as u8 == GE_CMD_PRIM).then_some(idx))
            .collect::<Vec<_>>();
        let first_inserted = prim_positions[0] + 1;

        assert_eq!(((words[first_inserted] >> 24) & 0xFF) as u8, GE_CMD_VADDR);
        assert_eq!(
            ((words[first_inserted + 1] >> 24) & 0xFF) as u8,
            GE_CMD_VERTEXTYPE
        );
        assert_eq!(((words[first_inserted + 2] >> 24) & 0xFF) as u8, 0x9B);
        assert_eq!(
            ((words[first_inserted + 3] >> 24) & 0xFF) as u8,
            GE_CMD_PRIM
        );
    }

    #[test]
    fn patch_append_filters_faces_that_already_exist_in_template() {
        let template_pmf2 = template_with_custom_mesh_command();
        let mut source_mesh = test_mesh("root", 2);
        source_mesh.faces = vec![
            source_mesh.faces[0],
            source_mesh.faces[0],
            source_mesh.faces[1],
        ];
        source_mesh.face_count = source_mesh.faces.len();
        let source_meta = Pmf2Meta {
            model_name: "test".to_string(),
            bbox: [2.0, 2.0, 2.0],
            section_count: 1,
            sections: vec![BoneSection {
                index: 0,
                name: "root".to_string(),
                offset: 0,
                size: 0,
                local_matrix: identity(),
                parent: -1,
                has_mesh: true,
                origin_offset: None,
                category: String::new(),
            }],
            bone_meshes: vec![source_mesh],
        };

        let patched = patch_pmf2_with_mesh_updates(&template_pmf2, &source_meta, 0.0).unwrap();
        let (meshes, _, _, _) = extract_per_bone_meshes(&patched, false);
        assert_eq!(meshes[0].faces.len(), 2);
    }

    #[test]
    fn patch_preserves_template_section_when_face_filter_finds_no_new_faces() {
        let template_pmf2 = template_with_custom_mesh_command();
        let mut source_mesh = test_mesh("root", 2);
        source_mesh.faces = vec![
            source_mesh.faces[0],
            source_mesh.faces[0],
            source_mesh.faces[0],
        ];
        source_mesh.face_count = source_mesh.faces.len();
        let source_meta = Pmf2Meta {
            model_name: "test".to_string(),
            bbox: [2.0, 2.0, 2.0],
            section_count: 1,
            sections: vec![BoneSection {
                index: 0,
                name: "root".to_string(),
                offset: 0,
                size: 0,
                local_matrix: identity(),
                parent: -1,
                has_mesh: true,
                origin_offset: None,
                category: String::new(),
            }],
            bone_meshes: vec![source_mesh],
        };

        let patched = patch_pmf2_with_mesh_updates(&template_pmf2, &source_meta, 0.0).unwrap();
        let (meshes, _, _, _) = extract_per_bone_meshes(&patched, false);
        assert_eq!(meshes[0].faces.len(), 1);
    }

    #[test]
    fn patch_mesh_updates_are_idempotent_when_reapplied_to_output() {
        let template_pmf2 = template_with_custom_mesh_command();
        let source_meta = Pmf2Meta {
            model_name: "test".to_string(),
            bbox: [2.0, 2.0, 2.0],
            section_count: 1,
            sections: vec![BoneSection {
                index: 0,
                name: "root".to_string(),
                offset: 0,
                size: 0,
                local_matrix: identity(),
                parent: -1,
                has_mesh: true,
                origin_offset: None,
                category: String::new(),
            }],
            bone_meshes: vec![test_mesh("root", 2)],
        };

        let first = patch_pmf2_with_mesh_updates(&template_pmf2, &source_meta, 0.0).unwrap();
        let second = patch_pmf2_with_mesh_updates(&first, &source_meta, 0.0).unwrap();
        let (first_meshes, _, _, _) = extract_per_bone_meshes(&first, false);
        let (second_meshes, _, _, _) = extract_per_bone_meshes(&second, false);

        assert_eq!(first_meshes[0].faces.len(), 2);
        assert_eq!(second_meshes[0].faces.len(), 2);
        assert_eq!(second, first);
    }

    #[test]
    fn patch_mesh_preserves_existing_section_when_source_has_fewer_faces() {
        let template_meta = Pmf2Meta {
            model_name: "test".to_string(),
            bbox: [2.0, 2.0, 2.0],
            section_count: 1,
            sections: vec![BoneSection {
                index: 0,
                name: "root".to_string(),
                offset: 0,
                size: 0,
                local_matrix: identity(),
                parent: -1,
                has_mesh: true,
                origin_offset: None,
                category: String::new(),
            }],
            bone_meshes: vec![test_mesh("root", 2)],
        };
        let template_pmf2 = rebuild_pmf2(&template_meta);
        let source_meta = Pmf2Meta {
            bone_meshes: vec![test_mesh("root", 1)],
            ..template_meta
        };

        let patched = patch_pmf2_with_mesh_updates(&template_pmf2, &source_meta, 0.0).unwrap();
        let (meshes, _, _, _) = extract_per_bone_meshes(&patched, false);
        assert_eq!(meshes[0].faces.len(), 2);
    }

    #[test]
    fn patch_mesh_clears_stale_no_mesh_header_word_when_enabling_section() {
        let template_meta = Pmf2Meta {
            model_name: "test".to_string(),
            bbox: [2.0, 2.0, 2.0],
            section_count: 1,
            sections: vec![BoneSection {
                index: 0,
                name: "root".to_string(),
                offset: 0,
                size: 0,
                local_matrix: identity(),
                parent: -1,
                has_mesh: false,
                origin_offset: None,
                category: String::new(),
            }],
            bone_meshes: Vec::new(),
        };
        let mut template_pmf2 = rebuild_pmf2(&template_meta);
        let sec_off = ru32(&template_pmf2, 0x20) as usize;
        template_pmf2[sec_off + SECTION_MESH_FLAG_OFFSET..sec_off + SECTION_MESH_FLAG_OFFSET + 4]
            .copy_from_slice(&1u32.to_le_bytes());
        template_pmf2
            [sec_off + SECTION_MATERIAL_INDEX_OFFSET..sec_off + SECTION_MATERIAL_INDEX_OFFSET + 4]
            .copy_from_slice(&0x7F4u32.to_le_bytes());
        let source_meta = Pmf2Meta {
            model_name: "test".to_string(),
            bbox: [2.0, 2.0, 2.0],
            section_count: 1,
            sections: vec![BoneSection {
                index: 0,
                name: "root".to_string(),
                offset: 0,
                size: 0,
                local_matrix: identity(),
                parent: -1,
                has_mesh: true,
                origin_offset: None,
                category: String::new(),
            }],
            bone_meshes: vec![test_mesh("root", 1)],
        };

        let patched = patch_pmf2_with_mesh_updates(&template_pmf2, &source_meta, 0.0).unwrap();
        let patched_sec_off = ru32(&patched, 0x20) as usize;
        assert_eq!(
            ru32(&patched, patched_sec_off + SECTION_MESH_FLAG_OFFSET),
            0
        );
        assert_eq!(
            ru32(&patched, patched_sec_off + SECTION_MATERIAL_INDEX_OFFSET),
            0
        );
    }

    #[test]
    fn probable_control_root_section_only_flags_root_m00() {
        let mut section = BoneSection {
            index: 0,
            name: "pl0a_m00".to_string(),
            offset: 0,
            size: 0,
            local_matrix: identity(),
            parent: -1,
            has_mesh: false,
            origin_offset: None,
            category: String::new(),
        };

        assert!(is_probable_control_root_section(&section));
        section.parent = 1;
        assert!(!is_probable_control_root_section(&section));
        section.parent = -1;
        section.name = "pl0a_m01".to_string();
        assert!(!is_probable_control_root_section(&section));
    }

    #[test]
    fn section_render_policy_matches_known_ida_draw_mask_entries() {
        let m00 = section_render_policy(0);
        assert_eq!(m00.flags, Some(0x0002));
        assert!(!m00.draws);
        assert!(m00.traverses);
        assert!(!m00.safe_mesh_target);

        for index in [1usize, 7, 11] {
            let policy = section_render_policy(index);
            assert_eq!(policy.flags, Some(0x0003));
            assert!(policy.draws);
            assert!(policy.traverses);
            assert!(policy.safe_mesh_target);
        }

        let o05 = section_render_policy(24);
        assert_eq!(o05.flags, Some(0x0000));
        assert!(!o05.draws);
        assert!(!o05.traverses);
        assert!(!o05.safe_mesh_target);

        assert_eq!(section_render_policy(57).flags, None);
    }

    #[test]
    fn build_ge_commands_splits_meshes_that_exceed_prim_vertex_limit() {
        let mesh = test_mesh("root", 22_000);
        let ge_data = build_ge_commands(&mesh, &[64.0, 64.0, 64.0]);
        let prim_counts = ge_data
            .chunks_exact(4)
            .filter_map(|word| {
                let value = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
                (((value >> 24) & 0xFF) as u8 == GE_CMD_PRIM).then_some(value & 0xFFFF)
            })
            .collect::<Vec<_>>();

        assert!(prim_counts.len() > 1);
        assert!(prim_counts.iter().all(|count| *count <= u16::MAX as u32));
        assert_eq!(prim_counts.iter().sum::<u32>(), 22_000 * 3);
    }

    #[test]
    fn build_ge_commands_emits_bbox_command_before_prim() {
        let mesh = test_mesh("root", 1);
        let ge_data = build_ge_commands(&mesh, &[2.0, 2.0, 2.0]);
        let words = ge_data
            .chunks_exact(4)
            .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
            .collect::<Vec<_>>();
        let prim_pos = words
            .iter()
            .position(|word| ((*word >> 24) & 0xFF) as u8 == GE_CMD_PRIM)
            .unwrap();

        assert_eq!(words[prim_pos - 1], 0x9B000001);
    }

    #[test]
    fn patch_expands_bbox_for_appended_mesh_without_scaling_template_vertices() {
        let template_pmf2 = template_with_custom_mesh_command();
        let mut source_mesh = test_mesh("root", 2);
        source_mesh.local_vertices[3][0] = 6.0;
        source_mesh.local_vertices[4][0] = 7.0;
        source_mesh.local_vertices[5][0] = 6.0;
        let source_meta = Pmf2Meta {
            model_name: "test".to_string(),
            bbox: [8.0, 2.0, 2.0],
            section_count: 1,
            sections: vec![BoneSection {
                index: 0,
                name: "root".to_string(),
                offset: 0,
                size: 0,
                local_matrix: identity(),
                parent: -1,
                has_mesh: true,
                origin_offset: None,
                category: String::new(),
            }],
            bone_meshes: vec![source_mesh],
        };

        let patched = patch_pmf2_with_mesh_updates(&template_pmf2, &source_meta, 0.0).unwrap();
        let (_, bbox) = parse_pmf2_sections(&patched);
        assert!(bbox[0] > 7.0);

        let (meshes, _, _, _) = extract_per_bone_meshes(&patched, false);
        assert!((meshes[0].local_vertices[1].x - 1.0).abs() < 0.01);
        assert!(meshes[0].local_vertices.iter().any(|v| v.x > 6.9));
    }

    #[test]
    fn rebuilt_pmf2_uses_fixed_header_marker_at_0x08() {
        let meta = Pmf2Meta {
            model_name: "test".to_string(),
            bbox: [1.0, 1.0, 1.0],
            section_count: 1,
            sections: vec![BoneSection {
                index: 0,
                name: "root".to_string(),
                offset: 0,
                size: 0,
                local_matrix: identity(),
                parent: -1,
                has_mesh: false,
                origin_offset: None,
                category: String::new(),
            }],
            bone_meshes: Vec::new(),
        };
        let pmf2 = rebuild_pmf2(&meta);
        let marker = u32::from_le_bytes([pmf2[8], pmf2[9], pmf2[10], pmf2[11]]);
        assert_eq!(marker, 32);
    }

    #[test]
    fn patch_mode_preserves_template_layout_and_updates_matrices() {
        let template_meta = Pmf2Meta {
            model_name: "test".to_string(),
            bbox: [1.0, 1.0, 1.0],
            section_count: 1,
            sections: vec![BoneSection {
                index: 0,
                name: "root".to_string(),
                offset: 0,
                size: 0,
                local_matrix: identity(),
                parent: -1,
                has_mesh: false,
                origin_offset: None,
                category: String::new(),
            }],
            bone_meshes: Vec::new(),
        };
        let template_pmf2 = rebuild_pmf2(&template_meta);
        let mut scaled = identity();
        scaled[0] = 2.0;
        let source_meta = Pmf2Meta {
            model_name: "test".to_string(),
            bbox: [1.0, 1.0, 1.0],
            section_count: 1,
            sections: vec![BoneSection {
                index: 0,
                name: "root".to_string(),
                offset: 0,
                size: 0,
                local_matrix: scaled,
                parent: -1,
                has_mesh: false,
                origin_offset: None,
                category: String::new(),
            }],
            bone_meshes: Vec::new(),
        };
        let patched = patch_pmf2_transforms_from_meta(&template_pmf2, &source_meta).unwrap();
        assert_eq!(patched.len(), template_pmf2.len());
        let (sections, _) = parse_pmf2_sections(&patched);
        assert_eq!(sections.len(), 1);
        assert!((sections[0].local_matrix[0] - 2.0).abs() < 1e-6);
    }

    #[test]
    fn metadata_edit_rebuild_updates_bbox_name_parent_and_matrix() {
        let mut matrix = identity();
        matrix[12] = 1.0;
        let meta = Pmf2Meta {
            model_name: "test".to_string(),
            bbox: [1.0, 2.0, 3.0],
            section_count: 1,
            sections: vec![BoneSection {
                index: 0,
                name: "root".to_string(),
                offset: 0,
                size: 0x100,
                local_matrix: matrix,
                parent: -1,
                has_mesh: false,
                origin_offset: None,
                category: String::new(),
            }],
            bone_meshes: vec![],
        };
        let pmf2 = rebuild_pmf2(&meta);
        let mut edit = Pmf2MetadataEdit::from_pmf2(&pmf2).unwrap();
        edit.bbox = [4.0, 5.0, 6.0];
        edit.sections[0].name = "renamed".to_string();
        edit.sections[0].parent = -1;
        edit.sections[0].local_matrix[12] = 9.0;

        let edited = apply_pmf2_metadata_edit(&pmf2, &edit).unwrap();
        let (sections, bbox) = parse_pmf2_sections(&edited);

        assert_eq!(bbox, [4.0, 5.0, 6.0]);
        assert_eq!(sections[0].name, "renamed");
        assert_eq!(sections[0].parent, -1);
        assert!((sections[0].local_matrix[12] - 9.0).abs() < 1e-6);
    }

    #[test]
    fn metadata_edit_rejects_names_that_do_not_fit_pmf2_section_header() {
        let meta = Pmf2Meta {
            model_name: "test".to_string(),
            bbox: [1.0, 1.0, 1.0],
            section_count: 1,
            sections: vec![BoneSection {
                index: 0,
                name: "root".to_string(),
                offset: 0,
                size: 0x100,
                local_matrix: identity(),
                parent: -1,
                has_mesh: false,
                origin_offset: None,
                category: String::new(),
            }],
            bone_meshes: vec![],
        };
        let pmf2 = rebuild_pmf2(&meta);
        let mut edit = Pmf2MetadataEdit::from_pmf2(&pmf2).unwrap();
        edit.sections[0].name = "this_name_is_too_long".to_string();

        assert!(apply_pmf2_metadata_edit(&pmf2, &edit)
            .unwrap_err()
            .to_string()
            .contains("must fit in 15 ASCII bytes"));
    }

    #[test]
    fn metadata_edit_rejects_names_with_embedded_nul() {
        let meta = Pmf2Meta {
            model_name: "test".to_string(),
            bbox: [1.0, 1.0, 1.0],
            section_count: 1,
            sections: vec![BoneSection {
                index: 0,
                name: "root".to_string(),
                offset: 0,
                size: 0x100,
                local_matrix: identity(),
                parent: -1,
                has_mesh: false,
                origin_offset: None,
                category: String::new(),
            }],
            bone_meshes: vec![],
        };
        let pmf2 = rebuild_pmf2(&meta);
        let mut edit = Pmf2MetadataEdit::from_pmf2(&pmf2).unwrap();
        edit.sections[0].name = "root\0tail".to_string();

        assert!(apply_pmf2_metadata_edit(&pmf2, &edit)
            .unwrap_err()
            .to_string()
            .contains("must not contain control characters"));
    }

    #[test]
    fn metadata_edit_rejects_self_parent() {
        let meta = Pmf2Meta {
            model_name: "test".to_string(),
            bbox: [1.0, 1.0, 1.0],
            section_count: 1,
            sections: vec![BoneSection {
                index: 0,
                name: "root".to_string(),
                offset: 0,
                size: 0x100,
                local_matrix: identity(),
                parent: -1,
                has_mesh: false,
                origin_offset: None,
                category: String::new(),
            }],
            bone_meshes: vec![],
        };
        let pmf2 = rebuild_pmf2(&meta);
        let mut edit = Pmf2MetadataEdit::from_pmf2(&pmf2).unwrap();
        edit.sections[0].parent = 0;

        assert!(apply_pmf2_metadata_edit(&pmf2, &edit)
            .unwrap_err()
            .to_string()
            .contains("cannot be its own parent"));
    }

    #[test]
    fn patch_mode_threshold_skips_tiny_deltas() {
        let template_meta = Pmf2Meta {
            model_name: "test".to_string(),
            bbox: [1.0, 1.0, 1.0],
            section_count: 1,
            sections: vec![BoneSection {
                index: 0,
                name: "root".to_string(),
                offset: 0,
                size: 0,
                local_matrix: identity(),
                parent: -1,
                has_mesh: false,
                origin_offset: None,
                category: String::new(),
            }],
            bone_meshes: Vec::new(),
        };
        let template_pmf2 = rebuild_pmf2(&template_meta);
        let mut tiny = identity();
        tiny[0] = 1.000001;
        let source_meta = Pmf2Meta {
            model_name: "test".to_string(),
            bbox: [1.0, 1.0, 1.0],
            section_count: 1,
            sections: vec![BoneSection {
                index: 0,
                name: "root".to_string(),
                offset: 0,
                size: 0,
                local_matrix: tiny,
                parent: -1,
                has_mesh: false,
                origin_offset: None,
                category: String::new(),
            }],
            bone_meshes: Vec::new(),
        };
        let patched =
            patch_pmf2_transforms_from_meta_with_threshold(&template_pmf2, &source_meta, 1e-4)
                .unwrap();
        assert_eq!(patched, template_pmf2);
    }

    #[test]
    fn rebuild_pmf2_recomputes_bbox_from_mesh_vertices() {
        let meta = Pmf2Meta {
            model_name: "test".to_string(),
            bbox: [1.0, 1.0, 1.0],
            section_count: 1,
            sections: vec![BoneSection {
                index: 0,
                name: "root".to_string(),
                offset: 0,
                size: 0,
                local_matrix: identity(),
                parent: -1,
                has_mesh: true,
                origin_offset: None,
                category: String::new(),
            }],
            bone_meshes: vec![BoneMeshMeta {
                bone_index: 0,
                bone_name: "root".to_string(),
                vertex_count: 3,
                face_count: 1,
                has_uv: false,
                has_normals: false,
                draw_call_vtypes: Vec::new(),
                local_vertices: vec![
                    [4.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
                    [-2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
                    [0.0, 0.0, -3.0, 0.0, 0.0, 0.0, 1.0, 0.0],
                ],
                faces: vec![[0, 1, 2]],
            }],
        };
        let pmf2 = rebuild_pmf2(&meta);
        let (_, bbox) = parse_pmf2_sections(&pmf2);
        assert!(bbox[0] > 4.0);
        assert!(bbox[2] > 3.0);
    }

    #[test]
    fn rebuild_pmf2_sets_no_mesh_flag_for_sections_without_display_list() {
        let meta = Pmf2Meta {
            model_name: "test".to_string(),
            bbox: [1.0, 1.0, 1.0],
            section_count: 2,
            sections: vec![
                BoneSection {
                    index: 0,
                    name: "root".to_string(),
                    offset: 0,
                    size: 0,
                    local_matrix: identity(),
                    parent: -1,
                    has_mesh: true,
                    origin_offset: None,
                    category: String::new(),
                },
                BoneSection {
                    index: 1,
                    name: "child".to_string(),
                    offset: 0,
                    size: 0,
                    local_matrix: identity(),
                    parent: 0,
                    has_mesh: false,
                    origin_offset: None,
                    category: String::new(),
                },
            ],
            bone_meshes: vec![BoneMeshMeta {
                bone_index: 0,
                bone_name: "root".to_string(),
                vertex_count: 3,
                face_count: 1,
                has_uv: false,
                has_normals: false,
                draw_call_vtypes: Vec::new(),
                local_vertices: vec![
                    [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
                    [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
                ],
                faces: vec![[0, 1, 2]],
            }],
        };

        let pmf2 = rebuild_pmf2(&meta);
        let sec0_off = ru32(&pmf2, 0x20) as usize;
        let sec1_off = ru32(&pmf2, 0x24) as usize;
        let sec0_flag = ru32(&pmf2, sec0_off + SECTION_MESH_FLAG_OFFSET);
        let sec1_flag = ru32(&pmf2, sec1_off + SECTION_MESH_FLAG_OFFSET);
        assert_eq!(sec0_flag, 0);
        assert_eq!(sec1_flag, 1);
    }
}

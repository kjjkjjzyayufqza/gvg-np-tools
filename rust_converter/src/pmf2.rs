use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

pub fn extract_per_bone_meshes(
    pmf2_data: &[u8],
    swap_yz: bool,
) -> (
    Vec<BoneMeshData>,
    Vec<BoneSection>,
    [f32; 3],
    HashMap<usize, Vec<f32>>,
) {
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
                    if !cache.contains_key(&idx_val) {
                        match decode_vertex(pmf2_data, dc.vaddr + idx_val * vs, vt) {
                            Some(mut pv) => {
                                pv.x *= sx;
                                pv.y *= sy;
                                pv.z *= sz;
                                cache.insert(idx_val, pv);
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
    val.max(-32768).min(32767) as i16
}

fn build_ge_commands(mesh: &BoneMeshMeta, bbox: &[f32; 3]) -> Vec<u8> {
    let sx = bbox[0] / 32768.0;
    let sy = bbox[1] / 32768.0;
    let sz = bbox[2] / 32768.0;

    let verts_i16: Vec<(i16, i16, i16, i16, i16, i16, i16, i16)> = mesh
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
            let tu = clamp_i16((lv[3] * 32768.0).round() as i32);
            let tv = clamp_i16((lv[4] * 32768.0).round() as i32);
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
    let mut stride = 0usize;
    if mesh.has_uv {
        vtype |= 2;
        stride += 4;
    }
    if mesh.has_normals {
        vtype |= 2 << 5;
        stride += 6;
    }
    vtype |= 2 << 7;
    stride += 6;
    if stride % 2 != 0 {
        stride += 1;
    }

    let num_cmds = 2 + 3 + 1;
    let cmd_block_size = num_cmds * 4;
    let vert_buf_start = cmd_block_size;

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
    push_cmd(&mut ge, 0x01, vert_buf_start as u32);
    push_cmd(&mut ge, 0x12, vtype);
    push_cmd(
        &mut ge,
        0x04,
        (PRIM_TRIANGLES as u32) << 16 | seq_verts.len() as u32,
    );
    push_cmd(&mut ge, 0x0B, 0);

    while ge.len() < cmd_block_size {
        ge.extend_from_slice(&[0, 0, 0, 0]);
    }
    ge.extend_from_slice(&vert_buf);
    ge
}

pub fn rebuild_pmf2(meta: &Pmf2Meta) -> Vec<u8> {
    let num_sec = meta.sections.len();
    let bbox = compute_auto_bbox_from_bone_meshes(&meta.bone_meshes).unwrap_or(meta.bbox);

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
        for i in 0xC0..0x100 {
            sec_buf[i] = 0xFF;
        }

        let mesh_for_section = meta.bone_meshes.iter().find(|bm| bm.bone_index == sm.index);
        let has_mesh_data = mesh_for_section
            .map(|mesh| !mesh.local_vertices.is_empty())
            .unwrap_or(false);
        let has_mesh_flag = if has_mesh_data { 0u32 } else { 1u32 };
        sec_buf[0x70..0x74].copy_from_slice(&has_mesh_flag.to_le_bytes());

        if let Some(mesh) = mesh_for_section {
            if has_mesh_data {
                let ge_data = build_ge_commands(mesh, &bbox);
                sec_buf.extend_from_slice(&ge_data);
            }
        }
        while sec_buf.len() % 16 != 0 {
            sec_buf.push(0);
        }
        section_data_list.push(sec_buf);
    }

    let mut header_size = 0x20 + num_sec * 4;
    while header_size % 16 != 0 {
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
    let mut template_face_count: HashMap<String, usize> = HashMap::new();
    for bm in &template_meshes {
        template_face_count.insert(bm.bone_name.to_ascii_lowercase(), bm.faces.len());
    }

    let mut source_sec_by_name: HashMap<String, &BoneSection> = HashMap::new();
    for s in &meta.sections {
        source_sec_by_name.insert(s.name.to_ascii_lowercase(), s);
    }
    let mut source_mesh_by_name: HashMap<String, &BoneMeshMeta> = HashMap::new();
    for bm in &meta.bone_meshes {
        source_mesh_by_name.insert(bm.bone_name.to_ascii_lowercase(), bm);
    }

    let threshold = matrix_delta_threshold.max(0.0);
    let num_sec = template_sections.len();
    let bbox = &template_bbox;

    let mut section_data_list: Vec<Vec<u8>> = Vec::with_capacity(num_sec);

    for tsec in &template_sections {
        let sec_start = tsec.offset;
        let sec_end = sec_start + tsec.size;
        let header_end = (sec_start + 0x100).min(sec_end);

        let key = tsec.name.to_ascii_lowercase();
        let src_sec = source_sec_by_name.get(&key);
        let src_mesh = source_mesh_by_name.get(&key);

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

        let orig_faces = template_face_count.get(&key).copied().unwrap_or(0);
        let dae_face_count = src_mesh.map(|m| m.faces.len()).unwrap_or(0);
        let dae_vert_count = src_mesh.map(|m| m.local_vertices.len()).unwrap_or(0);
        let template_had_mesh = tsec.has_mesh && orig_faces > 0;

        let mut sec_buf = header;
        if src_mesh.is_some() && dae_face_count > 0 {
            if dae_face_count != orig_faces {
                let mesh = src_mesh.unwrap();
                let ge_data = build_ge_commands(mesh, bbox);
                if !ge_data.is_empty() {
                    if !template_had_mesh {
                        sec_buf[0x70..0x74].copy_from_slice(&0u32.to_le_bytes());
                    }
                    sec_buf.extend_from_slice(&ge_data);
                    eprintln!(
                        "  [patch-mesh] rebuilt GE for {}: {} verts, {} faces (was {} faces)",
                        tsec.name, dae_vert_count, dae_face_count, orig_faces
                    );
                } else if sec_end > header_end {
                    sec_buf.extend_from_slice(&template_pmf2[header_end..sec_end]);
                }
            } else if sec_end > header_end {
                sec_buf.extend_from_slice(&template_pmf2[header_end..sec_end]);
            }
        } else if template_had_mesh && (src_mesh.is_none() || dae_face_count == 0) {
            sec_buf[0x70..0x74].copy_from_slice(&1u32.to_le_bytes());
            sec_buf.truncate(0x100);
            eprintln!(
                "  [patch-mesh] REMOVED mesh for {}: was {} faces, set +0x70=1 (no-mesh flag)",
                tsec.name, orig_faces
            );
        } else if sec_end > header_end {
            sec_buf.extend_from_slice(&template_pmf2[header_end..sec_end]);
        }

        while sec_buf.len() % 16 != 0 {
            sec_buf.push(0);
        }
        section_data_list.push(sec_buf);
    }

    let mut header_size = 0x20 + num_sec * 4;
    while header_size % 16 != 0 {
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
    pmf2[0x10..0x14].copy_from_slice(&template_bbox[0].to_le_bytes());
    pmf2[0x14..0x18].copy_from_slice(&template_bbox[1].to_le_bytes());
    pmf2[0x18..0x1C].copy_from_slice(&template_bbox[2].to_le_bytes());

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
        let sec0_flag = ru32(&pmf2, sec0_off + 0x70);
        let sec1_flag = ru32(&pmf2, sec1_off + 0x70);
        assert_eq!(sec0_flag, 0);
        assert_eq!(sec1_flag, 1);
    }
}

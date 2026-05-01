use crate::pmf2::{
    compute_auto_bbox_from_bone_meshes, compute_world_matrices, BoneMeshData, BoneMeshMeta,
    BoneSection, Pmf2Meta,
};
use anyhow::{anyhow, Context, Result};
use roxmltree::{Document, Node};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use std::io;
use std::path::Path;

pub fn write_dae(
    path: &Path,
    bone_meshes: &[BoneMeshData],
    sections: &[BoneSection],
    model_name: &str,
) -> io::Result<()> {
    if bone_meshes.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "no mesh data available for DAE export",
        ));
    }

    let model_name_escaped = escape_xml(model_name);
    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n");
    xml.push_str(
        "<COLLADA xmlns=\"http://www.collada.org/2005/11/COLLADASchema\" version=\"1.4.1\">\n",
    );
    xml.push_str("  <asset><up_axis>Y_UP</up_axis></asset>\n");
    xml.push_str("  <library_geometries>\n");

    let mut mesh_nodes = Vec::new();
    let mut controller_nodes = Vec::new();
    let section_by_index: HashMap<usize, &BoneSection> =
        sections.iter().map(|s| (s.index, s)).collect();
    let world_mats = if sections.is_empty() {
        HashMap::new()
    } else {
        compute_world_matrices(sections)
    };
    let mut inv_bind_by_section: HashMap<usize, [f64; 16]> = HashMap::new();
    for (idx, wm) in world_mats {
        let converted = convert_coord_matrix(&wm);
        let inv = invert_affine_row_major(&converted).unwrap_or(IDENTITY_F64);
        inv_bind_by_section.insert(idx, inv);
    }

    for (mesh_idx, mesh) in bone_meshes.iter().enumerate() {
        let geom_id = format!("geom_{}_{}", mesh_idx, mesh.bone_index);
        let geom_name = escape_xml(&format!("{}_{}", mesh.bone_name, mesh_idx));
        let src_verts = &mesh.vertices;
        if src_verts.is_empty() || mesh.faces.is_empty() {
            continue;
        }

        let mut positions = Vec::with_capacity(src_verts.len() * 3);
        let mut uvs = Vec::with_capacity(src_verts.len() * 2);
        let mut source_normals = Vec::with_capacity(src_verts.len() * 3);
        for v in src_verts {
            positions.push(v.x as f64);
            positions.push(v.y as f64);
            positions.push(v.z as f64);
            source_normals.push(v.nx as f64);
            source_normals.push(v.ny as f64);
            source_normals.push(v.nz as f64);
            if mesh.has_uv {
                uvs.push(v.u as f64);
                uvs.push(pmf2_v_to_collada(v.v) as f64);
            }
        }

        let mut indices = Vec::with_capacity(mesh.faces.len() * 3);
        for &(a, b, c) in &mesh.faces {
            indices.push(a);
            indices.push(b);
            indices.push(c);
        }
        if mesh.has_normals {
            orient_triangle_winding(&positions, &mut indices, &source_normals);
        }
        let normals = recompute_vertex_normals(&positions, &indices);

        write_geometry(
            &mut xml,
            &geom_id,
            &geom_name,
            &positions,
            Some(&normals),
            if mesh.has_uv { Some(&uvs) } else { None },
            &indices,
        );

        let mut node = String::new();
        let node_id = format!("mesh_{}_{}", mesh_idx, sanitize_id(&mesh.bone_name));
        let node_name = escape_xml(&format!("{}_{}", mesh.bone_name, mesh_idx));
        write!(
            &mut node,
            "<node id=\"{}\" name=\"{}\">",
            node_id, node_name
        )
        .unwrap();
        if let Some(sec) = section_by_index.get(&mesh.bone_index) {
            let controller_id = format!("ctrl_{}_{}", mesh_idx, mesh.bone_index);
            let joint_name = joint_sid(sec.index, &sec.name);
            let joint_node_id = joint_id(sec.index, &sec.name);
            let inv_bind = inv_bind_by_section
                .get(&sec.index)
                .copied()
                .unwrap_or(IDENTITY_F64);
            controller_nodes.push(build_controller(
                &controller_id,
                &geom_id,
                &joint_name,
                src_verts.len(),
                &inv_bind,
            ));
            write!(
                &mut node,
                "<instance_controller url=\"#{}\"><skeleton>#{}</skeleton></instance_controller>",
                controller_id, joint_node_id
            )
            .unwrap();
        } else {
            write!(&mut node, "<instance_geometry url=\"#{}\"/>", geom_id).unwrap();
        }
        node.push_str("</node>");
        mesh_nodes.push(node);
    }

    xml.push_str("  </library_geometries>\n");
    if !controller_nodes.is_empty() {
        xml.push_str("  <library_controllers>\n");
        for c in &controller_nodes {
            xml.push_str("    ");
            xml.push_str(c);
            xml.push('\n');
        }
        xml.push_str("  </library_controllers>\n");
    }
    xml.push_str("  <library_visual_scenes>\n");
    xml.push_str("    <visual_scene id=\"Scene\" name=\"Scene\">\n");

    for node in &mesh_nodes {
        xml.push_str("      ");
        xml.push_str(node);
        xml.push('\n');
    }

    if !sections.is_empty() {
        let mut children: HashMap<Option<usize>, Vec<usize>> = HashMap::new();
        for s in sections {
            let key = if s.parent < 0 {
                None
            } else {
                Some(s.parent as usize)
            };
            children.entry(key).or_default().push(s.index);
        }
        if let Some(roots) = children.get(&None) {
            for root_idx in roots {
                if let Some(node) = build_joint_node(*root_idx, sections, &children) {
                    xml.push_str("      ");
                    xml.push_str(&node);
                    xml.push('\n');
                }
            }
        }
    }

    if mesh_nodes.is_empty() && sections.is_empty() {
        let fallback = format!(
            "<node id=\"{}\" name=\"{}\"/>",
            sanitize_id(model_name),
            model_name_escaped
        );
        xml.push_str("      ");
        xml.push_str(&fallback);
        xml.push('\n');
    }

    xml.push_str("    </visual_scene>\n");
    xml.push_str("  </library_visual_scenes>\n");
    xml.push_str("  <scene><instance_visual_scene url=\"#Scene\"/></scene>\n");
    xml.push_str("</COLLADA>\n");

    std::fs::write(path, xml)
}

fn write_geometry(
    xml: &mut String,
    geom_id: &str,
    geom_name: &str,
    positions: &[f64],
    normals: Option<&[f64]>,
    uvs: Option<&[f64]>,
    indices: &[usize],
) {
    let vertex_count = positions.len() / 3;
    let triangle_count = indices.len() / 3;

    xml.push_str("    <geometry id=\"");
    xml.push_str(geom_id);
    xml.push_str("\" name=\"");
    xml.push_str(geom_name);
    xml.push_str("\">\n");
    xml.push_str("      <mesh>\n");

    xml.push_str("        <source id=\"");
    xml.push_str(geom_id);
    xml.push_str("-positions\">\n");
    xml.push_str("          <float_array id=\"");
    xml.push_str(geom_id);
    xml.push_str("-positions-array\" count=\"");
    write!(xml, "{}", positions.len()).unwrap();
    xml.push_str("\">");
    append_f64_list(xml, positions);
    xml.push_str("</float_array>\n");
    xml.push_str("          <technique_common><accessor source=\"#");
    xml.push_str(geom_id);
    xml.push_str("-positions-array\" count=\"");
    write!(xml, "{}", vertex_count).unwrap();
    xml.push_str("\" stride=\"3\"><param name=\"X\" type=\"float\"/><param name=\"Y\" type=\"float\"/><param name=\"Z\" type=\"float\"/></accessor></technique_common>\n");
    xml.push_str("        </source>\n");

    if let Some(normals) = normals {
        xml.push_str("        <source id=\"");
        xml.push_str(geom_id);
        xml.push_str("-normals\">\n");
        xml.push_str("          <float_array id=\"");
        xml.push_str(geom_id);
        xml.push_str("-normals-array\" count=\"");
        write!(xml, "{}", normals.len()).unwrap();
        xml.push_str("\">");
        append_f64_list(xml, normals);
        xml.push_str("</float_array>\n");
        xml.push_str("          <technique_common><accessor source=\"#");
        xml.push_str(geom_id);
        xml.push_str("-normals-array\" count=\"");
        write!(xml, "{}", vertex_count).unwrap();
        xml.push_str("\" stride=\"3\"><param name=\"X\" type=\"float\"/><param name=\"Y\" type=\"float\"/><param name=\"Z\" type=\"float\"/></accessor></technique_common>\n");
        xml.push_str("        </source>\n");
    }

    if let Some(uvs) = uvs {
        xml.push_str("        <source id=\"");
        xml.push_str(geom_id);
        xml.push_str("-map-0\">\n");
        xml.push_str("          <float_array id=\"");
        xml.push_str(geom_id);
        xml.push_str("-map-0-array\" count=\"");
        write!(xml, "{}", uvs.len()).unwrap();
        xml.push_str("\">");
        append_f64_list(xml, uvs);
        xml.push_str("</float_array>\n");
        xml.push_str("          <technique_common><accessor source=\"#");
        xml.push_str(geom_id);
        xml.push_str("-map-0-array\" count=\"");
        write!(xml, "{}", uvs.len() / 2).unwrap();
        xml.push_str("\" stride=\"2\"><param name=\"S\" type=\"float\"/><param name=\"T\" type=\"float\"/></accessor></technique_common>\n");
        xml.push_str("        </source>\n");
    }

    xml.push_str("        <vertices id=\"");
    xml.push_str(geom_id);
    xml.push_str("-vertices\"><input semantic=\"POSITION\" source=\"#");
    xml.push_str(geom_id);
    xml.push_str("-positions\"/></vertices>\n");
    xml.push_str("        <triangles count=\"");
    write!(xml, "{}", triangle_count).unwrap();
    xml.push_str("\">\n");
    xml.push_str("          <input semantic=\"VERTEX\" source=\"#");
    xml.push_str(geom_id);
    xml.push_str("-vertices\" offset=\"0\"/>\n");

    let mut next_offset = 1usize;
    if normals.is_some() {
        xml.push_str("          <input semantic=\"NORMAL\" source=\"#");
        xml.push_str(geom_id);
        xml.push_str("-normals\" offset=\"");
        write!(xml, "{}", next_offset).unwrap();
        xml.push_str("\"/>\n");
        next_offset += 1;
    }
    if uvs.is_some() {
        xml.push_str("          <input semantic=\"TEXCOORD\" source=\"#");
        xml.push_str(geom_id);
        xml.push_str("-map-0\" offset=\"");
        write!(xml, "{}", next_offset).unwrap();
        xml.push_str("\" set=\"0\"/>\n");
    }

    xml.push_str("          <p>");
    append_triangle_indices(xml, indices, normals.is_some(), uvs.is_some());
    xml.push_str("</p>\n");
    xml.push_str("        </triangles>\n");
    xml.push_str("      </mesh>\n");
    xml.push_str("    </geometry>\n");
}

fn append_f64_list(out: &mut String, values: &[f64]) {
    let mut first = true;
    for v in values {
        if !first {
            out.push(' ');
        }
        first = false;
        if *v != 0.0 && v.abs() < 1e-6 {
            write!(out, "{:.10e}", v).unwrap();
        } else {
            write!(out, "{:.6}", v).unwrap();
        }
    }
}

fn row_major_to_col_major(m: &[f64; 16]) -> [f64; 16] {
    [
        m[0], m[4], m[8], m[12], m[1], m[5], m[9], m[13], m[2], m[6], m[10], m[14], m[3], m[7],
        m[11], m[15],
    ]
}

fn append_matrix_for_collada(out: &mut String, m_row_major: &[f64; 16]) {
    let col = row_major_to_col_major(m_row_major);
    append_f64_list(out, &col);
}

fn append_triangle_indices(out: &mut String, indices: &[usize], has_normals: bool, has_uv: bool) {
    let mut first = true;
    for idx in indices {
        if !first {
            out.push(' ');
        }
        first = false;
        write!(out, "{}", idx).unwrap();
        if has_normals {
            out.push(' ');
            write!(out, "{}", idx).unwrap();
        }
        if has_uv {
            out.push(' ');
            write!(out, "{}", idx).unwrap();
        }
    }
}

fn pmf2_v_to_collada(v: f32) -> f32 {
    1.0 - v
}

fn collada_v_to_pmf2(v: f32) -> f32 {
    1.0 - v
}

fn orient_triangle_winding(positions: &[f64], indices: &mut [usize], source_normals: &[f64]) {
    let vertex_count = positions.len() / 3;
    if source_normals.len() < vertex_count * 3 {
        return;
    }
    for tri in indices.chunks_exact_mut(3) {
        let ia = tri[0];
        let ib = tri[1];
        let ic = tri[2];
        if ia >= vertex_count || ib >= vertex_count || ic >= vertex_count {
            continue;
        }
        let a = [
            positions[ia * 3],
            positions[ia * 3 + 1],
            positions[ia * 3 + 2],
        ];
        let b = [
            positions[ib * 3],
            positions[ib * 3 + 1],
            positions[ib * 3 + 2],
        ];
        let c = [
            positions[ic * 3],
            positions[ic * 3 + 1],
            positions[ic * 3 + 2],
        ];
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let sa = [
            source_normals[ia * 3],
            source_normals[ia * 3 + 1],
            source_normals[ia * 3 + 2],
        ];
        let sb = [
            source_normals[ib * 3],
            source_normals[ib * 3 + 1],
            source_normals[ib * 3 + 2],
        ];
        let sc = [
            source_normals[ic * 3],
            source_normals[ic * 3 + 1],
            source_normals[ic * 3 + 2],
        ];
        let avg = [
            sa[0] + sb[0] + sc[0],
            sa[1] + sb[1] + sc[1],
            sa[2] + sb[2] + sc[2],
        ];
        let dot = n[0] * avg[0] + n[1] * avg[1] + n[2] * avg[2];
        if dot < 0.0 {
            tri.swap(1, 2);
        }
    }
}

fn recompute_vertex_normals(positions: &[f64], indices: &[usize]) -> Vec<f64> {
    let vertex_count = positions.len() / 3;
    let quantize = |v: f64| -> i64 { (v * 100000.0).round() as i64 };
    let vertex_keys: Vec<(i64, i64, i64)> = (0..vertex_count)
        .map(|i| {
            (
                quantize(positions[i * 3]),
                quantize(positions[i * 3 + 1]),
                quantize(positions[i * 3 + 2]),
            )
        })
        .collect();
    let mut key_accum: HashMap<(i64, i64, i64), [f64; 3]> = HashMap::new();
    for tri in indices.chunks_exact(3) {
        let ia = tri[0];
        let ib = tri[1];
        let ic = tri[2];
        if ia >= vertex_count || ib >= vertex_count || ic >= vertex_count {
            continue;
        }
        let a = [
            positions[ia * 3],
            positions[ia * 3 + 1],
            positions[ia * 3 + 2],
        ];
        let b = [
            positions[ib * 3],
            positions[ib * 3 + 1],
            positions[ib * 3 + 2],
        ];
        let c = [
            positions[ic * 3],
            positions[ic * 3 + 1],
            positions[ic * 3 + 2],
        ];
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        for idx in [ia, ib, ic] {
            let e = key_accum.entry(vertex_keys[idx]).or_insert([0.0, 0.0, 0.0]);
            e[0] += n[0];
            e[1] += n[1];
            e[2] += n[2];
        }
    }
    let mut accum = vec![0.0f64; vertex_count * 3];
    for i in 0..vertex_count {
        let k = vertex_keys[i];
        let [x, y, z] = key_accum.get(&k).copied().unwrap_or([0.0, 0.0, 0.0]);
        let len = (x * x + y * y + z * z).sqrt();
        if len > 1e-12 {
            accum[i * 3] = x / len;
            accum[i * 3 + 1] = y / len;
            accum[i * 3 + 2] = z / len;
        } else {
            accum[i * 3] = 0.0;
            accum[i * 3 + 1] = 1.0;
            accum[i * 3 + 2] = 0.0;
        }
    }
    accum
}

fn escape_xml(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

fn sanitize_id(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "model".to_string()
    } else {
        out
    }
}

const S_MAT: [f32; 16] = [
    1., 0., 0., 0., 0., 0., -1., 0., 0., 1., 0., 0., 0., 0., 0., 1.,
];
const S_INV: [f32; 16] = [
    1., 0., 0., 0., 0., 0., 1., 0., 0., -1., 0., 0., 0., 0., 0., 1.,
];
const IDENTITY_F32: [f32; 16] = [
    1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
];
const IDENTITY_F64: [f64; 16] = [
    1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
];

fn m4mul(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    let mut r = [0f32; 16];
    for i in 0..4 {
        for j in 0..4 {
            r[i * 4 + j] = (0..4).map(|k| a[i * 4 + k] * b[k * 4 + j]).sum();
        }
    }
    r
}

fn as_mat4(m: &[f32]) -> [f32; 16] {
    if m.len() >= 16 {
        [
            m[0], m[1], m[2], m[3], m[4], m[5], m[6], m[7], m[8], m[9], m[10], m[11], m[12], m[13],
            m[14], m[15],
        ]
    } else {
        [
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
        ]
    }
}

fn convert_coord_matrix(m: &[f32]) -> [f64; 16] {
    let src = as_mat4(m);
    let r = m4mul(&m4mul(&S_INV, &src), &S_MAT);
    r.map(|v| v as f64)
}

fn invert_affine_row_major(m: &[f64; 16]) -> Option<[f64; 16]> {
    let (a, b, c) = (m[0], m[1], m[2]);
    let (d, e, f) = (m[4], m[5], m[6]);
    let (g, h, i) = (m[8], m[9], m[10]);
    let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv_det = 1.0 / det;
    let r00 = (e * i - f * h) * inv_det;
    let r01 = (c * h - b * i) * inv_det;
    let r02 = (b * f - c * e) * inv_det;
    let r10 = (f * g - d * i) * inv_det;
    let r11 = (a * i - c * g) * inv_det;
    let r12 = (c * d - a * f) * inv_det;
    let r20 = (d * h - e * g) * inv_det;
    let r21 = (b * g - a * h) * inv_det;
    let r22 = (a * e - b * d) * inv_det;

    let tx = m[12];
    let ty = m[13];
    let tz = m[14];
    let itx = -(tx * r00 + ty * r10 + tz * r20);
    let ity = -(tx * r01 + ty * r11 + tz * r21);
    let itz = -(tx * r02 + ty * r12 + tz * r22);

    Some([
        r00, r01, r02, 0.0, r10, r11, r12, 0.0, r20, r21, r22, 0.0, itx, ity, itz, 1.0,
    ])
}

fn joint_id(index: usize, name: &str) -> String {
    format!("joint_{}_{}", index, sanitize_id(name))
}

fn joint_sid(index: usize, name: &str) -> String {
    format!("joint_{}_{}", index, sanitize_id(name))
}

fn build_controller(
    controller_id: &str,
    geometry_id: &str,
    joint_name: &str,
    vertex_count: usize,
    inv_bind: &[f64; 16],
) -> String {
    let mut c = String::new();
    let joints_source = format!("{}-joints", controller_id);
    let bind_source = format!("{}-bind_poses", controller_id);
    let weights_source = format!("{}-weights", controller_id);
    write!(
        &mut c,
        "<controller id=\"{}\"><skin source=\"#{}\"><bind_shape_matrix>",
        controller_id, geometry_id
    )
    .unwrap();
    append_matrix_for_collada(&mut c, &IDENTITY_F64);
    c.push_str("</bind_shape_matrix>");
    write!(
        &mut c,
        "<source id=\"{}\"><Name_array id=\"{}-array\" count=\"1\">{}</Name_array><technique_common><accessor source=\"#{}-array\" count=\"1\" stride=\"1\"><param name=\"JOINT\" type=\"name\"/></accessor></technique_common></source>",
        joints_source, joints_source, joint_name, joints_source
    )
    .unwrap();
    write!(
        &mut c,
        "<source id=\"{}\"><float_array id=\"{}-array\" count=\"16\">",
        bind_source, bind_source
    )
    .unwrap();
    append_matrix_for_collada(&mut c, inv_bind);
    write!(
        &mut c,
        "</float_array><technique_common><accessor source=\"#{}-array\" count=\"1\" stride=\"16\"/></technique_common></source>",
        bind_source
    )
    .unwrap();
    write!(
        &mut c,
        "<source id=\"{}\"><float_array id=\"{}-array\" count=\"1\">1</float_array><technique_common><accessor source=\"#{}-array\" count=\"1\" stride=\"1\"><param name=\"WEIGHT\" type=\"float\"/></accessor></technique_common></source>",
        weights_source, weights_source, weights_source
    )
    .unwrap();
    write!(
        &mut c,
        "<joints><input semantic=\"JOINT\" source=\"#{}\"/><input semantic=\"INV_BIND_MATRIX\" source=\"#{}\"/></joints>",
        joints_source, bind_source
    )
    .unwrap();
    write!(
        &mut c,
        "<vertex_weights count=\"{}\"><input semantic=\"JOINT\" source=\"#{}\" offset=\"0\"/><input semantic=\"WEIGHT\" source=\"#{}\" offset=\"1\"/><vcount>",
        vertex_count, joints_source, weights_source
    )
    .unwrap();
    for i in 0..vertex_count {
        if i > 0 {
            c.push(' ');
        }
        c.push('1');
    }
    c.push_str("</vcount><v>");
    for i in 0..vertex_count {
        if i > 0 {
            c.push(' ');
        }
        c.push_str("0 0");
    }
    c.push_str("</v></vertex_weights></skin></controller>");
    c
}

fn build_joint_node(
    idx: usize,
    sections: &[BoneSection],
    children: &HashMap<Option<usize>, Vec<usize>>,
) -> Option<String> {
    let sec = sections.iter().find(|s| s.index == idx)?;
    let mut out = String::new();
    let id = joint_id(sec.index, &sec.name);
    let sid = joint_sid(sec.index, &sec.name);
    let name = escape_xml(&sec.name);
    write!(
        &mut out,
        "<node id=\"{}\" sid=\"{}\" name=\"{}\" type=\"JOINT\">",
        id, sid, name
    )
    .ok()?;
    let m = convert_coord_matrix(&sec.local_matrix);
    out.push_str("<matrix sid=\"transform\">");
    append_matrix_for_collada(&mut out, &m);
    out.push_str("</matrix>");

    if let Some(cs) = children.get(&Some(sec.index)) {
        for child in cs {
            if let Some(child_node) = build_joint_node(*child, sections, children) {
                out.push_str(&child_node);
            }
        }
    }

    out.push_str("</node>");
    Some(out)
}

#[derive(Clone, Default)]
struct SourceData {
    stride: usize,
    values: Vec<f32>,
}

#[derive(Clone)]
struct GeometryData {
    id: String,
    name: String,
    vertices: Vec<[f32; 8]>,
    faces: Vec<[usize; 3]>,
    has_uv: bool,
    has_normals: bool,
}

#[derive(Clone, Default)]
struct ControllerData {
    geometry_id: String,
    joint_names: Vec<String>,
    dominant_joint_name: Option<String>,
}

pub fn read_dae_to_meta(path: &Path, model_name: Option<&str>) -> Result<Pmf2Meta> {
    let xml = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let name = model_name
        .map(ToOwned::to_owned)
        .or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| "model".to_string());
    parse_dae_to_meta_text(&xml, &name)
}

fn parse_dae_to_meta_text(xml: &str, model_name: &str) -> Result<Pmf2Meta> {
    let doc = Document::parse(xml).context("failed to parse dae xml")?;
    let visual_scene = doc
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "visual_scene")
        .ok_or_else(|| anyhow!("visual_scene not found"))?;

    let geometries = parse_geometries(&doc)?;
    if geometries.is_empty() {
        return Err(anyhow!("no geometry found in dae"));
    }

    let mut sections = Vec::new();
    let mut used_joint_indices = HashSet::new();
    for child in visual_scene
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "node")
    {
        collect_joint_sections(child, None, &mut sections, &mut used_joint_indices);
    }
    normalize_sections(&mut sections);

    let controllers = parse_controllers(&doc);
    if sections.is_empty() {
        sections = build_sections_without_joints(&controllers, &geometries);
    }
    if sections.is_empty() {
        sections.push(BoneSection {
            index: 0,
            name: "root".to_string(),
            offset: 0,
            size: 0,
            local_matrix: IDENTITY_F32.to_vec(),
            parent: -1,
            has_mesh: true,
            origin_offset: None,
            category: String::new(),
        });
    }

    let joint_lookup = build_joint_lookup(&sections);
    let bindings = collect_mesh_bindings(
        visual_scene,
        &controllers,
        &joint_lookup,
        &sections,
        &geometries,
    );

    let world_mats = compute_world_matrices(&sections);
    let mut inv_world_mats: HashMap<usize, [f32; 16]> = HashMap::new();
    for sec in &sections {
        let world = world_mats
            .get(&sec.index)
            .map(|v| vec_to_mat4(v))
            .unwrap_or(IDENTITY_F32);
        let inv = invert_affine_row_major_f32(&world).unwrap_or(IDENTITY_F32);
        inv_world_mats.insert(sec.index, inv);
    }
    let section_names: HashMap<usize, String> =
        sections.iter().map(|s| (s.index, s.name.clone())).collect();

    let mut mesh_by_bone: BTreeMap<usize, BoneMeshMeta> = BTreeMap::new();
    for (geom_id, mut bone_index) in bindings {
        if !section_names.contains_key(&bone_index) {
            bone_index = 0;
        }
        let geom = match geometries.get(&geom_id) {
            Some(g) => g,
            None => continue,
        };
        if geom.vertices.is_empty() || geom.faces.is_empty() {
            continue;
        }
        let inv_world = inv_world_mats
            .get(&bone_index)
            .copied()
            .unwrap_or(IDENTITY_F32);
        let mut local_vertices = Vec::with_capacity(geom.vertices.len());
        for v in &geom.vertices {
            let (wx, wy, wz) = dae_to_game_position(v[0], v[1], v[2]);
            let (lx, ly, lz) = transform_point_f32(&inv_world, wx, wy, wz);
            let (wnx, wny, wnz) = dae_to_game_direction(v[5], v[6], v[7]);
            let (lnx, lny, lnz) = normalize3(transform_direction_f32(&inv_world, wnx, wny, wnz));
            local_vertices.push([lx, ly, lz, v[3], collada_v_to_pmf2(v[4]), lnx, lny, lnz]);
        }

        let entry = mesh_by_bone
            .entry(bone_index)
            .or_insert_with(|| BoneMeshMeta {
                bone_index,
                bone_name: section_names
                    .get(&bone_index)
                    .cloned()
                    .unwrap_or_else(|| format!("bone_{}", bone_index)),
                vertex_count: 0,
                face_count: 0,
                has_uv: geom.has_uv,
                has_normals: geom.has_normals,
                draw_call_vtypes: Vec::new(),
                local_vertices: Vec::new(),
                faces: Vec::new(),
            });
        entry.has_uv |= geom.has_uv;
        entry.has_normals |= geom.has_normals;
        let base = entry.local_vertices.len();
        entry.local_vertices.extend(local_vertices);
        for face in &geom.faces {
            entry
                .faces
                .push([face[0] + base, face[1] + base, face[2] + base]);
        }
    }

    let mut bone_meshes: Vec<BoneMeshMeta> = mesh_by_bone.into_values().collect();
    if bone_meshes.is_empty() {
        return Err(anyhow!("no mesh binding found in dae visual_scene"));
    }
    for bm in &mut bone_meshes {
        bm.vertex_count = bm.local_vertices.len();
        bm.face_count = bm.faces.len();
    }

    let bbox = compute_auto_bbox_from_bone_meshes(&bone_meshes).unwrap_or([1.0, 1.0, 1.0]);

    Ok(Pmf2Meta {
        model_name: model_name.to_string(),
        bbox,
        section_count: sections.len(),
        sections,
        bone_meshes,
    })
}

fn parse_geometries(doc: &Document<'_>) -> Result<BTreeMap<String, GeometryData>> {
    let mut out = BTreeMap::new();
    for geometry in doc
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "geometry")
    {
        let geom_id = match geometry.attribute("id") {
            Some(v) => v.to_string(),
            None => continue,
        };
        let geom_name = geometry.attribute("name").unwrap_or(&geom_id).to_string();
        let mesh = match geometry
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == "mesh")
        {
            Some(v) => v,
            None => continue,
        };

        let mut sources: HashMap<String, SourceData> = HashMap::new();
        for source in mesh
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "source")
        {
            if let Some(id) = source.attribute("id") {
                if let Some(data) = parse_source_data(source) {
                    sources.insert(id.to_string(), data);
                }
            }
        }

        let mut vertices_to_position: HashMap<String, String> = HashMap::new();
        let mut vertices_to_normal: HashMap<String, String> = HashMap::new();
        let mut vertices_to_uv: HashMap<String, String> = HashMap::new();
        for vertices_node in mesh
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "vertices")
        {
            let Some(vertices_id) = vertices_node.attribute("id") else {
                continue;
            };
            for input in vertices_node
                .children()
                .filter(|n| n.is_element() && n.tag_name().name() == "input")
            {
                let semantic = input.attribute("semantic").unwrap_or("");
                let src = input.attribute("source").unwrap_or("");
                match semantic {
                    "POSITION" => {
                        vertices_to_position
                            .insert(vertices_id.to_string(), trim_hash(src).to_string());
                    }
                    "NORMAL" => {
                        vertices_to_normal
                            .insert(vertices_id.to_string(), trim_hash(src).to_string());
                    }
                    "TEXCOORD" => {
                        vertices_to_uv.insert(vertices_id.to_string(), trim_hash(src).to_string());
                    }
                    _ => {}
                }
            }
        }

        let mut vertex_map: HashMap<(usize, usize, usize), usize> = HashMap::new();
        let mut vertices = Vec::<[f32; 8]>::new();
        let mut faces = Vec::<[usize; 3]>::new();
        let mut has_uv = false;
        let mut has_normals = false;

        for tri in mesh
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "triangles")
        {
            let mut max_offset = 0usize;
            let mut position_input: Option<(String, usize)> = None;
            let mut normal_input: Option<(String, usize)> = None;
            let mut uv_input: Option<(String, usize)> = None;
            for input in tri
                .children()
                .filter(|n| n.is_element() && n.tag_name().name() == "input")
            {
                let semantic = input.attribute("semantic").unwrap_or("");
                let source = trim_hash(input.attribute("source").unwrap_or(""));
                let offset = input
                    .attribute("offset")
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(0);
                max_offset = max_offset.max(offset);
                match semantic {
                    "VERTEX" => {
                        let resolved = vertices_to_position
                            .get(source)
                            .cloned()
                            .unwrap_or_else(|| source.to_string());
                        position_input = Some((resolved, offset));
                        if normal_input.is_none() {
                            if let Some(nrm_src) = vertices_to_normal.get(source) {
                                normal_input = Some((nrm_src.clone(), offset));
                            }
                        }
                        if uv_input.is_none() {
                            if let Some(uv_src) = vertices_to_uv.get(source) {
                                uv_input = Some((uv_src.clone(), offset));
                            }
                        }
                    }
                    "POSITION" => {
                        position_input = Some((source.to_string(), offset));
                    }
                    "NORMAL" => {
                        normal_input = Some((source.to_string(), offset));
                    }
                    "TEXCOORD" => {
                        if uv_input.is_none() {
                            uv_input = Some((source.to_string(), offset));
                        }
                    }
                    _ => {}
                }
            }
            let Some((position_source, pos_offset)) = position_input else {
                continue;
            };
            let stride = max_offset + 1;
            if stride == 0 {
                continue;
            }
            let Some(p_node) = tri
                .children()
                .find(|n| n.is_element() && n.tag_name().name() == "p")
            else {
                continue;
            };
            let p_values = parse_usize_list(p_node.text().unwrap_or(""));
            if p_values.is_empty() || !p_values.len().is_multiple_of(stride) {
                continue;
            }

            has_normals |= normal_input.is_some();
            has_uv |= uv_input.is_some();

            let mut corner_indices = Vec::with_capacity(p_values.len() / stride);
            for chunk in p_values.chunks(stride) {
                let pos_index = chunk[pos_offset];
                let normal_index = normal_input
                    .as_ref()
                    .map(|(_, off)| chunk[*off])
                    .unwrap_or(usize::MAX);
                let uv_index = uv_input
                    .as_ref()
                    .map(|(_, off)| chunk[*off])
                    .unwrap_or(usize::MAX);
                let key = (pos_index, normal_index, uv_index);
                let idx = if let Some(existing) = vertex_map.get(&key) {
                    *existing
                } else {
                    let p = read_source_vec3(&sources, &position_source, pos_index);
                    let n = if let Some((normal_source, _)) = &normal_input {
                        read_source_vec3(&sources, normal_source, normal_index)
                    } else {
                        [0.0, 1.0, 0.0]
                    };
                    let uv = if let Some((uv_source, _)) = &uv_input {
                        read_source_vec2(&sources, uv_source, uv_index)
                    } else {
                        [0.0, 0.0]
                    };
                    let new_idx = vertices.len();
                    vertices.push([p[0], p[1], p[2], uv[0], uv[1], n[0], n[1], n[2]]);
                    vertex_map.insert(key, new_idx);
                    new_idx
                };
                corner_indices.push(idx);
            }
            for tri_indices in corner_indices.chunks(3) {
                if tri_indices.len() == 3 {
                    faces.push([tri_indices[0], tri_indices[1], tri_indices[2]]);
                }
            }
        }

        if !vertices.is_empty() && !faces.is_empty() {
            out.insert(
                geom_id.clone(),
                GeometryData {
                    id: geom_id,
                    name: geom_name,
                    vertices,
                    faces,
                    has_uv,
                    has_normals,
                },
            );
        }
    }
    Ok(out)
}

fn parse_source_data(source_node: Node<'_, '_>) -> Option<SourceData> {
    let float_array = source_node
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "float_array")?;
    let values = parse_f32_list(float_array.text().unwrap_or(""));
    if values.is_empty() {
        return None;
    }
    let stride = source_node
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "accessor")
        .and_then(|n| n.attribute("stride"))
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(3)
        .max(1);
    Some(SourceData { stride, values })
}

fn parse_controllers(doc: &Document<'_>) -> HashMap<String, ControllerData> {
    let mut out = HashMap::new();
    for controller in doc
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "controller")
    {
        let Some(controller_id) = controller.attribute("id") else {
            continue;
        };
        let Some(skin) = controller
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == "skin")
        else {
            continue;
        };
        let geometry_id = trim_hash(skin.attribute("source").unwrap_or("")).to_string();
        let sources: HashMap<String, Node<'_, '_>> = skin
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "source")
            .filter_map(|n| n.attribute("id").map(|id| (id.to_string(), n)))
            .collect();
        let joint_source_id = skin
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == "joints")
            .and_then(|joints| {
                joints
                    .children()
                    .find(|n| {
                        n.is_element()
                            && n.tag_name().name() == "input"
                            && n.attribute("semantic") == Some("JOINT")
                    })
                    .and_then(|input| input.attribute("source"))
                    .map(|s| trim_hash(s).to_string())
            });
        let joint_names = joint_source_id
            .as_deref()
            .and_then(|id| sources.get(id).copied())
            .map(read_name_array)
            .unwrap_or_default();
        let dominant_joint_name = detect_dominant_joint_name(skin, &sources, &joint_names);
        out.insert(
            controller_id.to_string(),
            ControllerData {
                geometry_id,
                joint_names,
                dominant_joint_name,
            },
        );
    }
    out
}

fn read_name_array(source_node: Node<'_, '_>) -> Vec<String> {
    for tag in ["Name_array", "IDREF_array"] {
        if let Some(array) = source_node
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == tag)
        {
            return array
                .text()
                .unwrap_or("")
                .split_whitespace()
                .map(ToOwned::to_owned)
                .collect();
        }
    }
    Vec::new()
}

fn detect_dominant_joint_name(
    skin: Node<'_, '_>,
    sources: &HashMap<String, Node<'_, '_>>,
    joint_names: &[String],
) -> Option<String> {
    if joint_names.is_empty() {
        return None;
    }
    let vertex_weights = skin
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "vertex_weights")?;
    let mut joint_offset = None;
    let mut max_offset = 0usize;
    for input in vertex_weights
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "input")
    {
        let offset = input
            .attribute("offset")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0);
        max_offset = max_offset.max(offset);
        if input.attribute("semantic") == Some("JOINT") {
            let source_id = trim_hash(input.attribute("source").unwrap_or(""));
            if let Some(source_node) = sources.get(source_id) {
                let names = read_name_array(*source_node);
                if !names.is_empty() {
                    joint_offset = Some((offset, names));
                }
            }
            if joint_offset.is_none() {
                joint_offset = Some((offset, joint_names.to_vec()));
            }
        }
    }
    let (joint_offset, names) = joint_offset?;
    let stride = max_offset + 1;
    if stride == 0 {
        return None;
    }
    let vcount = vertex_weights
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "vcount")
        .map(|n| parse_usize_list(n.text().unwrap_or("")))
        .unwrap_or_default();
    let v = vertex_weights
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "v")
        .map(|n| parse_usize_list(n.text().unwrap_or("")))
        .unwrap_or_default();
    if vcount.is_empty() || v.is_empty() {
        return None;
    }
    let mut counts: HashMap<usize, usize> = HashMap::new();
    let mut ptr = 0usize;
    for influence_count in &vcount {
        for _ in 0..*influence_count {
            if ptr + stride > v.len() {
                break;
            }
            let joint_index = v[ptr + joint_offset];
            *counts.entry(joint_index).or_insert(0) += 1;
            ptr += stride;
        }
    }
    let dominant_index = counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(idx, _)| idx)?;
    names.get(dominant_index).cloned()
}

fn collect_joint_sections(
    node: Node<'_, '_>,
    parent: Option<usize>,
    sections: &mut Vec<BoneSection>,
    used_indices: &mut HashSet<usize>,
) {
    if !node.is_element() || node.tag_name().name() != "node" {
        return;
    }
    let is_joint = node
        .attribute("type")
        .map(|v| v.eq_ignore_ascii_case("JOINT"))
        .unwrap_or(false);
    if is_joint {
        let mut index = parse_joint_index_hint(node).unwrap_or(sections.len());
        while used_indices.contains(&index) {
            index += 1;
        }
        used_indices.insert(index);
        let name = node
            .attribute("name")
            .or_else(|| node.attribute("sid"))
            .or_else(|| node.attribute("id"))
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| format!("bone_{}", index));
        let collada_local = read_node_matrix(node).unwrap_or(IDENTITY_F32);
        let local_matrix = restore_coord_matrix(&collada_local).to_vec();
        sections.push(BoneSection {
            index,
            name,
            offset: 0,
            size: 0,
            local_matrix,
            parent: parent.map(|v| v as i32).unwrap_or(-1),
            has_mesh: true,
            origin_offset: None,
            category: String::new(),
        });
        for child in node
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "node")
        {
            collect_joint_sections(child, Some(index), sections, used_indices);
        }
        return;
    }
    for child in node
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "node")
    {
        collect_joint_sections(child, parent, sections, used_indices);
    }
}

fn parse_joint_index_hint(node: Node<'_, '_>) -> Option<usize> {
    for attr_name in ["sid", "id", "name"] {
        let value = node.attribute(attr_name).unwrap_or("");
        let trimmed = trim_hash(value);
        if let Some(rest) = trimmed.strip_prefix("joint_") {
            if let Some((index_text, _)) = rest.split_once('_') {
                if let Ok(index) = index_text.parse::<usize>() {
                    return Some(index);
                }
            }
        }
    }
    None
}

fn normalize_sections(sections: &mut [BoneSection]) {
    if sections.is_empty() {
        return;
    }
    sections.sort_by_key(|s| s.index);
    let old_to_new: HashMap<usize, usize> = sections
        .iter()
        .enumerate()
        .map(|(new_index, s)| (s.index, new_index))
        .collect();
    for (new_index, sec) in sections.iter_mut().enumerate() {
        let old_parent = sec.parent;
        sec.index = new_index;
        sec.parent = if old_parent < 0 {
            -1
        } else {
            old_to_new
                .get(&(old_parent as usize))
                .map(|v| *v as i32)
                .unwrap_or(-1)
        };
    }
}

fn read_node_matrix(node: Node<'_, '_>) -> Option<[f32; 16]> {
    let matrix = node
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "matrix")?;
    let values = parse_f32_list(matrix.text().unwrap_or(""));
    if values.len() < 16 {
        return None;
    }
    Some(col_major_to_row_major_f32(&values[..16]))
}

fn build_sections_without_joints(
    controllers: &HashMap<String, ControllerData>,
    geometries: &BTreeMap<String, GeometryData>,
) -> Vec<BoneSection> {
    let mut names = BTreeSet::new();
    for ctrl in controllers.values() {
        for name in &ctrl.joint_names {
            names.insert(trim_hash(name).to_string());
        }
    }
    if names.is_empty() {
        for geom in geometries.values() {
            if let Some((base, suffix)) = geom.name.rsplit_once('_') {
                if suffix.chars().all(|c| c.is_ascii_digit()) {
                    names.insert(base.to_string());
                }
            }
        }
    }
    if names.is_empty() {
        names.insert("root".to_string());
    }
    names
        .into_iter()
        .enumerate()
        .map(|(index, name)| BoneSection {
            index,
            name,
            offset: 0,
            size: 0,
            local_matrix: IDENTITY_F32.to_vec(),
            parent: -1,
            has_mesh: true,
            origin_offset: None,
            category: String::new(),
        })
        .collect()
}

fn build_joint_lookup(sections: &[BoneSection]) -> HashMap<String, usize> {
    let mut map = HashMap::new();
    for sec in sections {
        map.insert(sec.name.clone(), sec.index);
        map.insert(sec.name.to_ascii_lowercase(), sec.index);
        let jid = joint_id(sec.index, &sec.name);
        let jsid = joint_sid(sec.index, &sec.name);
        map.insert(jid.clone(), sec.index);
        map.insert(jsid.clone(), sec.index);
        map.insert(jid.to_ascii_lowercase(), sec.index);
        map.insert(jsid.to_ascii_lowercase(), sec.index);
    }
    map
}

fn collect_mesh_bindings(
    visual_scene: Node<'_, '_>,
    controllers: &HashMap<String, ControllerData>,
    joint_lookup: &HashMap<String, usize>,
    sections: &[BoneSection],
    geometries: &BTreeMap<String, GeometryData>,
) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for node in visual_scene
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "node")
    {
        if node
            .attribute("type")
            .map(|v| v.eq_ignore_ascii_case("JOINT"))
            .unwrap_or(false)
        {
            continue;
        }
        let node_name = node
            .attribute("name")
            .or_else(|| node.attribute("id"))
            .unwrap_or("");
        for instance_controller in node
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "instance_controller")
        {
            let controller_id = trim_hash(instance_controller.attribute("url").unwrap_or(""));
            let Some(controller) = controllers.get(controller_id) else {
                continue;
            };
            if !geometries.contains_key(&controller.geometry_id) {
                continue;
            }
            let mut bone_index = controller
                .dominant_joint_name
                .as_deref()
                .and_then(|name| resolve_bone_index(name, joint_lookup))
                .or_else(|| {
                    geometries
                        .get(&controller.geometry_id)
                        .and_then(|g| match_bone_from_name(&g.name, sections))
                });
            if bone_index.is_none() {
                for name in &controller.joint_names {
                    bone_index = resolve_bone_index(name, joint_lookup);
                    if bone_index.is_some() {
                        break;
                    }
                }
            }
            if bone_index.is_none() {
                for skeleton in instance_controller
                    .children()
                    .filter(|n| n.is_element() && n.tag_name().name() == "skeleton")
                {
                    bone_index = resolve_bone_index(skeleton.text().unwrap_or(""), joint_lookup);
                    if bone_index.is_some() {
                        break;
                    }
                }
            }
            let bone_index = bone_index
                .or_else(|| match_bone_from_name(node_name, sections))
                .unwrap_or(0);
            let key = (controller.geometry_id.clone(), bone_index);
            if seen.insert(key.clone()) {
                out.push(key);
            }
        }
        for instance_geometry in node
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "instance_geometry")
        {
            let geometry_id =
                trim_hash(instance_geometry.attribute("url").unwrap_or("")).to_string();
            if !geometries.contains_key(&geometry_id) {
                continue;
            }
            let bone_index = match_bone_from_name(node_name, sections)
                .or_else(|| {
                    geometries
                        .get(&geometry_id)
                        .and_then(|g| match_bone_from_name(&g.name, sections))
                })
                .unwrap_or(0);
            let key = (geometry_id, bone_index);
            if seen.insert(key.clone()) {
                out.push(key);
            }
        }
    }
    if out.is_empty() {
        for geom in geometries.values() {
            let bone_index = match_bone_from_name(&geom.name, sections).unwrap_or(0);
            out.push((geom.id.clone(), bone_index));
        }
    }
    out
}

fn resolve_bone_index(name: &str, joint_lookup: &HashMap<String, usize>) -> Option<usize> {
    let key = trim_hash(name.trim());
    if key.is_empty() {
        return None;
    }
    if let Some(v) = joint_lookup.get(key) {
        return Some(*v);
    }
    let lower = key.to_ascii_lowercase();
    if let Some(v) = joint_lookup.get(&lower) {
        return Some(*v);
    }
    if let Some(rest) = key.strip_prefix("joint_") {
        if let Some((index_text, _)) = rest.split_once('_') {
            if let Ok(index) = index_text.parse::<usize>() {
                return Some(index);
            }
        }
    }
    None
}

fn match_bone_from_name(name: &str, sections: &[BoneSection]) -> Option<usize> {
    let value = name.trim();
    if value.is_empty() {
        return None;
    }
    let base = if let Some((left, right)) = value.rsplit_once('_') {
        if right.chars().all(|c| c.is_ascii_digit()) {
            left
        } else {
            value
        }
    } else {
        value
    };
    for sec in sections {
        if sec.name.eq_ignore_ascii_case(base) {
            return Some(sec.index);
        }
    }
    for sec in sections {
        if value.eq_ignore_ascii_case(&sec.name) {
            return Some(sec.index);
        }
    }
    None
}

fn read_source_vec3(
    sources: &HashMap<String, SourceData>,
    source_id: &str,
    index: usize,
) -> [f32; 3] {
    let Some(source) = sources.get(source_id) else {
        return [0.0, 0.0, 0.0];
    };
    if source.values.is_empty() {
        return [0.0, 0.0, 0.0];
    }
    let stride = source.stride.max(1);
    let start = index.saturating_mul(stride);
    if start >= source.values.len() {
        return [0.0, 0.0, 0.0];
    }
    let x = source.values.get(start).copied().unwrap_or(0.0);
    let y = source.values.get(start + 1).copied().unwrap_or(0.0);
    let z = source.values.get(start + 2).copied().unwrap_or(0.0);
    [x, y, z]
}

fn read_source_vec2(
    sources: &HashMap<String, SourceData>,
    source_id: &str,
    index: usize,
) -> [f32; 2] {
    let Some(source) = sources.get(source_id) else {
        return [0.0, 0.0];
    };
    if source.values.is_empty() {
        return [0.0, 0.0];
    }
    let stride = source.stride.max(1);
    let start = index.saturating_mul(stride);
    if start >= source.values.len() {
        return [0.0, 0.0];
    }
    let u = source.values.get(start).copied().unwrap_or(0.0);
    let v = source.values.get(start + 1).copied().unwrap_or(0.0);
    [u, v]
}

fn parse_f32_list(text: &str) -> Vec<f32> {
    text.split_whitespace()
        .filter_map(|v| v.parse::<f32>().ok())
        .collect()
}

fn parse_usize_list(text: &str) -> Vec<usize> {
    text.split_whitespace()
        .filter_map(|v| v.parse::<usize>().ok())
        .collect()
}

fn trim_hash(value: &str) -> &str {
    value.strip_prefix('#').unwrap_or(value)
}

fn row_major_to_col_major_f32(m: &[f32; 16]) -> [f32; 16] {
    [
        m[0], m[4], m[8], m[12], m[1], m[5], m[9], m[13], m[2], m[6], m[10], m[14], m[3], m[7],
        m[11], m[15],
    ]
}

fn col_major_to_row_major_f32(values: &[f32]) -> [f32; 16] {
    let mut m = [0.0f32; 16];
    for (i, value) in m.iter_mut().enumerate() {
        *value = values.get(i).copied().unwrap_or(0.0);
    }
    row_major_to_col_major_f32(&m)
}

fn restore_coord_matrix(m: &[f32; 16]) -> [f32; 16] {
    m4mul(&m4mul(&S_MAT, m), &S_INV)
}

fn vec_to_mat4(values: &[f32]) -> [f32; 16] {
    if values.len() >= 16 {
        [
            values[0], values[1], values[2], values[3], values[4], values[5], values[6], values[7],
            values[8], values[9], values[10], values[11], values[12], values[13], values[14],
            values[15],
        ]
    } else {
        IDENTITY_F32
    }
}

fn invert_affine_row_major_f32(m: &[f32; 16]) -> Option<[f32; 16]> {
    let (a, b, c) = (m[0], m[1], m[2]);
    let (d, e, f) = (m[4], m[5], m[6]);
    let (g, h, i) = (m[8], m[9], m[10]);
    let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv_det = 1.0 / det;
    let r00 = (e * i - f * h) * inv_det;
    let r01 = (c * h - b * i) * inv_det;
    let r02 = (b * f - c * e) * inv_det;
    let r10 = (f * g - d * i) * inv_det;
    let r11 = (a * i - c * g) * inv_det;
    let r12 = (c * d - a * f) * inv_det;
    let r20 = (d * h - e * g) * inv_det;
    let r21 = (b * g - a * h) * inv_det;
    let r22 = (a * e - b * d) * inv_det;

    let tx = m[12];
    let ty = m[13];
    let tz = m[14];
    let itx = -(tx * r00 + ty * r10 + tz * r20);
    let ity = -(tx * r01 + ty * r11 + tz * r21);
    let itz = -(tx * r02 + ty * r12 + tz * r22);

    Some([
        r00, r01, r02, 0.0, r10, r11, r12, 0.0, r20, r21, r22, 0.0, itx, ity, itz, 1.0,
    ])
}

fn transform_point_f32(m: &[f32; 16], x: f32, y: f32, z: f32) -> (f32, f32, f32) {
    (
        x * m[0] + y * m[4] + z * m[8] + m[12],
        x * m[1] + y * m[5] + z * m[9] + m[13],
        x * m[2] + y * m[6] + z * m[10] + m[14],
    )
}

fn transform_direction_f32(m: &[f32; 16], x: f32, y: f32, z: f32) -> (f32, f32, f32) {
    (
        x * m[0] + y * m[4] + z * m[8],
        x * m[1] + y * m[5] + z * m[9],
        x * m[2] + y * m[6] + z * m[10],
    )
}

fn normalize3(v: (f32, f32, f32)) -> (f32, f32, f32) {
    let len = (v.0 * v.0 + v.1 * v.1 + v.2 * v.2).sqrt();
    if len > 1e-12 {
        (v.0 / len, v.1 / len, v.2 / len)
    } else {
        (0.0, 1.0, 0.0)
    }
}

fn dae_to_game_position(x: f32, y: f32, z: f32) -> (f32, f32, f32) {
    (x, -z, y)
}

fn dae_to_game_direction(x: f32, y: f32, z: f32) -> (f32, f32, f32) {
    (x, -z, y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pmf2::ParsedVertex;

    fn tmp_path(name: &str) -> std::path::PathBuf {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("{}_{}_{}.dae", name, std::process::id(), now))
    }

    fn mesh(bone_index: usize, bone_name: &str, has_uv: bool, has_normals: bool) -> BoneMeshData {
        BoneMeshData {
            bone_index,
            bone_name: bone_name.to_string(),
            vertices: vec![
                ParsedVertex {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    u: 0.0,
                    v: 0.0,
                    nx: 0.0,
                    ny: 1.0,
                    nz: 0.0,
                },
                ParsedVertex {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                    u: 1.0,
                    v: 0.0,
                    nx: 0.0,
                    ny: 1.0,
                    nz: 0.0,
                },
                ParsedVertex {
                    x: 0.0,
                    y: 1.0,
                    z: 0.0,
                    u: 0.0,
                    v: 1.0,
                    nx: 0.0,
                    ny: 1.0,
                    nz: 0.0,
                },
            ],
            faces: vec![(0, 1, 2)],
            local_vertices: vec![
                ParsedVertex {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    u: 0.0,
                    v: 0.0,
                    nx: 0.0,
                    ny: 1.0,
                    nz: 0.0,
                },
                ParsedVertex {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                    u: 1.0,
                    v: 0.0,
                    nx: 0.0,
                    ny: 1.0,
                    nz: 0.0,
                },
                ParsedVertex {
                    x: 0.0,
                    y: 1.0,
                    z: 0.0,
                    u: 0.0,
                    v: 1.0,
                    nx: 0.0,
                    ny: 1.0,
                    nz: 0.0,
                },
            ],
            has_uv,
            has_normals,
            vtypes: Vec::new(),
        }
    }

    fn section(index: usize, name: &str, parent: i32, tx: f32, ty: f32, tz: f32) -> BoneSection {
        BoneSection {
            index,
            name: name.to_string(),
            offset: 0,
            size: 0,
            local_matrix: vec![
                1.0, 0.0, 0.0, 0.0, //
                0.0, 1.0, 0.0, 0.0, //
                0.0, 0.0, 1.0, 0.0, //
                tx, ty, tz, 1.0,
            ],
            parent,
            has_mesh: true,
            origin_offset: None,
            category: String::new(),
        }
    }

    #[test]
    fn writes_joint_hierarchy_and_multiple_meshes() {
        let path = tmp_path("gvg_dae_test");
        let meshes = vec![mesh(0, "root", true, true), mesh(1, "arm", true, true)];
        let sections = vec![
            section(0, "root", -1, 0.0, 0.0, 0.0),
            section(1, "arm", 0, 0.0, 1.0, 0.0),
        ];
        write_dae(&path, &meshes, &sections, "pl00_stream000").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(text.matches("<geometry id=").count() >= 2);
        assert!(text.contains("type=\"JOINT\""));
        assert!(text.contains("<library_controllers>"));
        assert!(text.contains("<instance_controller"));
        assert!(text.contains("<vertex_weights"));
        assert!(!text.contains("id=\"tree\""));
        assert!(!text.contains("id=\"bone_list\""));
        assert!(text.contains("joint_0_root"));
        assert!(text.contains("joint_1_arm"));
        assert!(text.contains("source=\"#geom_0_0\""));
        assert!(text.contains("source=\"#geom_1_1\""));
        let first_joint = text.find("type=\"JOINT\"").unwrap();
        let first_mesh = text.find("<instance_controller").unwrap();
        assert!(first_mesh < first_joint);
    }

    #[test]
    fn writes_optional_streams_per_mesh() {
        let path = tmp_path("gvg_dae_optional");
        let meshes = vec![mesh(0, "root", true, true), mesh(1, "arm", false, false)];
        let sections = vec![
            section(0, "root", -1, 0.0, 0.0, 0.0),
            section(1, "arm", 0, 0.0, 1.0, 0.0),
        ];
        write_dae(&path, &meshes, &sections, "pl00_stream000").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(text.contains("semantic=\"NORMAL\""));
        assert!(text.contains("semantic=\"TEXCOORD\""));
    }

    #[test]
    fn dae_export_flips_v_for_collada_coordinates() {
        let path = tmp_path("gvg_dae_uv_export");
        let meshes = vec![mesh(0, "root", true, true)];
        let sections = vec![section(0, "root", -1, 0.0, 0.0, 0.0)];
        write_dae(&path, &meshes, &sections, "pl00_stream000").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert!(
            text.contains(">0.000000 1.000000 1.000000 1.000000 0.000000 0.000000</float_array>")
        );
    }

    #[test]
    fn dae_import_flips_v_back_to_pmf2_coordinates() {
        let xml = r##"<?xml version="1.0" encoding="utf-8"?>
<COLLADA xmlns="http://www.collada.org/2005/11/COLLADASchema" version="1.4.1">
  <library_geometries>
    <geometry id="geom0" name="root">
      <mesh>
        <source id="geom0-positions">
          <float_array id="geom0-positions-array" count="9">0 0 0 1 0 0 0 1 0</float_array>
          <technique_common><accessor source="#geom0-positions-array" count="3" stride="3"/></technique_common>
        </source>
        <source id="geom0-map-0">
          <float_array id="geom0-map-0-array" count="6">0.25 0.25 0.5 0.5 0.75 0.75</float_array>
          <technique_common><accessor source="#geom0-map-0-array" count="3" stride="2"/></technique_common>
        </source>
        <vertices id="geom0-vertices">
          <input semantic="POSITION" source="#geom0-positions"/>
        </vertices>
        <triangles count="1">
          <input semantic="VERTEX" source="#geom0-vertices" offset="0"/>
          <input semantic="TEXCOORD" source="#geom0-map-0" offset="1"/>
          <p>0 0 1 1 2 2</p>
        </triangles>
      </mesh>
    </geometry>
  </library_geometries>
  <library_visual_scenes>
    <visual_scene id="Scene" name="Scene">
      <node id="root" name="root">
        <instance_geometry url="#geom0"/>
      </node>
    </visual_scene>
  </library_visual_scenes>
  <scene><instance_visual_scene url="#Scene"/></scene>
</COLLADA>"##;

        let meta = parse_dae_to_meta_text(xml, "uv_import").unwrap();
        let verts = &meta.bone_meshes[0].local_vertices;
        assert!((verts[0][4] - 0.75).abs() < 1e-6);
        assert!((verts[1][4] - 0.5).abs() < 1e-6);
        assert!((verts[2][4] - 0.25).abs() < 1e-6);
    }

    #[test]
    fn parses_dae_and_rebuilds_pmf2() {
        let path = tmp_path("gvg_dae_parse");
        let meshes = vec![mesh(0, "root", true, true), mesh(1, "arm", true, true)];
        let sections = vec![
            section(0, "root", -1, 0.0, 0.0, 0.0),
            section(1, "arm", 0, 0.0, 1.0, 0.0),
        ];
        write_dae(&path, &meshes, &sections, "pl00_stream000").unwrap();
        let meta = read_dae_to_meta(&path, Some("pl00_stream000")).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(meta.section_count, 2);
        assert_eq!(meta.sections.len(), 2);
        assert!(!meta.bone_meshes.is_empty());
        let rebuilt = crate::pmf2::rebuild_pmf2(&meta);
        let (parsed_meshes, parsed_sections, _, _) =
            crate::pmf2::extract_per_bone_meshes(&rebuilt, false);
        assert_eq!(parsed_sections.len(), 2);
        assert!(!parsed_meshes.is_empty());
    }

    #[test]
    fn uses_vertex_weights_joint_indices_for_binding() {
        let xml = r##"<?xml version="1.0" encoding="utf-8"?>
<COLLADA xmlns="http://www.collada.org/2005/11/COLLADASchema" version="1.4.1">
  <library_geometries>
    <geometry id="geom0" name="geom0">
      <mesh>
        <source id="geom0-positions">
          <float_array id="geom0-positions-array" count="9">0 0 0 1 0 0 0 1 0</float_array>
          <technique_common><accessor source="#geom0-positions-array" count="3" stride="3"/></technique_common>
        </source>
        <vertices id="geom0-vertices">
          <input semantic="POSITION" source="#geom0-positions"/>
        </vertices>
        <triangles count="1">
          <input semantic="VERTEX" source="#geom0-vertices" offset="0"/>
          <p>0 1 2</p>
        </triangles>
      </mesh>
    </geometry>
  </library_geometries>
  <library_controllers>
    <controller id="ctrl0">
      <skin source="#geom0">
        <source id="ctrl0-joints">
          <Name_array id="ctrl0-joints-array" count="2">root arm</Name_array>
          <technique_common><accessor source="#ctrl0-joints-array" count="2" stride="1"/></technique_common>
        </source>
        <source id="ctrl0-bind">
          <float_array id="ctrl0-bind-array" count="32">1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1 1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1</float_array>
          <technique_common><accessor source="#ctrl0-bind-array" count="2" stride="16"/></technique_common>
        </source>
        <source id="ctrl0-weights">
          <float_array id="ctrl0-weights-array" count="1">1</float_array>
          <technique_common><accessor source="#ctrl0-weights-array" count="1" stride="1"/></technique_common>
        </source>
        <joints>
          <input semantic="JOINT" source="#ctrl0-joints"/>
          <input semantic="INV_BIND_MATRIX" source="#ctrl0-bind"/>
        </joints>
        <vertex_weights count="3">
          <input semantic="JOINT" source="#ctrl0-joints" offset="0"/>
          <input semantic="WEIGHT" source="#ctrl0-weights" offset="1"/>
          <vcount>1 1 1</vcount>
          <v>1 0 1 0 1 0</v>
        </vertex_weights>
      </skin>
    </controller>
  </library_controllers>
  <library_visual_scenes>
    <visual_scene id="Scene" name="Scene">
      <node id="mesh0" name="mesh0">
        <instance_controller url="#ctrl0"><skeleton>#joint_1_arm</skeleton></instance_controller>
      </node>
      <node id="joint_0_root" name="root" type="JOINT">
        <matrix>1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1</matrix>
        <node id="joint_1_arm" name="arm" type="JOINT">
          <matrix>1 0 0 0 0 1 0 0 0 0 1 0 0 1 0 1</matrix>
        </node>
      </node>
    </visual_scene>
  </library_visual_scenes>
  <scene><instance_visual_scene url="#Scene"/></scene>
</COLLADA>"##;

        let meta = parse_dae_to_meta_text(xml, "weights_test").unwrap();
        assert_eq!(meta.bone_meshes.len(), 1);
        assert_eq!(meta.bone_meshes[0].bone_index, 1);
    }

    #[test]
    fn controller_binding_uses_geometry_bone_name_when_weights_are_absent() {
        let xml = r##"<?xml version="1.0" encoding="utf-8"?>
<COLLADA xmlns="http://www.collada.org/2005/11/COLLADASchema" version="1.4.1">
  <library_geometries>
    <geometry id="geom_m02" name="pl0a_m02_0">
      <mesh>
        <source id="geom_m02-positions">
          <float_array id="geom_m02-positions-array" count="9">0 0 0 1 0 0 0 1 0</float_array>
          <technique_common><accessor source="#geom_m02-positions-array" count="3" stride="3"/></technique_common>
        </source>
        <vertices id="geom_m02-vertices">
          <input semantic="POSITION" source="#geom_m02-positions"/>
        </vertices>
        <triangles count="1">
          <input semantic="VERTEX" source="#geom_m02-vertices" offset="0"/>
          <p>0 1 2</p>
        </triangles>
      </mesh>
    </geometry>
    <geometry id="geom_m11" name="pl0a_m11_5">
      <mesh>
        <source id="geom_m11-positions">
          <float_array id="geom_m11-positions-array" count="9">0 0 0 1 0 0 0 1 0</float_array>
          <technique_common><accessor source="#geom_m11-positions-array" count="3" stride="3"/></technique_common>
        </source>
        <vertices id="geom_m11-vertices">
          <input semantic="POSITION" source="#geom_m11-positions"/>
        </vertices>
        <triangles count="1">
          <input semantic="VERTEX" source="#geom_m11-vertices" offset="0"/>
          <p>0 1 2</p>
        </triangles>
      </mesh>
    </geometry>
  </library_geometries>
  <library_controllers>
    <controller id="ctrl_m02">
      <skin source="#geom_m02">
        <source id="ctrl_m02-joints">
          <Name_array id="ctrl_m02-joints-array" count="1">pl0a_m11</Name_array>
          <technique_common><accessor source="#ctrl_m02-joints-array" count="1" stride="1"/></technique_common>
        </source>
        <source id="ctrl_m02-weights">
          <float_array id="ctrl_m02-weights-array" count="1">1</float_array>
          <technique_common><accessor source="#ctrl_m02-weights-array" count="1" stride="1"/></technique_common>
        </source>
        <joints><input semantic="JOINT" source="#ctrl_m02-joints"/></joints>
      </skin>
    </controller>
    <controller id="ctrl_m11">
      <skin source="#geom_m11">
        <source id="ctrl_m11-joints">
          <Name_array id="ctrl_m11-joints-array" count="1">pl0a_m11</Name_array>
          <technique_common><accessor source="#ctrl_m11-joints-array" count="1" stride="1"/></technique_common>
        </source>
        <source id="ctrl_m11-weights">
          <float_array id="ctrl_m11-weights-array" count="1">1</float_array>
          <technique_common><accessor source="#ctrl_m11-weights-array" count="1" stride="1"/></technique_common>
        </source>
        <joints><input semantic="JOINT" source="#ctrl_m11-joints"/></joints>
        <vertex_weights count="3">
          <input semantic="JOINT" source="#ctrl_m11-joints" offset="0"/>
          <input semantic="WEIGHT" source="#ctrl_m11-weights" offset="1"/>
          <vcount>1 1 1</vcount>
          <v>0 0 0 0 0 0</v>
        </vertex_weights>
      </skin>
    </controller>
  </library_controllers>
  <library_visual_scenes>
    <visual_scene id="Scene" name="Scene">
      <node id="VisualSceneNode65" name="pl0a_m11_5">
        <instance_controller url="#ctrl_m02"/>
        <instance_controller url="#ctrl_m11"/>
      </node>
      <node id="joint_2_pl0a_m02" name="pl0a_m02" type="JOINT">
        <matrix>1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1</matrix>
      </node>
      <node id="joint_11_pl0a_m11" name="pl0a_m11" type="JOINT">
        <matrix>1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1</matrix>
      </node>
    </visual_scene>
  </library_visual_scenes>
  <scene><instance_visual_scene url="#Scene"/></scene>
</COLLADA>"##;

        let meta = parse_dae_to_meta_text(xml, "geometry_binding").unwrap();
        let face_counts = meta
            .bone_meshes
            .iter()
            .map(|mesh| (mesh.bone_name.as_str(), mesh.face_count))
            .collect::<HashMap<_, _>>();

        assert_eq!(face_counts.get("pl0a_m02"), Some(&1));
        assert_eq!(face_counts.get("pl0a_m11"), Some(&1));
    }

    #[test]
    fn controller_binding_prefers_vertex_weights_over_geometry_name() {
        let xml = r##"<?xml version="1.0" encoding="utf-8"?>
<COLLADA xmlns="http://www.collada.org/2005/11/COLLADASchema" version="1.4.1">
  <library_geometries>
    <geometry id="geom0" name="pl0a_m02_2">
      <mesh>
        <source id="geom0-positions">
          <float_array id="geom0-positions-array" count="9">0 0 0 1 0 0 0 1 0</float_array>
          <technique_common><accessor source="#geom0-positions-array" count="3" stride="3"/></technique_common>
        </source>
        <vertices id="geom0-vertices">
          <input semantic="POSITION" source="#geom0-positions"/>
        </vertices>
        <triangles count="1">
          <input semantic="VERTEX" source="#geom0-vertices" offset="0"/>
          <p>0 1 2</p>
        </triangles>
      </mesh>
    </geometry>
  </library_geometries>
  <library_controllers>
    <controller id="ctrl0">
      <skin source="#geom0">
        <source id="ctrl0-joints">
          <Name_array id="ctrl0-joints-array" count="2">pl0a_m02 pl0a_m11</Name_array>
          <technique_common><accessor source="#ctrl0-joints-array" count="2" stride="1"/></technique_common>
        </source>
        <source id="ctrl0-weights">
          <float_array id="ctrl0-weights-array" count="1">1</float_array>
          <technique_common><accessor source="#ctrl0-weights-array" count="1" stride="1"/></technique_common>
        </source>
        <joints><input semantic="JOINT" source="#ctrl0-joints"/></joints>
        <vertex_weights count="3">
          <input semantic="JOINT" source="#ctrl0-joints" offset="0"/>
          <input semantic="WEIGHT" source="#ctrl0-weights" offset="1"/>
          <vcount>1 1 1</vcount>
          <v>1 0 1 0 1 0</v>
        </vertex_weights>
      </skin>
    </controller>
  </library_controllers>
  <library_visual_scenes>
    <visual_scene id="Scene" name="Scene">
      <node id="VisualSceneNode112" name="pl0a_m02_2">
        <instance_controller url="#ctrl0"/>
      </node>
      <node id="joint_2_pl0a_m02" name="pl0a_m02" type="JOINT">
        <matrix>1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1</matrix>
      </node>
      <node id="joint_11_pl0a_m11" name="pl0a_m11" type="JOINT">
        <matrix>1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1</matrix>
      </node>
    </visual_scene>
  </library_visual_scenes>
  <scene><instance_visual_scene url="#Scene"/></scene>
</COLLADA>"##;

        let meta = parse_dae_to_meta_text(xml, "weighted_geometry_binding").unwrap();

        assert_eq!(meta.bone_meshes.len(), 1);
        assert_eq!(meta.bone_meshes[0].bone_name, "pl0a_m11");
        assert_eq!(meta.bone_meshes[0].face_count, 1);
    }

    #[test]
    fn computed_bbox_has_i16_safety_margin() {
        let xml = r##"<?xml version="1.0" encoding="utf-8"?>
<COLLADA xmlns="http://www.collada.org/2005/11/COLLADASchema" version="1.4.1">
  <library_geometries>
    <geometry id="geom0" name="geom0">
      <mesh>
        <source id="geom0-positions">
          <float_array id="geom0-positions-array" count="9">2 0 0 -1 0 0 0 0 3</float_array>
          <technique_common><accessor source="#geom0-positions-array" count="3" stride="3"/></technique_common>
        </source>
        <vertices id="geom0-vertices">
          <input semantic="POSITION" source="#geom0-positions"/>
        </vertices>
        <triangles count="1">
          <input semantic="VERTEX" source="#geom0-vertices" offset="0"/>
          <p>0 1 2</p>
        </triangles>
      </mesh>
    </geometry>
  </library_geometries>
  <library_visual_scenes>
    <visual_scene id="Scene" name="Scene">
      <node id="mesh0" name="mesh0">
        <instance_geometry url="#geom0"/>
      </node>
    </visual_scene>
  </library_visual_scenes>
  <scene><instance_visual_scene url="#Scene"/></scene>
</COLLADA>"##;

        let meta = parse_dae_to_meta_text(xml, "bbox_margin").unwrap();
        assert!(meta.bbox[0] > 2.0);
        assert!(meta.bbox[1] > 3.0);
        assert!(meta.bbox[2] >= 1.0);
    }
}

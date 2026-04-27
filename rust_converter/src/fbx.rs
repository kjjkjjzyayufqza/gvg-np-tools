use crate::pmf2::{BoneMeshData, BoneSection};
use fbxcel::low::{v7400::ArrayAttributeEncoding, FbxVersion};
use fbxcel::writer::v7400::binary::{FbxFooter, Writer};
use std::collections::HashMap;
use std::io::Seek;
use std::path::Path;

const S_MAT: [f32; 16] = [
    1., 0., 0., 0., 0., 0., -1., 0., 0., 1., 0., 0., 0., 0., 0., 1.,
];
const S_INV: [f32; 16] = [
    1., 0., 0., 0., 0., 0., 1., 0., 0., -1., 0., 0., 0., 0., 0., 1.,
];
const IDENT: [f64; 16] = [
    1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
];

fn m4mul(a: &[f32], b: &[f32]) -> [f32; 16] {
    let mut r = [0f32; 16];
    for i in 0..4 {
        for j in 0..4 {
            r[i * 4 + j] = (0..4).map(|k| a[i * 4 + k] * b[k * 4 + j]).sum();
        }
    }
    r
}
fn to_fbx(wm: &[f32]) -> [f64; 16] {
    let r = m4mul(&m4mul(&S_INV, wm), &S_MAT);
    r.map(|v| v as f64)
}
fn decompose_trs(m: &[f64; 16]) -> ([f64; 3], [f64; 3], [f64; 3]) {
    let t = [m[12], m[13], m[14]];
    let sx = (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt().max(1e-10);
    let sy = (m[4] * m[4] + m[5] * m[5] + m[6] * m[6]).sqrt().max(1e-10);
    let sz = (m[8] * m[8] + m[9] * m[9] + m[10] * m[10])
        .sqrt()
        .max(1e-10);
    let (r00, r01, r02) = (m[0] / sx, m[1] / sx, m[2] / sx);
    let (r12, r22) = (m[6] / sy, m[10] / sz);
    let beta = (-r02).clamp(-1., 1.).asin();
    let (alpha, gamma) = if beta.cos().abs() > 1e-6 {
        (r12.atan2(r22), r01.atan2(r00))
    } else {
        let (r10, r11) = (m[4] / sy, m[5] / sy);
        ((-r10).atan2(r11), 0.0)
    };
    (
        t,
        [alpha.to_degrees(), beta.to_degrees(), gamma.to_degrees()],
        [sx, sy, sz],
    )
}

fn enc() -> Option<ArrayAttributeEncoding> {
    Some(ArrayAttributeEncoding::Zlib)
}

pub fn write_fbx(
    path: &Path,
    bone_meshes: &[BoneMeshData],
    sections: &[BoneSection],
    world_mats: &HashMap<usize, Vec<f32>>,
    _model_name: &str,
) -> std::io::Result<()> {
    let mut w = Writer::new(std::io::Cursor::new(Vec::new()), FbxVersion::V7_5).map_err(io_err)?;

    let has_uv = bone_meshes.iter().any(|bm| bm.has_uv);
    let has_nrm = bone_meshes.iter().any(|bm| bm.has_normals);

    let mut all_verts = Vec::new();
    let mut all_poly = Vec::new();
    let mut pv_nrm = Vec::new();
    let mut pv_uv = Vec::new();
    let mut bone_vr: HashMap<usize, (usize, usize)> = HashMap::new();
    let mut vo = 0usize;

    for bm in bone_meshes {
        let start = vo;
        for pv in &bm.vertices {
            all_verts.push(pv.x as f64);
            all_verts.push(pv.y as f64);
            all_verts.push(pv.z as f64);
        }
        for &(a, b, c) in &bm.faces {
            all_poly.push((a + vo) as i32);
            all_poly.push((b + vo) as i32);
            all_poly.push(-((c + vo) as i32) - 1);
            if has_nrm {
                for &i in &[a, b, c] {
                    let v = &bm.vertices[i];
                    pv_nrm.push(v.nx as f64);
                    pv_nrm.push(v.ny as f64);
                    pv_nrm.push(v.nz as f64);
                }
            }
            if has_uv {
                for &i in &[a, b, c] {
                    let v = &bm.vertices[i];
                    pv_uv.push(v.u as f64);
                    pv_uv.push(1.0 - v.v as f64);
                }
            }
        }
        bone_vr.insert(bm.bone_index, (start, start + bm.vertices.len()));
        vo += bm.vertices.len();
    }

    let fbx_w: HashMap<usize, [f64; 16]> =
        world_mats.iter().map(|(&k, v)| (k, to_fbx(v))).collect();

    let mut nid = 2_000_000i64;
    let mut next_id = || {
        nid += 1;
        nid
    };
    let geom_id = next_id();
    let mesh_id = next_id();
    let mat_id = next_id();
    let skin_id = next_id();
    let pose_id = next_id();
    let bone_mid: HashMap<usize, i64> = sections.iter().map(|s| (s.index, next_id())).collect();
    let bone_aid: HashMap<usize, i64> = sections.iter().map(|s| (s.index, next_id())).collect();
    let cluster_id: HashMap<usize, i64> = bone_meshes
        .iter()
        .map(|bm| (bm.bone_index, next_id()))
        .collect();

    // FBXHeaderExtension
    w.new_node("FBXHeaderExtension").map_err(io_err)?;
    {
        let mut a = w.new_node("FBXVersion").map_err(io_err)?;
        a.append_i32(7500).map_err(io_err)?;
    }
    w.close_node().map_err(io_err)?;
    {
        let mut a = w.new_node("Creator").map_err(io_err)?;
        a.append_string_direct("gvg_converter").map_err(io_err)?;
    }
    w.close_node().map_err(io_err)?;
    w.close_node().map_err(io_err)?;

    // GlobalSettings
    w.new_node("GlobalSettings").map_err(io_err)?;
    {
        let mut a = w.new_node("Version").map_err(io_err)?;
        a.append_i32(1000).map_err(io_err)?;
    }
    w.close_node().map_err(io_err)?;
    w.new_node("Properties70").map_err(io_err)?;
    write_prop_int(&mut w, "UpAxis", 1)?;
    write_prop_int(&mut w, "UpAxisSign", 1)?;
    write_prop_int(&mut w, "FrontAxis", 2)?;
    write_prop_int(&mut w, "FrontAxisSign", 1)?;
    write_prop_int(&mut w, "CoordAxis", 0)?;
    write_prop_int(&mut w, "CoordAxisSign", 1)?;
    write_prop_int(&mut w, "OriginalUpAxis", -1)?;
    write_prop_int(&mut w, "OriginalUpAxisSign", 1)?;
    write_prop_double(&mut w, "UnitScaleFactor", 1.0)?;
    write_prop_double(&mut w, "OriginalUnitScaleFactor", 1.0)?;
    w.close_node().map_err(io_err)?;
    w.close_node().map_err(io_err)?;

    // Documents
    w.new_node("Documents").map_err(io_err)?;
    {
        let mut a = w.new_node("Count").map_err(io_err)?;
        a.append_i32(1).map_err(io_err)?;
    }
    w.close_node().map_err(io_err)?;
    {
        let mut a = w.new_node("Document").map_err(io_err)?;
        a.append_i64(1_000_000_000).map_err(io_err)?;
        a.append_string_direct("").map_err(io_err)?;
        a.append_string_direct("Scene").map_err(io_err)?;
    }
    w.new_node("Properties70").map_err(io_err)?;
    w.close_node().map_err(io_err)?;
    {
        let mut a = w.new_node("RootNode").map_err(io_err)?;
        a.append_i64(0).map_err(io_err)?;
    }
    w.close_node().map_err(io_err)?;
    w.close_node().map_err(io_err)?;
    w.close_node().map_err(io_err)?;

    w.new_node("References").map_err(io_err)?;
    w.close_node().map_err(io_err)?;

    // Definitions
    let n_cluster = bone_meshes.len();
    w.new_node("Definitions").map_err(io_err)?;
    {
        let mut a = w.new_node("Version").map_err(io_err)?;
        a.append_i32(100).map_err(io_err)?;
    }
    w.close_node().map_err(io_err)?;
    write_def(&mut w, "GlobalSettings", 1)?;
    write_def(&mut w, "Model", 1 + sections.len() as i32)?;
    write_def(&mut w, "Geometry", 1)?;
    write_def(&mut w, "Material", 1)?;
    write_def(&mut w, "NodeAttribute", sections.len() as i32)?;
    write_def(&mut w, "Deformer", 1 + n_cluster as i32)?;
    write_def(&mut w, "Pose", 1)?;
    w.close_node().map_err(io_err)?;

    // Objects
    w.new_node("Objects").map_err(io_err)?;

    // --- Geometry ---
    {
        let mut a = w.new_node("Geometry").map_err(io_err)?;
        a.append_i64(geom_id).map_err(io_err)?;
        a.append_string_direct("Geometry::mesh").map_err(io_err)?;
        a.append_string_direct("Mesh").map_err(io_err)?;
    }
    {
        let mut a = w.new_node("GeometryVersion").map_err(io_err)?;
        a.append_i32(124).map_err(io_err)?;
    }
    w.close_node().map_err(io_err)?;
    {
        let mut a = w.new_node("Vertices").map_err(io_err)?;
        a.append_arr_f64_from_iter(enc(), all_verts.iter().copied())
            .map_err(io_err)?;
    }
    w.close_node().map_err(io_err)?;
    {
        let mut a = w.new_node("PolygonVertexIndex").map_err(io_err)?;
        a.append_arr_i32_from_iter(enc(), all_poly.iter().copied())
            .map_err(io_err)?;
    }
    w.close_node().map_err(io_err)?;

    if has_nrm {
        w.new_node("LayerElementNormal").map_err(io_err)?;
        {
            let mut a = w.new_node("Version").map_err(io_err)?;
            a.append_i32(102).map_err(io_err)?;
        }
        w.close_node().map_err(io_err)?;
        {
            let mut a = w.new_node("Name").map_err(io_err)?;
            a.append_string_direct("").map_err(io_err)?;
        }
        w.close_node().map_err(io_err)?;
        {
            let mut a = w.new_node("MappingInformationType").map_err(io_err)?;
            a.append_string_direct("ByPolygonVertex").map_err(io_err)?;
        }
        w.close_node().map_err(io_err)?;
        {
            let mut a = w.new_node("ReferenceInformationType").map_err(io_err)?;
            a.append_string_direct("Direct").map_err(io_err)?;
        }
        w.close_node().map_err(io_err)?;
        {
            let mut a = w.new_node("Normals").map_err(io_err)?;
            a.append_arr_f64_from_iter(enc(), pv_nrm.iter().copied())
                .map_err(io_err)?;
        }
        w.close_node().map_err(io_err)?;
        w.close_node().map_err(io_err)?;
    }
    if has_uv {
        w.new_node("LayerElementUV").map_err(io_err)?;
        {
            let mut a = w.new_node("Version").map_err(io_err)?;
            a.append_i32(101).map_err(io_err)?;
        }
        w.close_node().map_err(io_err)?;
        {
            let mut a = w.new_node("Name").map_err(io_err)?;
            a.append_string_direct("UVMap").map_err(io_err)?;
        }
        w.close_node().map_err(io_err)?;
        {
            let mut a = w.new_node("MappingInformationType").map_err(io_err)?;
            a.append_string_direct("ByPolygonVertex").map_err(io_err)?;
        }
        w.close_node().map_err(io_err)?;
        {
            let mut a = w.new_node("ReferenceInformationType").map_err(io_err)?;
            a.append_string_direct("Direct").map_err(io_err)?;
        }
        w.close_node().map_err(io_err)?;
        {
            let mut a = w.new_node("UV").map_err(io_err)?;
            a.append_arr_f64_from_iter(enc(), pv_uv.iter().copied())
                .map_err(io_err)?;
        }
        w.close_node().map_err(io_err)?;
        w.close_node().map_err(io_err)?;
    }
    {
        w.new_node("LayerElementMaterial").map_err(io_err)?;
        {
            let mut a = w.new_node("Version").map_err(io_err)?;
            a.append_i32(101).map_err(io_err)?;
        }
        w.close_node().map_err(io_err)?;
        {
            let mut a = w.new_node("Name").map_err(io_err)?;
            a.append_string_direct("").map_err(io_err)?;
        }
        w.close_node().map_err(io_err)?;
        {
            let mut a = w.new_node("MappingInformationType").map_err(io_err)?;
            a.append_string_direct("AllSame").map_err(io_err)?;
        }
        w.close_node().map_err(io_err)?;
        {
            let mut a = w.new_node("ReferenceInformationType").map_err(io_err)?;
            a.append_string_direct("IndexToDirect").map_err(io_err)?;
        }
        w.close_node().map_err(io_err)?;
        {
            let mut a = w.new_node("Materials").map_err(io_err)?;
            a.append_arr_i32_from_iter(None, [0].iter().copied())
                .map_err(io_err)?;
        }
        w.close_node().map_err(io_err)?;
        w.close_node().map_err(io_err)?;
    }
    w.new_node("Layer").map_err(io_err)?;
    {
        let mut a = w.new_node("Version").map_err(io_err)?;
        a.append_i32(100).map_err(io_err)?;
    }
    w.close_node().map_err(io_err)?;
    if has_nrm {
        write_layer_elem(&mut w, "LayerElementNormal")?;
    }
    write_layer_elem(&mut w, "LayerElementMaterial")?;
    if has_uv {
        write_layer_elem(&mut w, "LayerElementUV")?;
    }
    w.close_node().map_err(io_err)?;
    w.close_node().map_err(io_err)?; // end Geometry

    // --- Mesh Model ---
    {
        let mut a = w.new_node("Model").map_err(io_err)?;
        a.append_i64(mesh_id).map_err(io_err)?;
        a.append_string_direct("Model::mesh").map_err(io_err)?;
        a.append_string_direct("Mesh").map_err(io_err)?;
    }
    {
        let mut a = w.new_node("Version").map_err(io_err)?;
        a.append_i32(232).map_err(io_err)?;
    }
    w.close_node().map_err(io_err)?;
    w.new_node("Properties70").map_err(io_err)?;
    write_prop_int(&mut w, "DefaultAttributeIndex", 0)?;
    w.close_node().map_err(io_err)?;
    {
        let mut a = w.new_node("Shading").map_err(io_err)?;
        a.append_bool(true).map_err(io_err)?;
    }
    w.close_node().map_err(io_err)?;
    {
        let mut a = w.new_node("Culling").map_err(io_err)?;
        a.append_string_direct("CullingOff").map_err(io_err)?;
    }
    w.close_node().map_err(io_err)?;
    w.close_node().map_err(io_err)?;

    // --- Bones ---
    for sec in sections {
        {
            let mut a = w.new_node("NodeAttribute").map_err(io_err)?;
            a.append_i64(bone_aid[&sec.index]).map_err(io_err)?;
            a.append_string_direct(&format!("NodeAttribute::{}", sec.name))
                .map_err(io_err)?;
            a.append_string_direct("LimbNode").map_err(io_err)?;
        }
        {
            let mut a = w.new_node("TypeFlags").map_err(io_err)?;
            a.append_string_direct("Skeleton").map_err(io_err)?;
        }
        w.close_node().map_err(io_err)?;
        w.close_node().map_err(io_err)?;

        let local_fbx = to_fbx(&sec.local_matrix);
        let (t, r, sc) = decompose_trs(&local_fbx);
        {
            let mut a = w.new_node("Model").map_err(io_err)?;
            a.append_i64(bone_mid[&sec.index]).map_err(io_err)?;
            a.append_string_direct(&format!("Model::{}", sec.name))
                .map_err(io_err)?;
            a.append_string_direct("LimbNode").map_err(io_err)?;
        }
        {
            let mut a = w.new_node("Version").map_err(io_err)?;
            a.append_i32(232).map_err(io_err)?;
        }
        w.close_node().map_err(io_err)?;
        w.new_node("Properties70").map_err(io_err)?;
        write_prop_lcl(&mut w, "Lcl Translation", t)?;
        write_prop_lcl(&mut w, "Lcl Rotation", r)?;
        write_prop_lcl(&mut w, "Lcl Scaling", sc)?;
        w.close_node().map_err(io_err)?;
        w.close_node().map_err(io_err)?;
    }

    // --- Material ---
    {
        let mut a = w.new_node("Material").map_err(io_err)?;
        a.append_i64(mat_id).map_err(io_err)?;
        a.append_string_direct("Material::mat").map_err(io_err)?;
        a.append_string_direct("").map_err(io_err)?;
    }
    {
        let mut a = w.new_node("Version").map_err(io_err)?;
        a.append_i32(102).map_err(io_err)?;
    }
    w.close_node().map_err(io_err)?;
    {
        let mut a = w.new_node("ShadingModel").map_err(io_err)?;
        a.append_string_direct("phong").map_err(io_err)?;
    }
    w.close_node().map_err(io_err)?;
    w.new_node("Properties70").map_err(io_err)?;
    write_prop_color(&mut w, "DiffuseColor", [0.8, 0.8, 0.8])?;
    write_prop_color(&mut w, "AmbientColor", [0.2, 0.2, 0.2])?;
    w.close_node().map_err(io_err)?;
    w.close_node().map_err(io_err)?;

    // --- Skin ---
    {
        let mut a = w.new_node("Deformer").map_err(io_err)?;
        a.append_i64(skin_id).map_err(io_err)?;
        a.append_string_direct("Deformer::Skin").map_err(io_err)?;
        a.append_string_direct("Skin").map_err(io_err)?;
    }
    {
        let mut a = w.new_node("Version").map_err(io_err)?;
        a.append_i32(101).map_err(io_err)?;
    }
    w.close_node().map_err(io_err)?;
    {
        let mut a = w.new_node("Link_DeformAcuracy").map_err(io_err)?;
        a.append_f64(50.0).map_err(io_err)?;
    }
    w.close_node().map_err(io_err)?;
    w.close_node().map_err(io_err)?;

    // --- Clusters ---
    for bm in bone_meshes {
        let (vs, ve) = bone_vr[&bm.bone_index];
        let count = ve - vs;
        let indices: Vec<i32> = (vs..ve).map(|i| i as i32).collect();
        let weights: Vec<f64> = vec![1.0; count];
        let wm = fbx_w.get(&bm.bone_index).copied().unwrap_or(IDENT);

        {
            let mut a = w.new_node("Deformer").map_err(io_err)?;
            a.append_i64(cluster_id[&bm.bone_index]).map_err(io_err)?;
            a.append_string_direct(&format!("SubDeformer::{}", bm.bone_name))
                .map_err(io_err)?;
            a.append_string_direct("Cluster").map_err(io_err)?;
        }
        {
            let mut a = w.new_node("Version").map_err(io_err)?;
            a.append_i32(100).map_err(io_err)?;
        }
        w.close_node().map_err(io_err)?;
        {
            let mut a = w.new_node("Indexes").map_err(io_err)?;
            a.append_arr_i32_from_iter(enc(), indices.into_iter())
                .map_err(io_err)?;
        }
        w.close_node().map_err(io_err)?;
        {
            let mut a = w.new_node("Weights").map_err(io_err)?;
            a.append_arr_f64_from_iter(enc(), weights.into_iter())
                .map_err(io_err)?;
        }
        w.close_node().map_err(io_err)?;
        {
            let mut a = w.new_node("Transform").map_err(io_err)?;
            a.append_arr_f64_from_iter(None, IDENT.iter().copied())
                .map_err(io_err)?;
        }
        w.close_node().map_err(io_err)?;
        {
            let mut a = w.new_node("TransformLink").map_err(io_err)?;
            a.append_arr_f64_from_iter(None, wm.iter().copied())
                .map_err(io_err)?;
        }
        w.close_node().map_err(io_err)?;
        w.close_node().map_err(io_err)?;
    }

    // --- BindPose ---
    {
        let mut a = w.new_node("Pose").map_err(io_err)?;
        a.append_i64(pose_id).map_err(io_err)?;
        a.append_string_direct("Pose::BindPose").map_err(io_err)?;
        a.append_string_direct("BindPose").map_err(io_err)?;
    }
    {
        let mut a = w.new_node("Type").map_err(io_err)?;
        a.append_string_direct("BindPose").map_err(io_err)?;
    }
    w.close_node().map_err(io_err)?;
    {
        let mut a = w.new_node("Version").map_err(io_err)?;
        a.append_i32(100).map_err(io_err)?;
    }
    w.close_node().map_err(io_err)?;
    {
        let mut a = w.new_node("NbPoseNodes").map_err(io_err)?;
        a.append_i32((1 + sections.len()) as i32).map_err(io_err)?;
    }
    w.close_node().map_err(io_err)?;

    w.new_node("PoseNode").map_err(io_err)?;
    {
        let mut a = w.new_node("Node").map_err(io_err)?;
        a.append_i64(mesh_id).map_err(io_err)?;
    }
    w.close_node().map_err(io_err)?;
    {
        let mut a = w.new_node("Matrix").map_err(io_err)?;
        a.append_arr_f64_from_iter(None, IDENT.iter().copied())
            .map_err(io_err)?;
    }
    w.close_node().map_err(io_err)?;
    w.close_node().map_err(io_err)?;

    for sec in sections {
        let wm = fbx_w.get(&sec.index).copied().unwrap_or(IDENT);
        w.new_node("PoseNode").map_err(io_err)?;
        {
            let mut a = w.new_node("Node").map_err(io_err)?;
            a.append_i64(bone_mid[&sec.index]).map_err(io_err)?;
        }
        w.close_node().map_err(io_err)?;
        {
            let mut a = w.new_node("Matrix").map_err(io_err)?;
            a.append_arr_f64_from_iter(None, wm.iter().copied())
                .map_err(io_err)?;
        }
        w.close_node().map_err(io_err)?;
        w.close_node().map_err(io_err)?;
    }
    w.close_node().map_err(io_err)?; // end Pose

    w.close_node().map_err(io_err)?; // end Objects

    // Connections
    w.new_node("Connections").map_err(io_err)?;
    write_conn(&mut w, "OO", mesh_id, 0)?;
    write_conn(&mut w, "OO", geom_id, mesh_id)?;
    write_conn(&mut w, "OO", mat_id, mesh_id)?;
    write_conn(&mut w, "OO", skin_id, geom_id)?;

    for sec in sections {
        write_conn(&mut w, "OO", bone_aid[&sec.index], bone_mid[&sec.index])?;
        let parent = if sec.parent < 0 {
            0
        } else {
            bone_mid[&(sec.parent as usize)]
        };
        write_conn(&mut w, "OO", bone_mid[&sec.index], parent)?;
    }
    for bm in bone_meshes {
        write_conn(&mut w, "OO", cluster_id[&bm.bone_index], skin_id)?;
        write_conn(
            &mut w,
            "OO",
            bone_mid[&bm.bone_index],
            cluster_id[&bm.bone_index],
        )?;
    }
    w.close_node().map_err(io_err)?;

    let cursor = w
        .finalize_and_flush(&FbxFooter::default())
        .map_err(io_err)?;
    std::fs::write(path, cursor.into_inner())?;
    Ok(())
}

fn write_conn<W: std::io::Write + Seek>(
    w: &mut Writer<W>,
    typ: &str,
    child: i64,
    parent: i64,
) -> std::io::Result<()> {
    let mut a = w.new_node("C").map_err(io_err)?;
    a.append_string_direct(typ).map_err(io_err)?;
    a.append_i64(child).map_err(io_err)?;
    a.append_i64(parent).map_err(io_err)?;
    drop(a);
    w.close_node().map_err(io_err)
}

fn write_prop_int<W: std::io::Write + Seek>(
    w: &mut Writer<W>,
    name: &str,
    val: i32,
) -> std::io::Result<()> {
    let mut a = w.new_node("P").map_err(io_err)?;
    a.append_string_direct(name).map_err(io_err)?;
    a.append_string_direct("int").map_err(io_err)?;
    a.append_string_direct("Integer").map_err(io_err)?;
    a.append_string_direct("").map_err(io_err)?;
    a.append_i32(val).map_err(io_err)?;
    drop(a);
    w.close_node().map_err(io_err)
}

fn write_prop_double<W: std::io::Write + Seek>(
    w: &mut Writer<W>,
    name: &str,
    val: f64,
) -> std::io::Result<()> {
    let mut a = w.new_node("P").map_err(io_err)?;
    a.append_string_direct(name).map_err(io_err)?;
    a.append_string_direct("double").map_err(io_err)?;
    a.append_string_direct("Number").map_err(io_err)?;
    a.append_string_direct("").map_err(io_err)?;
    a.append_f64(val).map_err(io_err)?;
    drop(a);
    w.close_node().map_err(io_err)
}

fn write_prop_lcl<W: std::io::Write + Seek>(
    w: &mut Writer<W>,
    name: &str,
    v: [f64; 3],
) -> std::io::Result<()> {
    let mut a = w.new_node("P").map_err(io_err)?;
    a.append_string_direct(name).map_err(io_err)?;
    a.append_string_direct(name).map_err(io_err)?;
    a.append_string_direct("").map_err(io_err)?;
    a.append_string_direct("A").map_err(io_err)?;
    a.append_f64(v[0]).map_err(io_err)?;
    a.append_f64(v[1]).map_err(io_err)?;
    a.append_f64(v[2]).map_err(io_err)?;
    drop(a);
    w.close_node().map_err(io_err)
}

fn write_prop_color<W: std::io::Write + Seek>(
    w: &mut Writer<W>,
    name: &str,
    c: [f64; 3],
) -> std::io::Result<()> {
    let mut a = w.new_node("P").map_err(io_err)?;
    a.append_string_direct(name).map_err(io_err)?;
    a.append_string_direct("Color").map_err(io_err)?;
    a.append_string_direct("").map_err(io_err)?;
    a.append_string_direct("A").map_err(io_err)?;
    a.append_f64(c[0]).map_err(io_err)?;
    a.append_f64(c[1]).map_err(io_err)?;
    a.append_f64(c[2]).map_err(io_err)?;
    drop(a);
    w.close_node().map_err(io_err)
}

fn write_def<W: std::io::Write + Seek>(
    w: &mut Writer<W>,
    name: &str,
    count: i32,
) -> std::io::Result<()> {
    {
        let mut a = w.new_node("ObjectType").map_err(io_err)?;
        a.append_string_direct(name).map_err(io_err)?;
    }
    {
        let mut a = w.new_node("Count").map_err(io_err)?;
        a.append_i32(count).map_err(io_err)?;
    }
    w.close_node().map_err(io_err)?;
    w.close_node().map_err(io_err)
}

fn write_layer_elem<W: std::io::Write + Seek>(w: &mut Writer<W>, typ: &str) -> std::io::Result<()> {
    w.new_node("LayerElement").map_err(io_err)?;
    {
        let mut a = w.new_node("Type").map_err(io_err)?;
        a.append_string_direct(typ).map_err(io_err)?;
    }
    w.close_node().map_err(io_err)?;
    {
        let mut a = w.new_node("TypedIndex").map_err(io_err)?;
        a.append_i32(0).map_err(io_err)?;
    }
    w.close_node().map_err(io_err)?;
    w.close_node().map_err(io_err)
}

fn io_err<E: std::fmt::Display>(e: E) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
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
        std::env::temp_dir().join(format!("{}_{}_{}.fbx", name, std::process::id(), now))
    }

    #[test]
    fn fbx_binary_names_do_not_embed_separator_bytes() {
        let path = tmp_path("gvg_fbx_name_test");
        let bone_meshes = vec![BoneMeshData {
            bone_index: 0,
            bone_name: "root".to_string(),
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
            local_vertices: Vec::new(),
            has_uv: true,
            has_normals: true,
            vtypes: Vec::new(),
        }];
        let sections = vec![BoneSection {
            index: 0,
            name: "root".to_string(),
            offset: 0,
            size: 0,
            local_matrix: vec![
                1.0, 0.0, 0.0, 0.0, //
                0.0, 1.0, 0.0, 0.0, //
                0.0, 0.0, 1.0, 0.0, //
                0.0, 0.0, 0.0, 1.0,
            ],
            parent: -1,
            has_mesh: true,
            origin_offset: None,
            category: String::new(),
        }];
        let mut world_mats = HashMap::new();
        world_mats.insert(
            0usize,
            vec![
                1.0, 0.0, 0.0, 0.0, //
                0.0, 1.0, 0.0, 0.0, //
                0.0, 0.0, 1.0, 0.0, //
                0.0, 0.0, 0.0, 1.0,
            ],
        );

        write_fbx(&path, &bone_meshes, &sections, &world_mats, "test_model").unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(!bytes
            .windows("Model::mesh\x00\x01Model".len())
            .any(|w| w == b"Model::mesh\x00\x01Model"));
        assert!(!bytes
            .windows("Geometry::\x00\x01Mesh".len())
            .any(|w| w == b"Geometry::\x00\x01Mesh"));
        assert!(!bytes
            .windows("Pose::BindPose\x00\x01Pose".len())
            .any(|w| w == b"Pose::BindPose\x00\x01Pose"));
    }
}

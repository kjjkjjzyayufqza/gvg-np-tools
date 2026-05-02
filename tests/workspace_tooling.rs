use gvg_converter::{
    afs::{patch_entry_bytes, plan_patch_entry, AfsEntry, AfsInventory},
    pmf2::{BoneMeshData, ParsedVertex},
    pzz::{build_pzz, build_pzz_with_tail, compute_pzz_tail, inspect_pzz},
    render::{Pmf2PreviewMesh, PreviewCamera, PreviewViewport, PreviewVisibility},
    save::{AfsSavePlanner, PzzSavePlanner},
    texture::{GimImage, PixelFormat},
    workspace::{AssetKind, ModWorkspace},
};

fn make_afs(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let file_count = entries.len();
    let name_table_pos = 8 + file_count * 8;
    let names_start = name_table_pos + 8;
    let name_table_size = file_count * 0x30;
    let mut data_offset = align_up(names_start + name_table_size, 2048);
    let mut data = vec![0u8; data_offset];
    data[0..4].copy_from_slice(b"AFS\0");
    data[4..8].copy_from_slice(&(file_count as u32).to_le_bytes());
    data[name_table_pos..name_table_pos + 4]
        .copy_from_slice((names_start as u32).to_le_bytes().as_slice());
    data[name_table_pos + 4..name_table_pos + 8]
        .copy_from_slice((name_table_size as u32).to_le_bytes().as_slice());
    for (index, (name, payload)) in entries.iter().enumerate() {
        let table_pos = 8 + index * 8;
        data[table_pos..table_pos + 4].copy_from_slice(&(data_offset as u32).to_le_bytes());
        data[table_pos + 4..table_pos + 8].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        let name_pos = names_start + index * 0x30;
        let name_bytes = name.as_bytes();
        data[name_pos..name_pos + name_bytes.len()].copy_from_slice(name_bytes);
        data[name_pos + 0x2C..name_pos + 0x30]
            .copy_from_slice(&(payload.len() as u32).to_le_bytes());
        data.resize(data_offset + align_up(payload.len(), 2048), 0);
        data[data_offset..data_offset + payload.len()].copy_from_slice(payload);
        data_offset += align_up(payload.len(), 2048);
    }
    data
}

fn make_afs_without_names(entries: &[&[u8]]) -> Vec<u8> {
    let file_count = entries.len();
    let name_table_pos = 8 + file_count * 8;
    let mut data_offset = align_up(name_table_pos + 8, 2048);
    let mut data = vec![0u8; data_offset];
    data[0..4].copy_from_slice(b"AFS\0");
    data[4..8].copy_from_slice(&(file_count as u32).to_le_bytes());
    for (index, payload) in entries.iter().enumerate() {
        let table_pos = 8 + index * 8;
        data[table_pos..table_pos + 4].copy_from_slice(&(data_offset as u32).to_le_bytes());
        data[table_pos + 4..table_pos + 8].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        data.resize(data_offset + align_up(payload.len(), 2048), 0);
        data[data_offset..data_offset + payload.len()].copy_from_slice(payload);
        data_offset += align_up(payload.len(), 2048);
    }
    data
}

fn align_up(value: usize, alignment: usize) -> usize {
    (value + alignment - 1) & !(alignment - 1)
}

fn make_gim_rgba8888() -> Vec<u8> {
    let mut data = vec![0u8; 0x10 + 0x10 + 0x30 + 4];
    data[0..11].copy_from_slice(b"MIG.00.1PSP");
    let block = 0x10;
    data[block..block + 2].copy_from_slice(&0x04u16.to_le_bytes());
    data[block + 4..block + 8].copy_from_slice(&0x44u32.to_le_bytes());
    data[block + 8..block + 12].copy_from_slice(&0x44u32.to_le_bytes());
    data[block + 12..block + 16].copy_from_slice(&0x10u32.to_le_bytes());
    let image = block + 0x10;
    data[image + 0x04..image + 0x06].copy_from_slice(&0x03u16.to_le_bytes());
    data[image + 0x06..image + 0x08].copy_from_slice(&0u16.to_le_bytes());
    data[image + 0x08..image + 0x0A].copy_from_slice(&1u16.to_le_bytes());
    data[image + 0x0A..image + 0x0C].copy_from_slice(&1u16.to_le_bytes());
    data[image + 0x1C..image + 0x20].copy_from_slice(&0x30u32.to_le_bytes());
    data[image + 0x30..image + 0x34].copy_from_slice(&[10, 20, 30, 255]);
    data
}

fn make_pzz_with_non_stream_chunk() -> Vec<u8> {
    let key = 0x1234_5678u32;
    let stream = b"PMF2_original".to_vec();
    let compressed = gvg_converter::pzz::compress_stream(&stream);
    let mut stream_chunk = Vec::new();
    stream_chunk.extend_from_slice(&(compressed.len() as u32).to_be_bytes());
    stream_chunk.extend_from_slice(&(stream.len() as u32).to_be_bytes());
    stream_chunk.extend_from_slice(&compressed);
    stream_chunk.resize(128, 0);

    let raw_chunk = vec![0xAB; 128];
    let descriptors = [0x4000_0001u32, 0x0000_0001u32];
    let data_start = 0x800usize;
    let mut decrypted = vec![0u8; data_start + stream_chunk.len() + raw_chunk.len()];
    decrypted[0..4].copy_from_slice(&(descriptors.len() as u32).to_le_bytes());
    for (index, descriptor) in descriptors.iter().enumerate() {
        decrypted[4 + index * 4..8 + index * 4].copy_from_slice(&descriptor.to_le_bytes());
    }
    decrypted[data_start..data_start + stream_chunk.len()].copy_from_slice(&stream_chunk);
    decrypted[data_start + stream_chunk.len()..].copy_from_slice(&raw_chunk);
    gvg_converter::pzz::xor_decrypt(&decrypted, key)
}

#[test]
fn library_modules_are_available_to_gui_and_cli() {
    let pzz = build_pzz(&[b"PMF2 model".to_vec()], 0x1234_5678);
    let afs = make_afs(&[("pl00.pzz", &pzz)]);
    let workspace = ModWorkspace::open_afs_bytes("Z_DATA.BIN", afs).unwrap();

    assert_eq!(workspace.afs_entries().len(), 1);
    assert_eq!(workspace.afs_entries()[0].kind, AssetKind::Pzz);
}

#[test]
fn workspace_tracks_stream_replacements_and_new_streams() {
    let original_pzz = build_pzz_with_tail(&[b"PMF2 original".to_vec()], 0x1234_5678, true);
    let info = inspect_pzz(&original_pzz).unwrap();
    assert_eq!(info.stream_count, 1);
    let mut workspace = ModWorkspace::open_pzz_bytes("pl00.pzz", original_pzz).unwrap();

    workspace
        .replace_stream(0, b"PMF2 replacement".to_vec())
        .unwrap();

    let pzz = workspace.open_pzz().unwrap();
    assert!(pzz.is_dirty());
    assert_eq!(pzz.streams().len(), 1);
    assert_eq!(pzz.streams()[0].kind, AssetKind::Pmf2);
}

#[test]
fn workspace_rejects_pzz_without_strict_layout() {
    let compressed = gvg_converter::pzz::compress_stream(b"PMF2 orphan stream");
    let encrypted = gvg_converter::pzz::xor_decrypt(&compressed, 0x1234_5678);

    assert!(ModWorkspace::open_pzz_bytes("broken.pzz", encrypted).is_err());
}

#[test]
fn workspace_opens_pzz_entries_without_losing_afs_context() {
    let pzz = build_pzz(&[b"PMF2 model".to_vec()], 0x1234_5678);
    let afs = make_afs(&[("pl00.pzz", &pzz), ("readme.bin", b"abcd")]);
    let mut workspace = ModWorkspace::open_afs_bytes("Z_DATA.BIN", afs).expect("AFS should parse");

    workspace.open_pzz_entry(0).expect("PZZ entry should open");

    assert_eq!(workspace.afs_entries().len(), 2);
    let pzz_workspace = workspace.open_pzz().unwrap();
    assert_eq!(pzz_workspace.name(), "pl00.pzz");
    assert_eq!(pzz_workspace.streams().len(), 1);
}

#[test]
fn workspace_without_name_table_marks_entries_as_raw() {
    let pzz = build_pzz(&[b"PMF2 model".to_vec()], 0x1234_5678);
    let afs = make_afs_without_names(&[&pzz, b"abcd"]);
    let workspace = ModWorkspace::open_afs_bytes("Z_DATA.BIN", afs).unwrap();

    assert_eq!(workspace.afs_entries()[0].kind, AssetKind::Raw);
    assert_eq!(workspace.afs_entries()[1].kind, AssetKind::Raw);
}

#[test]
fn afs_patch_bytes_updates_tables_and_name_size_mirror() {
    let afs = make_afs(&[("pl00.pzz", b"old"), ("other.bin", b"abcd")]);
    let patched = patch_entry_bytes(&afs, 0, b"new replacement").unwrap();
    let inventory = gvg_converter::afs::scan_inventory(&patched, None).unwrap();

    assert_eq!(inventory.entries[0].size, b"new replacement".len());
    assert_eq!(inventory.entries[1].offset % 2048, 0);
    let file_count = u32::from_le_bytes([patched[4], patched[5], patched[6], patched[7]]) as usize;
    let name_table_pos = 8 + file_count * 8;
    let name_offset = u32::from_le_bytes([
        patched[name_table_pos],
        patched[name_table_pos + 1],
        patched[name_table_pos + 2],
        patched[name_table_pos + 3],
    ]) as usize;
    let mirrored_size = u32::from_le_bytes([
        patched[name_offset + 0x2C],
        patched[name_offset + 0x2D],
        patched[name_offset + 0x2E],
        patched[name_offset + 0x2F],
    ]) as usize;
    assert_eq!(mirrored_size, b"new replacement".len());
}

#[test]
fn afs_patch_rejects_invalid_or_truncated_sources() {
    let mut invalid_magic = make_afs(&[("pl00.pzz", b"old")]);
    invalid_magic[0..4].copy_from_slice(b"BAD!");
    assert!(patch_entry_bytes(&invalid_magic, 0, b"new")
        .unwrap_err()
        .to_string()
        .contains("unsupported AFS magic"));

    let truncated = make_afs(&[("pl00.pzz", b"old")])[..12].to_vec();
    assert!(patch_entry_bytes(&truncated, 0, b"new").is_err());
}

#[test]
fn save_planners_report_pzz_tail_and_afs_alignment_changes() {
    let original_pzz = build_pzz_with_tail(&[b"PMF2 original".to_vec()], 0x1234_5678, true);
    let mut state = 0x1234_5678u32;
    let mut large_replacement = (0..12000)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 16) as u8
        })
        .collect::<Vec<_>>();
    large_replacement[0..4].copy_from_slice(b"PMF2");
    let replacement_streams = vec![large_replacement];
    let pzz_plan = PzzSavePlanner::new(&original_pzz, replacement_streams.clone())
        .plan_preserving_layout()
        .unwrap();

    assert_eq!(pzz_plan.stream_count, 1);
    assert!(pzz_plan.tail_recomputed);
    assert_eq!(
        pzz_plan.rebuilt_tail,
        Some(compute_pzz_tail(&pzz_plan.decrypted_body))
    );

    let afs = make_afs(&[("pl00.pzz", &original_pzz), ("other.bin", b"abcd")]);
    let afs_plan = plan_patch_entry(&afs, 0, pzz_plan.rebuilt_pzz.len()).unwrap();
    assert_eq!(afs_plan.entry_index, 0);
    assert_eq!(afs_plan.old_aligned_size % 2048, 0);
    assert_eq!(afs_plan.new_aligned_size % 2048, 0);

    let inventory = AfsInventory {
        file: Some("Z_DATA.BIN".to_string()),
        file_count: Some(2),
        entries: vec![
            AfsEntry {
                index: 0,
                offset: afs_plan.old_offset,
                size: original_pzz.len(),
                name: Some("pl00.pzz".to_string()),
            },
            AfsEntry {
                index: 1,
                offset: afs_plan.shifted_entries[0].new_offset,
                size: 4,
                name: Some("other.bin".to_string()),
            },
        ],
    };
    let save_plan = AfsSavePlanner::new(inventory, 0, pzz_plan.rebuilt_pzz.len())
        .plan()
        .unwrap();
    assert!(save_plan
        .validation_messages
        .iter()
        .any(|m| m.contains("2048-byte aligned")));
}

#[test]
fn pzz_save_planner_preserves_non_stream_chunks_and_rejects_stream_count_changes() {
    let original = make_pzz_with_non_stream_chunk();
    let plan = PzzSavePlanner::new(&original, vec![b"PMF2_changed".to_vec()])
        .plan_preserving_layout()
        .unwrap();

    let original_info = inspect_pzz(&original).unwrap();
    let rebuilt_info = inspect_pzz(&plan.rebuilt_pzz).unwrap();
    assert_eq!(original_info.chunk_count, rebuilt_info.chunk_count);
    assert_eq!(rebuilt_info.stream_count, 1);

    let error = PzzSavePlanner::new(
        &original,
        vec![b"PMF2_changed".to_vec(), b"MIG.00.1PSP".to_vec()],
    )
    .plan_preserving_layout()
    .unwrap_err();
    assert!(error.to_string().contains("requires 1 streams"));
}

#[test]
fn pmf2_preview_mesh_extracts_vertices_for_direct_viewport_rendering() {
    let meta = serde_json::json!({
        "model_name": "preview",
        "bbox": [1.0, 1.0, 1.0],
        "section_count": 0,
        "sections": [],
        "bone_meshes": [{
            "bone_index": 0,
            "bone_name": "body",
            "vertex_count": 3,
            "face_count": 1,
            "has_uv": true,
            "has_normals": false,
            "draw_call_vtypes": [],
            "local_vertices": [
                [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
                [1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
                [0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0]
            ],
            "faces": [[0, 1, 2]]
        }]
    });
    let meta = serde_json::from_value(meta).unwrap();
    let preview = Pmf2PreviewMesh::from_meta(&meta).unwrap();

    assert_eq!(preview.vertices.len(), 3);
    assert_eq!(preview.indices, vec![0, 1, 2]);
    assert_eq!(preview.bounds.min, [0.0, 0.0, 0.0]);
    assert_eq!(preview.bounds.max, [1.0, 1.0, 0.0]);
}

#[test]
fn pmf2_preview_from_extracted_mesh_uses_world_vertices() {
    let meshes = vec![BoneMeshData {
        bone_index: 0,
        bone_name: "body".to_string(),
        vertices: vec![
            ParsedVertex {
                x: 10.0,
                y: 0.0,
                z: 0.0,
                ..ParsedVertex::default()
            },
            ParsedVertex {
                x: 11.0,
                y: 0.0,
                z: 0.0,
                ..ParsedVertex::default()
            },
            ParsedVertex {
                x: 10.0,
                y: 1.0,
                z: 0.0,
                ..ParsedVertex::default()
            },
        ],
        faces: vec![(0, 1, 2)],
        local_vertices: vec![
            ParsedVertex::default(),
            ParsedVertex::default(),
            ParsedVertex::default(),
        ],
        has_uv: false,
        has_normals: false,
        vtypes: Vec::new(),
    }];
    let preview = Pmf2PreviewMesh::from_bone_meshes(&meshes).unwrap();

    assert_eq!(preview.vertices.len(), 3);
    assert_eq!(preview.indices, vec![0, 1, 2]);
    assert_eq!(preview.bounds.min, [10.0, 0.0, 0.0]);
}

#[test]
fn pmf2_preview_projects_visible_triangles_axes_and_bounds() {
    let meta = serde_json::json!({
        "model_name": "preview",
        "bbox": [2.0, 2.0, 2.0],
        "section_count": 0,
        "sections": [],
        "bone_meshes": [
            {
                "bone_index": 0,
                "bone_name": "body",
                "vertex_count": 3,
                "face_count": 1,
                "has_uv": true,
                "has_normals": false,
                "draw_call_vtypes": [],
                "local_vertices": [
                    [-1.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
                    [1.0, -1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
                    [0.0, 1.0, 0.0, 0.5, 1.0, 0.0, 0.0, 1.0]
                ],
                "faces": [[0, 1, 2]]
            },
            {
                "bone_index": 1,
                "bone_name": "hidden_weapon",
                "vertex_count": 3,
                "face_count": 1,
                "has_uv": false,
                "has_normals": false,
                "draw_call_vtypes": [],
                "local_vertices": [
                    [3.0, 3.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
                    [4.0, 3.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
                    [3.0, 4.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0]
                ],
                "faces": [[0, 1, 2]]
            }
        ]
    });
    let meta = serde_json::from_value(meta).unwrap();
    let preview = Pmf2PreviewMesh::from_meta(&meta).unwrap();
    let camera = PreviewCamera::frame_bounds(preview.bounds);
    let viewport = PreviewViewport {
        width: 800.0,
        height: 600.0,
    };
    let mut visibility = PreviewVisibility::default();
    visibility.set_bone_visible(1, false);

    let projected = preview.project(&camera, viewport, &visibility).unwrap();

    assert_eq!(projected.triangles.len(), 1);
    assert_eq!(projected.axes.len(), 3);
    assert_eq!(projected.bounds.len(), 12);
    assert!(projected.triangles[0]
        .points
        .iter()
        .all(|p| p[0] >= 0.0 && p[0] <= viewport.width && p[1] >= 0.0 && p[1] <= viewport.height));
}

#[test]
fn pmf2_preview_camera_orbit_changes_projection() {
    let meta = serde_json::json!({
        "model_name": "preview",
        "bbox": [1.0, 1.0, 1.0],
        "section_count": 0,
        "sections": [],
        "bone_meshes": [{
            "bone_index": 0,
            "bone_name": "body",
            "vertex_count": 3,
            "face_count": 1,
            "has_uv": false,
            "has_normals": false,
            "draw_call_vtypes": [],
            "local_vertices": [
                [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
                [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
                [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0]
            ],
            "faces": [[0, 1, 2]]
        }]
    });
    let meta = serde_json::from_value(meta).unwrap();
    let preview = Pmf2PreviewMesh::from_meta(&meta).unwrap();
    let viewport = PreviewViewport {
        width: 800.0,
        height: 600.0,
    };
    let camera = PreviewCamera::frame_bounds(preview.bounds);
    let mut orbit_camera = camera;
    orbit_camera.orbit(0.6, 0.0);

    let original = preview
        .project(&camera, viewport, &PreviewVisibility::default())
        .unwrap();
    let orbit = preview
        .project(&orbit_camera, viewport, &PreviewVisibility::default())
        .unwrap();

    assert_ne!(original.triangles[0].points, orbit.triangles[0].points);
}

#[test]
fn pmf2_preview_reports_unique_bones_for_visibility_controls() {
    let meta = serde_json::json!({
        "model_name": "preview",
        "bbox": [1.0, 1.0, 1.0],
        "section_count": 0,
        "sections": [],
        "bone_meshes": [
            {
                "bone_index": 2,
                "bone_name": "weapon",
                "vertex_count": 3,
                "face_count": 1,
                "has_uv": false,
                "has_normals": false,
                "draw_call_vtypes": [],
                "local_vertices": [
                    [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
                    [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
                    [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0]
                ],
                "faces": [[0, 1, 2]]
            },
            {
                "bone_index": 2,
                "bone_name": "weapon_dup",
                "vertex_count": 3,
                "face_count": 1,
                "has_uv": false,
                "has_normals": false,
                "draw_call_vtypes": [],
                "local_vertices": [
                    [0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
                    [1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
                    [0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]
                ],
                "faces": [[0, 1, 2]]
            },
            {
                "bone_index": 0,
                "bone_name": "body",
                "vertex_count": 3,
                "face_count": 1,
                "has_uv": false,
                "has_normals": false,
                "draw_call_vtypes": [],
                "local_vertices": [
                    [0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 1.0],
                    [1.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 1.0],
                    [0.0, 1.0, 2.0, 0.0, 0.0, 0.0, 0.0, 1.0]
                ],
                "faces": [[0, 1, 2]]
            }
        ]
    });
    let meta = serde_json::from_value(meta).unwrap();
    let preview = Pmf2PreviewMesh::from_meta(&meta).unwrap();

    assert_eq!(preview.bones(), vec![0, 2]);
}

#[test]
fn gim_metadata_and_rgba_preview_are_decoded_without_external_tools() {
    let gim = make_gim_rgba8888();
    let image = GimImage::decode(&gim).unwrap();

    assert_eq!(image.metadata.width, 1);
    assert_eq!(image.metadata.height, 1);
    assert_eq!(image.metadata.format, PixelFormat::Rgba8888);
    assert_eq!(image.rgba, vec![[10, 20, 30, 255]]);

    let replaced = image.replace_rgba(&[[1, 2, 3, 4]]).unwrap();
    let replaced_image = GimImage::decode(&replaced).unwrap();
    assert_eq!(replaced_image.rgba, vec![[1, 2, 3, 4]]);
}

#[test]
fn gim_decode_rejects_unsupported_pixel_order() {
    let mut gim = make_gim_rgba8888();
    let image_info = 0x20usize;
    gim[image_info + 0x06..image_info + 0x08].copy_from_slice(&2u16.to_le_bytes());

    let error = GimImage::decode(&gim).unwrap_err();

    assert!(error.to_string().contains("unsupported GIM pixel order"));
}

#[test]
fn gim_decode_rejects_malformed_recursive_blocks() {
    let mut gim = vec![0u8; 0x30];
    gim[0..11].copy_from_slice(b"MIG.00.1PSP");
    gim[0x10..0x12].copy_from_slice(&0x02u16.to_le_bytes());
    gim[0x14..0x18].copy_from_slice(&0x20u32.to_le_bytes());
    gim[0x1C..0x20].copy_from_slice(&0u32.to_le_bytes());

    let error = GimImage::decode(&gim).unwrap_err();

    assert!(error.to_string().contains("invalid GIM child block offset"));
}

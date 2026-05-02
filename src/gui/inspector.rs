use super::gim_preview_cache::{gim_data_identity, GimPreviewCache, GimPreviewCacheKey};
use crate::{
    pmf2, pzz,
    workspace::{AssetKind, EntryValidation, ModWorkspace},
};
use eframe::egui;

pub fn show_inspector(
    ui: &mut egui::Ui,
    workspace: &ModWorkspace,
    selected_afs_entry: Option<usize>,
    selected_stream: Option<usize>,
    gim_cache: &mut GimPreviewCache,
) {
    ui.heading("Inspector");
    ui.separator();

    if let Some(stream_index) = selected_stream {
        if let Some(pzz) = workspace.open_pzz() {
            if let Some(stream_node) = pzz.streams().get(stream_index) {
                show_stream_inspector(
                    ui,
                    stream_node,
                    pzz.stream_data().get(stream_index),
                    pzz.revision(),
                    gim_cache,
                );
                return;
            }
        }
    }

    if let Some(entry_index) = selected_afs_entry {
        if let Some(entry) = workspace
            .afs_entries()
            .iter()
            .find(|e| e.index == entry_index)
        {
            show_entry_inspector(ui, entry);
            return;
        }
    }

    ui.label("Select an AFS entry or PZZ stream.");
}

fn show_entry_inspector(ui: &mut egui::Ui, entry: &crate::workspace::AfsEntryNode) {
    ui.strong(&entry.name);
    ui.separator();
    labeled_row(ui, "Index", &format!("{}", entry.index));
    labeled_row(ui, "Offset", &format!("0x{:08X}", entry.offset));
    labeled_row(ui, "Size", &format_size(entry.size));
    labeled_row(ui, "Kind", &format!("{:?}", entry.kind));
    let validation_text = match entry.validation {
        EntryValidation::Ok => "OK",
        EntryValidation::Empty => "Empty (offset=0, size=0)",
        EntryValidation::ExceedsBounds => "ERROR: exceeds file bounds",
        EntryValidation::Overlapping => "WARNING: overlaps with another entry",
    };
    labeled_row(ui, "Validation", validation_text);
}

fn show_stream_inspector(
    ui: &mut egui::Ui,
    stream: &crate::workspace::StreamNode,
    data: Option<&Vec<u8>>,
    pzz_revision: u64,
    gim_cache: &mut GimPreviewCache,
) {
    ui.strong(&stream.name);
    ui.separator();
    labeled_row(ui, "Index", &format!("{}", stream.index));
    labeled_row(ui, "Size", &format_size(stream.size));
    labeled_row(ui, "Kind", &format!("{:?}", stream.kind));
    labeled_row(ui, "Dirty", if stream.dirty { "Yes" } else { "No" });

    let Some(data) = data else { return };

    ui.separator();

    match stream.kind {
        AssetKind::Pmf2 => show_pmf2_summary(ui, data),
        AssetKind::Gim => show_gim_summary(ui, stream.index, pzz_revision, data, gim_cache),
        _ => show_raw_summary(ui, data),
    }
}

fn show_pmf2_summary(ui: &mut egui::Ui, data: &[u8]) {
    ui.strong("PMF2 Summary");
    let (sections, bbox) = pmf2::parse_pmf2_sections(data);
    labeled_row(
        ui,
        "BBox Scale",
        &format!("{:.4}, {:.4}, {:.4}", bbox[0], bbox[1], bbox[2]),
    );
    labeled_row(ui, "Sections", &format!("{}", sections.len()));
    let mesh_count = sections.iter().filter(|s| s.has_mesh).count();
    labeled_row(ui, "Bones with mesh", &format!("{}", mesh_count));

    let drawable_count = sections
        .iter()
        .filter(|section| pmf2::section_render_policy(section.index).draws)
        .count();
    let safe_mesh_target_count = sections
        .iter()
        .filter(|section| pmf2::section_render_policy(section.index).safe_mesh_target)
        .count();
    let special_count = sections
        .iter()
        .filter(|section| !pmf2::section_render_policy(section.index).safe_mesh_target)
        .count();
    labeled_row(
        ui,
        "Runtime drawable sections",
        &format!("{}", drawable_count),
    );
    labeled_row(
        ui,
        "Safe mesh targets",
        &format!(
            "{} safe, {} special/unsafe",
            safe_mesh_target_count, special_count
        ),
    );

    let (meshes, _, _, _) = pmf2::extract_per_bone_meshes(data, false);
    let total_verts: usize = meshes.iter().map(|m| m.vertices.len()).sum();
    let total_faces: usize = meshes.iter().map(|m| m.faces.len()).sum();
    labeled_row(ui, "Total vertices", &format!("{}", total_verts));
    labeled_row(ui, "Total faces", &format!("{}", total_faces));

    ui.separator();
    ui.strong("Runtime Render Policy");
    ui.label("IDA-derived main render mask: draw bit means the game enqueues this section's display list. Preview can still show unsafe sections.");
    egui::ScrollArea::vertical()
        .max_height(360.0)
        .show(ui, |ui| {
            egui::Grid::new("pmf2_runtime_render_policy_grid")
                .striped(true)
                .num_columns(9)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    ui.strong("#");
                    ui.strong("Name");
                    ui.strong("Kind");
                    ui.strong("Parent");
                    ui.strong("Mesh");
                    ui.strong("Mask");
                    ui.strong("Draw");
                    ui.strong("Traverse");
                    ui.strong("Import target");
                    ui.end_row();

                    for section in &sections {
                        let policy = pmf2::section_render_policy(section.index);
                        ui.monospace(format!("{:02}", section.index));
                        ui.monospace(&section.name);
                        ui.monospace(if section.category.is_empty() {
                            "-"
                        } else {
                            &section.category
                        });
                        ui.monospace(format!("{}", section.parent));
                        ui.monospace(if section.has_mesh { "yes" } else { "no" });
                        ui.monospace(
                            policy
                                .flags
                                .map(|flags| format!("0x{flags:04X}"))
                                .unwrap_or_else(|| "unknown".to_string()),
                        );
                        ui.monospace(if policy.draws { "yes" } else { "no" });
                        ui.monospace(if policy.traverses { "yes" } else { "no" });
                        let target_text = if policy.safe_mesh_target {
                            "safe"
                        } else if policy.flags.is_none() {
                            "unknown"
                        } else {
                            "unsafe"
                        };
                        if policy.safe_mesh_target {
                            ui.monospace(target_text);
                        } else {
                            ui.colored_label(egui::Color32::YELLOW, target_text);
                        }
                        ui.end_row();
                    }
                });
        });
}

fn show_gim_summary(
    ui: &mut egui::Ui,
    stream_index: usize,
    pzz_revision: u64,
    data: &[u8],
    cache: &mut GimPreviewCache,
) {
    ui.strong("GIM Summary");
    let key = GimPreviewCacheKey {
        stream_index,
        pzz_revision,
        data_identity: gim_data_identity(data),
    };
    match cache.ensure_decoded(key, data) {
        Ok(()) => {
            let Some(image) = cache.image() else {
                ui.label("GIM decode error: preview cache is empty.");
                return;
            };
            labeled_row(
                ui,
                "Dimensions",
                &format!("{}x{}", image.metadata.width, image.metadata.height),
            );
            labeled_row(ui, "Format", &format!("{:?}", image.metadata.format));
            labeled_row(
                ui,
                "Swizzled",
                if image.metadata.swizzled { "Yes" } else { "No" },
            );
            let metadata = image.metadata.clone();
            ui.separator();
            if let Some(texture) =
                cache.texture_handle(ui.ctx(), format!("inspector_gim_thumb_{}", stream_index))
            {
                let max_side = 200.0;
                let scale = (max_side / metadata.width as f32)
                    .min(max_side / metadata.height as f32)
                    .min(1.0);
                let display_size = egui::vec2(
                    metadata.width as f32 * scale,
                    metadata.height as f32 * scale,
                );
                ui.image((texture.id(), display_size));
            } else {
                ui.label("GIM texture error: preview cache is empty.");
            }
        }
        Err(e) => {
            ui.label(format!("GIM decode error: {e}"));
        }
    }
}

fn show_raw_summary(ui: &mut egui::Ui, data: &[u8]) {
    if data.len() >= 4 {
        let magic = &data[..4];
        let magic_str = String::from_utf8_lossy(magic)
            .chars()
            .map(|c| {
                if c.is_ascii_graphic() || c == ' ' {
                    c
                } else {
                    '.'
                }
            })
            .collect::<String>();
        labeled_row(
            ui,
            "Magic",
            &format!(
                "0x{:02X}{:02X}{:02X}{:02X} (\"{}\")",
                magic[0], magic[1], magic[2], magic[3], magic_str
            ),
        );
    }
    let classify = pzz::classify_stream(data);
    if classify != "unknown" {
        labeled_row(ui, "Detected", classify);
    }
    ui.separator();
    ui.strong("Hex Preview");
    egui::ScrollArea::vertical()
        .max_height(300.0)
        .show(ui, |ui| {
            for (row, chunk) in data.chunks(16).take(16).enumerate() {
                ui.monospace(format!(
                    "{:08X}: {}",
                    row * 16,
                    chunk
                        .iter()
                        .map(|b| format!("{b:02X}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                ));
            }
        });
}

fn labeled_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(format!("{label}:"));
        ui.monospace(value);
    });
}

fn format_size(bytes: usize) -> String {
    if bytes >= 1024 * 1024 {
        format!("{} ({:.1} MB)", bytes, bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{} ({:.1} KB)", bytes, bytes as f64 / 1024.0)
    } else {
        format!("{} bytes", bytes)
    }
}

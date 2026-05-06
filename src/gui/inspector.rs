use super::gim_preview_cache::{GimPreviewCache, GimPreviewCacheKey, gim_data_identity};
use crate::{
    pmf2, pzz,
    render::PreviewVisibility,
    workspace::{AssetKind, EntryValidation, ModWorkspace},
};
use eframe::egui;
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Pmf2SummaryCacheKey {
    pub stream_index: usize,
    pub pzz_revision: u64,
    pub data_identity: u64,
}

#[derive(Default)]
pub(super) struct Pmf2SummaryCache {
    key: Option<Pmf2SummaryCacheKey>,
    summary: Option<Pmf2Summary>,
}

struct Pmf2Summary {
    sections: Vec<pmf2::BoneSection>,
    bbox: [f32; 3],
    mesh_count: usize,
    drawable_count: usize,
    safe_mesh_target_count: usize,
    special_count: usize,
    total_verts: usize,
    total_faces: usize,
}

impl Pmf2SummaryCache {
    pub(super) fn is_valid_for(&self, key: Pmf2SummaryCacheKey) -> bool {
        self.key == Some(key)
    }

    fn ensure_summary(&mut self, key: Pmf2SummaryCacheKey, data: &[u8]) -> &Pmf2Summary {
        if !self.is_valid_for(key) {
            let started = Instant::now();
            let (sections, bbox) = pmf2::parse_pmf2_sections(data);
            let mesh_count = sections.iter().filter(|s| s.has_mesh).count();
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
            let (meshes, _, _, _) = pmf2::extract_per_bone_meshes(data, false);
            let total_verts = meshes.iter().map(|m| m.vertices.len()).sum();
            let total_faces = meshes.iter().map(|m| m.faces.len()).sum();

            self.key = Some(key);
            self.summary = Some(Pmf2Summary {
                sections,
                bbox,
                mesh_count,
                drawable_count,
                safe_mesh_target_count,
                special_count,
                total_verts,
                total_faces,
            });
            eprintln!(
                "[gui] cached PMF2 summary stream={} revision={} data=0x{:016X} verts={} faces={} in {:?}",
                key.stream_index,
                key.pzz_revision,
                key.data_identity,
                total_verts,
                total_faces,
                started.elapsed()
            );
        }

        self.summary
            .as_ref()
            .expect("PMF2 summary cache populated after ensure")
    }

    #[cfg(test)]
    fn store_test_key(&mut self, key: Pmf2SummaryCacheKey) {
        self.key = Some(key);
    }
}

pub struct InspectorAction {
    pub rename_entry: Option<(usize, String)>,
}

impl InspectorAction {
    fn none() -> Self {
        Self { rename_entry: None }
    }
}

pub fn show_inspector(
    ui: &mut egui::Ui,
    workspace: &ModWorkspace,
    selected_afs_entry: Option<usize>,
    selected_stream: Option<usize>,
    gim_cache: &mut GimPreviewCache,
    pmf2_cache: &mut Pmf2SummaryCache,
    preview_visibility: &mut PreviewVisibility,
    entry_name_edit_buf: &mut String,
) -> InspectorAction {
    let mut action = InspectorAction::none();
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
                    pmf2_cache,
                    preview_visibility,
                );
                return action;
            }
        }
    }

    if let Some(entry_index) = selected_afs_entry {
        if let Some(entry) = workspace
            .afs_entries()
            .iter()
            .find(|e| e.index == entry_index)
        {
            show_entry_inspector(ui, entry, entry_name_edit_buf, &mut action);
            return action;
        }
    }

    ui.label("Select an AFS entry or PZZ stream.");
    action
}

fn show_entry_inspector(
    ui: &mut egui::Ui,
    entry: &crate::workspace::AfsEntryNode,
    name_edit_buf: &mut String,
    action: &mut InspectorAction,
) {
    ui.strong("Entry Info");
    ui.separator();

    ui.horizontal(|ui| {
        ui.label("Name:");
        if *name_edit_buf != entry.name && !ui.memory(|m| m.has_focus(egui::Id::new("entry_name_edit"))) {
            *name_edit_buf = entry.name.clone();
        }
        let response = ui.add(
            egui::TextEdit::singleline(name_edit_buf)
                .id(egui::Id::new("entry_name_edit"))
                .desired_width(180.0),
        );
        if response.lost_focus() && *name_edit_buf != entry.name {
            let mut new_name = name_edit_buf.clone();
            if new_name.len() > 0x20 {
                let truncate_at = new_name
                    .char_indices()
                    .take_while(|(i, _)| *i < 0x20)
                    .last()
                    .map(|(i, c)| i + c.len_utf8())
                    .unwrap_or(0);
                new_name.truncate(truncate_at);
            }
            action.rename_entry = Some((entry.index, new_name));
        }
    });

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
    pmf2_cache: &mut Pmf2SummaryCache,
    preview_visibility: &mut PreviewVisibility,
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
        AssetKind::Pmf2 => show_pmf2_summary(
            ui,
            stream.index,
            pzz_revision,
            data,
            pmf2_cache,
            preview_visibility,
        ),
        AssetKind::Gim => show_gim_summary(ui, stream.index, pzz_revision, data, gim_cache),
        _ => show_raw_summary(ui, data),
    }
}

fn show_pmf2_summary(
    ui: &mut egui::Ui,
    stream_index: usize,
    pzz_revision: u64,
    data: &[u8],
    cache: &mut Pmf2SummaryCache,
    preview_visibility: &mut PreviewVisibility,
) {
    ui.strong("PMF2 Summary");
    let key = Pmf2SummaryCacheKey {
        stream_index,
        pzz_revision,
        data_identity: gim_data_identity(data),
    };
    let summary = cache.ensure_summary(key, data);
    labeled_row(
        ui,
        "BBox Scale",
        &format!(
            "{:.4}, {:.4}, {:.4}",
            summary.bbox[0], summary.bbox[1], summary.bbox[2]
        ),
    );
    labeled_row(ui, "Sections", &format!("{}", summary.sections.len()));
    labeled_row(ui, "Bones with mesh", &format!("{}", summary.mesh_count));
    labeled_row(
        ui,
        "Runtime drawable sections",
        &format!("{}", summary.drawable_count),
    );
    labeled_row(
        ui,
        "Safe mesh targets",
        &format!(
            "{} safe, {} special/unsafe",
            summary.safe_mesh_target_count, summary.special_count
        ),
    );

    labeled_row(ui, "Total vertices", &format!("{}", summary.total_verts));
    labeled_row(ui, "Total faces", &format!("{}", summary.total_faces));

    ui.separator();
    egui::CollapsingHeader::new("Mesh Visibility")
        .default_open(true)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button("All").clicked() {
                    for section in summary.sections.iter().filter(|s| s.has_mesh) {
                        preview_visibility.set_bone_visible(section.index, true);
                    }
                }
                if ui.button("None").clicked() {
                    for section in summary.sections.iter().filter(|s| s.has_mesh) {
                        preview_visibility.set_bone_visible(section.index, false);
                    }
                }
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .max_height(220.0)
                .show(ui, |ui| {
                    for section in summary.sections.iter().filter(|s| s.has_mesh) {
                        let mut visible = preview_visibility.is_bone_visible(section.index);
                        let label = format!("#{:02} {}", section.index, section.name);
                        if ui.checkbox(&mut visible, label).changed() {
                            preview_visibility.set_bone_visible(section.index, visible);
                        }
                    }
                });
        });

    egui::CollapsingHeader::new("Runtime Render Policy")
        .default_open(false)
        .show(ui, |ui| {
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

                            for section in &summary.sections {
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
            let dims = format!("{}x{}", image.metadata.width, image.metadata.height);
            let format_text = format!("{:?}", image.metadata.format);
            let swizzled = if image.metadata.swizzled { "swizzled" } else { "linear" };
            ui.separator();

            ui.horizontal_wrapped(|ui| {
                ui.monospace(format!("{dims} | {format_text} | {swizzled}"));
            });
            ui.separator();

            let available = ui.available_size();
            if available.x <= 1.0 || available.y <= 1.0 {
                ui.label("Inspector area too small for GIM preview.");
                return;
            }

            if let Some(texture) =
                cache.texture_handle(ui.ctx(), format!("inspector_gim_full_{}", stream_index))
            {
                ui.add_sized(
                    available,
                    egui::Image::new((texture.id(), texture.size_vec2())).fit_to_exact_size(available),
                );
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

#[cfg(test)]
mod tests {
    use super::{Pmf2SummaryCache, Pmf2SummaryCacheKey};

    #[test]
    fn pmf2_summary_cache_key_hits_only_for_same_stream_revision_and_data() {
        let mut cache = Pmf2SummaryCache::default();
        let key = Pmf2SummaryCacheKey {
            stream_index: 0,
            pzz_revision: 7,
            data_identity: 42,
        };

        assert!(!cache.is_valid_for(key));
        cache.store_test_key(key);

        assert!(cache.is_valid_for(key));
        assert!(!cache.is_valid_for(Pmf2SummaryCacheKey {
            stream_index: 1,
            ..key
        }));
        assert!(!cache.is_valid_for(Pmf2SummaryCacheKey {
            pzz_revision: 8,
            ..key
        }));
        assert!(!cache.is_valid_for(Pmf2SummaryCacheKey {
            data_identity: 43,
            ..key
        }));
    }
}

use crate::{pmf2, pzz, save::{PzzSavePlan, PzzSavePlanner}, texture::GimImage, workspace::ModWorkspace};
use anyhow::Result;
use eframe::egui;
use std::path::PathBuf;

#[derive(Default)]
pub struct EditorWindows {
    pub pmf2_metadata: Option<usize>,
    pub pmf2_data: Option<usize>,
    pub gim_preview: Option<usize>,
    pub hex_view: Option<usize>,
    pub save_planner: bool,
    cached_save_plan: Option<Result<PzzSavePlan, String>>,
}

pub struct EditorAction {
    pub status: Option<String>,
    /// When true, show an egui modal with the status text plus bottom status bar copy.
    pub error_modal: bool,
    pub dirs_changed: bool,
}

impl EditorAction {
    fn none() -> Self {
        Self {
            status: None,
            error_modal: false,
            dirs_changed: false,
        }
    }
    fn status(msg: String) -> Self {
        Self {
            status: Some(msg),
            error_modal: false,
            dirs_changed: false,
        }
    }
    fn error(msg: String) -> Self {
        Self {
            status: Some(msg.clone()),
            error_modal: true,
            dirs_changed: false,
        }
    }

    fn status_touch_dirs(msg: String) -> Self {
        Self {
            status: Some(msg),
            error_modal: false,
            dirs_changed: true,
        }
    }

    pub fn accumulate_from(&mut self, other: EditorAction) {
        self.dirs_changed |= other.dirs_changed;
        if other.status.is_some() {
            self.status = other.status;
            self.error_modal = other.error_modal;
        }
    }
}

pub fn show_editor_windows(
    ctx: &egui::Context,
    workspace: &mut ModWorkspace,
    editors: &mut EditorWindows,
    last_dir_save_pzz_as: &mut Option<PathBuf>,
    last_dir_patch_afs_entry: &mut Option<PathBuf>,
) -> EditorAction {
    let mut action = EditorAction::none();

    if let Some(stream_index) = editors.pmf2_metadata {
        let mut open = true;
        egui::Window::new(format!("PMF2 Metadata - stream{:03}", stream_index))
            .open(&mut open)
            .default_size([560.0, 400.0])
            .resizable(true)
            .show(ctx, |ui| {
                show_pmf2_metadata_editor(ui, workspace, stream_index);
            });
        if !open {
            editors.pmf2_metadata = None;
        }
    }

    if let Some(stream_index) = editors.pmf2_data {
        let mut open = true;
        egui::Window::new(format!("PMF2 Data - stream{:03}", stream_index))
            .open(&mut open)
            .default_size([560.0, 400.0])
            .resizable(true)
            .show(ctx, |ui| {
                show_pmf2_data_viewer(ui, workspace, stream_index);
            });
        if !open {
            editors.pmf2_data = None;
        }
    }

    if let Some(stream_index) = editors.gim_preview {
        let mut open = true;
        egui::Window::new(format!("GIM Preview - stream{:03}", stream_index))
            .open(&mut open)
            .default_size([420.0, 400.0])
            .resizable(true)
            .show(ctx, |ui| {
                if let Some(result) = show_gim_preview_editor(ui, workspace, stream_index) {
                    action.accumulate_from(result);
                }
            });
        if !open {
            editors.gim_preview = None;
        }
    }

    if let Some(stream_index) = editors.hex_view {
        let mut open = true;
        egui::Window::new(format!("Hex View - stream{:03}", stream_index))
            .open(&mut open)
            .default_size([640.0, 400.0])
            .resizable(true)
            .show(ctx, |ui| {
                show_hex_viewer(ui, workspace, stream_index);
            });
        if !open {
            editors.hex_view = None;
        }
    }

    if editors.save_planner {
        let mut open = true;
        egui::Window::new("Save Planner")
            .open(&mut open)
            .default_size([560.0, 350.0])
            .resizable(true)
            .show(ctx, |ui| {
                if let Some(result) = show_save_planner(
                    ui,
                    workspace,
                    &mut editors.cached_save_plan,
                    last_dir_save_pzz_as,
                    last_dir_patch_afs_entry,
                ) {
                    action.accumulate_from(result);
                }
            });
        if !open {
            editors.save_planner = false;
            editors.cached_save_plan = None;
        }
    }

    action
}

fn get_stream_data<'a>(workspace: &'a ModWorkspace, index: usize) -> Option<&'a [u8]> {
    workspace
        .open_pzz()
        .and_then(|pzz| pzz.stream_data().get(index))
        .map(Vec::as_slice)
}

fn show_pmf2_metadata_editor(ui: &mut egui::Ui, workspace: &ModWorkspace, stream_index: usize) {
    let Some(data) = get_stream_data(workspace, stream_index) else {
        ui.label("Stream not available.");
        return;
    };
    if pzz::classify_stream(data) != "pmf2" {
        ui.label("Not a PMF2 stream.");
        return;
    }
    let (sections, bbox) = pmf2::parse_pmf2_sections(data);
    ui.label(format!(
        "BBox scale: {:.6}, {:.6}, {:.6}",
        bbox[0], bbox[1], bbox[2]
    ));
    ui.label(format!("Sections: {}", sections.len()));
    ui.separator();
    egui::ScrollArea::both().show(ui, |ui| {
        ui.set_min_width(500.0);
        for section in sections {
            ui.collapsing(&section.name, |ui| {
                ui.monospace(format!("Index: {}", section.index));
                ui.monospace(format!("Parent: {}", section.parent));
                ui.monospace(format!("Has mesh: {}", section.has_mesh));
                ui.monospace(format!("Offset: 0x{:X}", section.offset));
                ui.monospace(format!("Size: {}", section.size));
                ui.monospace(format!("Category: {}", section.category));
            });
        }
    });
}

fn show_pmf2_data_viewer(ui: &mut egui::Ui, workspace: &ModWorkspace, stream_index: usize) {
    let Some(data) = get_stream_data(workspace, stream_index) else {
        ui.label("Stream not available.");
        return;
    };
    if pzz::classify_stream(data) != "pmf2" {
        ui.label("Not a PMF2 stream.");
        return;
    }
    let (meshes, _, _, _) = pmf2::extract_per_bone_meshes(data, false);
    egui::ScrollArea::both().show(ui, |ui| {
        ui.set_min_width(500.0);
        for mesh in meshes {
            ui.collapsing(&mesh.bone_name, |ui| {
                ui.monospace(format!("Vertices: {}", mesh.vertices.len()));
                ui.monospace(format!("Faces: {}", mesh.faces.len()));
                ui.monospace(format!("UV: {}", mesh.has_uv));
                ui.monospace(format!("Normals: {}", mesh.has_normals));
                ui.monospace(format!("VTypes: {:?}", mesh.vtypes));
            });
        }
    });
}

fn show_gim_preview_editor(
    ui: &mut egui::Ui,
    workspace: &mut ModWorkspace,
    stream_index: usize,
) -> Option<EditorAction> {
    let data = workspace
        .open_pzz()
        .and_then(|pzz| pzz.stream_data().get(stream_index))
        .map(Vec::as_slice);
    let Some(data) = data else {
        ui.label("Stream not available.");
        return None;
    };
    if pzz::classify_stream(data) != "gim" {
        ui.label("Not a GIM stream.");
        return None;
    }
    let image = match GimImage::decode(data) {
        Ok(img) => img,
        Err(e) => {
            ui.label(format!("GIM decode failed: {e}"));
            return None;
        }
    };

    ui.label(format!(
        "{}x{} {:?}{}",
        image.metadata.width,
        image.metadata.height,
        image.metadata.format,
        if image.metadata.swizzled {
            " (swizzled)"
        } else {
            ""
        }
    ));

    let mut result = None;
    ui.horizontal(|ui| {
        if ui.button("Export PNG").clicked() {
            result = Some(export_gim_png(&image));
        }
        if ui.button("Replace from PNG").clicked() {
            result = Some(replace_gim_png(&image, workspace, stream_index));
        }
    });
    ui.separator();

    let flat: Vec<u8> = image.rgba.iter().flat_map(|p| *p).collect();
    let color_image = egui::ColorImage::from_rgba_unmultiplied(
        [image.metadata.width, image.metadata.height],
        &flat,
    );
    let texture = ui.ctx().load_texture(
        format!("gim_editor_{}", stream_index),
        color_image,
        egui::TextureOptions::NEAREST,
    );
    ui.image((texture.id(), texture.size_vec2()));

    result
}

fn export_gim_png(image: &GimImage) -> EditorAction {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("PNG", &["png"])
        .set_file_name("texture.png")
        .save_file()
    else {
        return EditorAction::none();
    };
    let mut output =
        image::RgbaImage::new(image.metadata.width as u32, image.metadata.height as u32);
    for (i, pixel) in image.rgba.iter().enumerate() {
        let x = (i % image.metadata.width) as u32;
        let y = (i / image.metadata.width) as u32;
        output.put_pixel(x, y, image::Rgba(*pixel));
    }
    match output.save(&path) {
        Ok(()) => EditorAction::status(format!("Exported PNG: {}", path.display())),
        Err(e) => EditorAction::error(format!("PNG export failed: {e}")),
    }
}

fn replace_gim_png(
    image: &GimImage,
    workspace: &mut ModWorkspace,
    stream_index: usize,
) -> EditorAction {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("PNG", &["png"])
        .pick_file()
    else {
        return EditorAction::none();
    };
    let png_data = match std::fs::read(&path) {
        Ok(d) => d,
        Err(e) => return EditorAction::error(format!("Failed to read PNG: {e}")),
    };
    let replaced = match image.replace_png_bytes(&png_data) {
        Ok(d) => d,
        Err(e) => return EditorAction::error(format!("GIM replace failed: {e}")),
    };
    match workspace.replace_stream(stream_index, replaced) {
        Ok(()) => EditorAction::status("Replaced GIM stream from PNG".to_string()),
        Err(e) => EditorAction::error(format!("Stream replace failed: {e}")),
    }
}

fn show_hex_viewer(ui: &mut egui::Ui, workspace: &ModWorkspace, stream_index: usize) {
    let Some(data) = get_stream_data(workspace, stream_index) else {
        ui.label("Stream not available.");
        return;
    };
    ui.label(format!("{} bytes", data.len()));
    ui.separator();
    let row_count = (data.len() + 15) / 16;
    let row_height = 16.0;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, row_height, row_count, |ui, range| {
            for row in range {
                let offset = row * 16;
                let end = (offset + 16).min(data.len());
                let chunk = &data[offset..end];
                let hex: String = chunk
                    .iter()
                    .map(|b| format!("{b:02X}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                let ascii: String = chunk
                    .iter()
                    .map(|&b| {
                        if b.is_ascii_graphic() || b == b' ' {
                            b as char
                        } else {
                            '.'
                        }
                    })
                    .collect();
                ui.monospace(format!("{offset:08X}: {hex:<48} {ascii}"));
            }
        });
}

fn show_save_planner(
    ui: &mut egui::Ui,
    workspace: &mut ModWorkspace,
    cached_plan: &mut Option<Result<PzzSavePlan, String>>,
    last_dir_save_pzz_as: &mut Option<PathBuf>,
    last_dir_patch_afs_entry: &mut Option<PathBuf>,
) -> Option<EditorAction> {
    let Some(pzz) = workspace.open_pzz() else {
        ui.label("Open a PZZ to inspect save impact.");
        return None;
    };

    if cached_plan.is_none() {
        let planner = PzzSavePlanner::new(pzz.original(), pzz.stream_data().to_vec());
        let original_stream_count = pzz::inspect_pzz(pzz.original())
            .map(|info| info.stream_count)
            .unwrap_or(0);
        let plan = if original_stream_count == pzz.stream_data().len() {
            planner.plan_preserving_layout()
        } else {
            planner.plan_stream_archive_rebuild()
        };
        *cached_plan = Some(plan.map_err(|e| e.to_string()));
    }

    let plan_result = cached_plan.as_ref().unwrap();
    match plan_result {
        Ok(plan) => {
            ui.strong("PZZ Rebuild Summary");
            ui.separator();
            ui.monospace(format!("Original size: {} bytes", plan.original_size));
            ui.monospace(format!("Rebuilt size:  {} bytes", plan.rebuilt_size));
            ui.monospace(format!("Streams: {}", plan.stream_count));
            ui.monospace(format!("Changed: {}", plan.changed_stream_count));
            ui.monospace(format!("Tail recomputed: {}", plan.tail_recomputed));
            ui.separator();
            for msg in &plan.validation_messages {
                ui.label(msg);
            }
            ui.separator();
            let mut result = None;
            ui.horizontal(|ui| {
                if ui.button("Save PZZ As...").clicked() {
                    result = Some(save_pzz_dialog(workspace, last_dir_save_pzz_as));
                }
                if ui.button("Patch AFS Entry...").clicked() {
                    result = Some(patch_afs_dialog(workspace, last_dir_patch_afs_entry));
                }
                if ui.button("Recalculate").clicked() {
                    *cached_plan = None;
                }
            });
            result
        }
        Err(e) => {
            ui.label(format!("Save plan failed: {e}"));
            if ui.button("Retry").clicked() {
                *cached_plan = None;
            }
            None
        }
    }
}

fn plan_pzz_save(workspace: &ModWorkspace) -> Result<Vec<u8>> {
    let pzz = workspace
        .open_pzz()
        .ok_or_else(|| anyhow::anyhow!("no PZZ is open"))?;
    let planner = PzzSavePlanner::new(pzz.original(), pzz.stream_data().to_vec());
    let original_stream_count = pzz::inspect_pzz(pzz.original())?.stream_count;
    if original_stream_count == pzz.stream_data().len() {
        Ok(planner.plan_preserving_layout()?.rebuilt_pzz)
    } else {
        Ok(planner.plan_stream_archive_rebuild()?.rebuilt_pzz)
    }
}

fn save_pzz_dialog(
    workspace: &ModWorkspace,
    last_dir: &mut Option<PathBuf>,
) -> EditorAction {
    let Some(pzz) = workspace.open_pzz() else {
        return EditorAction::error("No PZZ is open.".to_string());
    };
    let mut dialog = rfd::FileDialog::new().set_file_name(pzz.name());
    if let Some(dir) = last_dir.clone() {
        dialog = dialog.set_directory(dir);
    }
    let Some(path) = dialog.save_file() else {
        return EditorAction::none();
    };
    match plan_pzz_save(workspace)
        .and_then(|rebuilt| std::fs::write(&path, rebuilt).map_err(anyhow::Error::from))
    {
        Ok(()) => {
            super::remember_parent_dir(last_dir, &path);
            EditorAction::status_touch_dirs(format!("Saved PZZ: {}", path.display()))
        }
        Err(e) => EditorAction::error(format!("Failed to save PZZ: {e}")),
    }
}

fn patch_afs_dialog(
    workspace: &ModWorkspace,
    last_dir: &mut Option<PathBuf>,
) -> EditorAction {
    let Some(afs_path) = workspace.afs_path() else {
        return EditorAction::error("No AFS file is open.".to_string());
    };
    let Some(pzz) = workspace.open_pzz() else {
        return EditorAction::error("No PZZ is open.".to_string());
    };
    let Some(entry_index) = pzz.afs_entry_index() else {
        return EditorAction::error("PZZ was not opened from AFS.".to_string());
    };
    let mut dialog = rfd::FileDialog::new().set_file_name("Z_DATA_patched.BIN");
    if let Some(dir) = last_dir.clone() {
        dialog = dialog.set_directory(dir);
    }
    let Some(output_path) = dialog.save_file() else {
        return EditorAction::none();
    };
    let rebuilt = match plan_pzz_save(workspace) {
        Ok(r) => r,
        Err(e) => return EditorAction::error(format!("PZZ rebuild failed: {e}")),
    };
    let afs_data = match std::fs::read(afs_path) {
        Ok(d) => d,
        Err(e) => return EditorAction::error(format!("Failed to read AFS: {e}")),
    };
    match crate::afs::patch_entry_bytes(&afs_data, entry_index, &rebuilt) {
        Ok(patched) => match std::fs::write(&output_path, patched) {
            Ok(()) => {
                super::remember_parent_dir(last_dir, &output_path);
                EditorAction::status_touch_dirs(format!(
                    "Patched AFS entry {} -> {}",
                    entry_index,
                    output_path.display()
                ))
            }
            Err(e) => EditorAction::error(format!("Failed to write patched AFS: {e}")),
        },
        Err(e) => EditorAction::error(format!("AFS patch failed: {e}")),
    }
}

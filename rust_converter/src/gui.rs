mod asset_tree;
mod editors;
mod inspector;
mod preview;
mod status;

use crate::{
    afs, dae, pmf2, pzz,
    render::PreviewState,
    workspace::ModWorkspace,
};
use asset_tree::{AssetTreeState, TreeAction};
use editors::EditorWindows;
use eframe::egui;

pub struct GvgModdingApp {
    workspace: ModWorkspace,
    tree_state: AssetTreeState,
    preview_state: PreviewState,
    editors: EditorWindows,
    status: String,
    show_left_panel: bool,
    show_right_panel: bool,
}

pub fn run_native() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([960.0, 640.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "GVG Modding Tool",
        options,
        Box::new(|_cc| Ok(Box::new(GvgModdingApp::default()))),
    )
}

impl Default for GvgModdingApp {
    fn default() -> Self {
        Self {
            workspace: ModWorkspace::default(),
            tree_state: AssetTreeState::default(),
            preview_state: PreviewState::default(),
            editors: EditorWindows::default(),
            status: "Ready".to_string(),
            show_left_panel: true,
            show_right_panel: true,
        }
    }
}

impl eframe::App for GvgModdingApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            self.menu_bar(ui);
        });

        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            status::show_status_bar(ui, &self.status);
        });

        if self.show_left_panel {
            egui::SidePanel::left("asset_tree")
                .resizable(true)
                .default_width(320.0)
                .show(ctx, |ui| {
                    let actions =
                        asset_tree::show_asset_tree(ui, &self.workspace, &mut self.tree_state);
                    for action in actions {
                        self.handle_tree_action(action);
                    }
                });
        }

        if self.show_right_panel {
            egui::SidePanel::right("inspector")
                .resizable(true)
                .default_width(320.0)
                .show(ctx, |ui| {
                    inspector::show_inspector(
                        ui,
                        &self.workspace,
                        self.tree_state.selected_afs_entry,
                        self.tree_state.selected_stream,
                    );
                });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            preview::show_preview(
                ui,
                &self.workspace,
                self.tree_state.selected_stream,
                &mut self.preview_state,
            );
        });

        let editor_result =
            editors::show_editor_windows(ctx, &mut self.workspace, &mut self.editors);
        if let Some(msg) = editor_result.status {
            self.status = msg;
        }
    }
}

impl GvgModdingApp {
    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Open AFS/BIN...").clicked() {
                    self.open_afs_dialog();
                    ui.close();
                }
                if ui.button("Open PZZ...").clicked() {
                    self.open_pzz_dialog();
                    ui.close();
                }
                ui.separator();
                if ui.button("Save PZZ As...").clicked() {
                    self.editors.save_planner = true;
                    ui.close();
                }
                if ui.button("Patch AFS Entry...").clicked() {
                    self.editors.save_planner = true;
                    ui.close();
                }
                ui.separator();
                if ui.button("Exit").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("View", |ui| {
                ui.checkbox(&mut self.show_left_panel, "Show Left Panel");
                ui.checkbox(&mut self.show_right_panel, "Show Right Panel");
            });
        });
    }

    fn open_afs_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("AFS/BIN", &["bin", "afs"])
            .pick_file()
        else {
            return;
        };
        match ModWorkspace::open_afs_file(path) {
            Ok(ws) => {
                let count = ws.afs_entries().len();
                self.workspace = ws;
                self.tree_state = AssetTreeState::default();
                self.preview_state = PreviewState::default();
                self.editors = EditorWindows::default();
                self.status = format!("Loaded AFS ({} entries)", count);
            }
            Err(e) => {
                self.status = format!("Failed to open AFS: {e}");
            }
        }
    }

    fn open_pzz_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("PZZ", &["pzz"])
            .pick_file()
        else {
            return;
        };
        match ModWorkspace::open_pzz_file(path) {
            Ok(ws) => {
                self.workspace = ws;
                self.tree_state = AssetTreeState::default();
                self.preview_state = PreviewState::default();
                self.editors = EditorWindows::default();
                self.status = "Loaded PZZ".to_string();
            }
            Err(e) => {
                self.status = format!("Failed to open PZZ: {e}");
            }
        }
    }

    fn handle_tree_action(&mut self, action: TreeAction) {
        match action {
            TreeAction::SelectAfsEntry(index) => {
                self.tree_state.selected_afs_entry = Some(index);
                self.tree_state.selected_stream = None;
            }
            TreeAction::OpenPzz(index) => {
                if self.workspace.expanded_pzz_entry() == Some(index) {
                    self.workspace.close_open_pzz();
                    self.status = "Closed PZZ".to_string();
                    return;
                }
                match self.workspace.open_pzz_entry(index) {
                    Ok(()) => {
                        let stream_count = self
                            .workspace
                            .open_pzz()
                            .map(|p| p.streams().len())
                            .unwrap_or(0);
                        self.status = format!(
                            "Opened PZZ entry {} ({} streams)",
                            index, stream_count
                        );
                    }
                    Err(e) => {
                        self.status = format!("Failed to open PZZ entry {}: {e}", index);
                    }
                }
            }
            TreeAction::SelectStream(index) => {
                self.tree_state.selected_stream = Some(index);
                self.preview_state.camera = None;
            }
            TreeAction::ExportEntryRaw(index) => {
                self.export_entry_raw(index);
            }
            TreeAction::ExportStreamDae(index) => {
                self.export_stream_dae(index);
            }
            TreeAction::ReplaceStreamDae(index) => {
                self.replace_stream_dae(index);
            }
            TreeAction::ExportStreamPng(index) => {
                self.export_stream_png(index);
            }
            TreeAction::ReplaceStreamPng(index) => {
                self.replace_stream_png(index);
            }
            TreeAction::ExportStreamRaw(index) => {
                self.export_stream_raw(index);
            }
            TreeAction::OpenPmf2Metadata(index) => {
                self.editors.pmf2_metadata = Some(index);
            }
            TreeAction::OpenPmf2Data(index) => {
                self.editors.pmf2_data = Some(index);
            }
            TreeAction::OpenGimPreview(index) => {
                self.editors.gim_preview = Some(index);
            }
            TreeAction::OpenHexView(index) => {
                self.editors.hex_view = Some(index);
            }
            TreeAction::OpenSavePlanner => {
                self.editors.save_planner = true;
            }
        }
    }

    fn export_entry_raw(&mut self, entry_index: usize) {
        let Some(afs_path) = self.workspace.afs_path().cloned() else {
            self.status = "No AFS file is open".to_string();
            return;
        };
        let Some(entry) = self
            .workspace
            .afs_entries()
            .iter()
            .find(|e| e.index == entry_index)
        else {
            self.status = format!("Entry {} not found", entry_index);
            return;
        };
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(&entry.name)
            .save_file()
        else {
            return;
        };
        match afs::read_entry_from_file(&afs_path, entry.offset, entry.size) {
            Ok(data) => match std::fs::write(&path, &data) {
                Ok(()) => {
                    self.status = format!("Exported: {}", path.display());
                }
                Err(e) => {
                    self.status = format!("Write failed: {e}");
                }
            },
            Err(e) => {
                self.status = format!("Read failed: {e}");
            }
        }
    }

    fn export_stream_raw(&mut self, stream_index: usize) {
        let Some(data) = self.get_stream_data(stream_index) else {
            self.status = "Stream not available".to_string();
            return;
        };
        let name = self
            .workspace
            .open_pzz()
            .and_then(|p| p.streams().get(stream_index))
            .map(|s| s.name.clone())
            .unwrap_or_else(|| format!("stream{:03}.bin", stream_index));
        let Some(path) = rfd::FileDialog::new().set_file_name(&name).save_file() else {
            return;
        };
        match std::fs::write(&path, data) {
            Ok(()) => self.status = format!("Exported: {}", path.display()),
            Err(e) => self.status = format!("Write failed: {e}"),
        }
    }

    fn export_stream_dae(&mut self, stream_index: usize) {
        let Some(data) = self.get_stream_data(stream_index) else {
            self.status = "Stream not available".to_string();
            return;
        };
        if pzz::classify_stream(data) != "pmf2" {
            self.status = "Not a PMF2 stream".to_string();
            return;
        }
        let model_name = format!("stream{:03}", stream_index);
        let Some(path) = rfd::FileDialog::new()
            .add_filter("DAE", &["dae"])
            .set_file_name(&format!("{}.dae", model_name))
            .save_file()
        else {
            return;
        };
        let (bone_meshes, sections, bbox, _) = pmf2::extract_per_bone_meshes(data, true);
        if bone_meshes.is_empty() {
            self.status = "No mesh data in PMF2".to_string();
            return;
        }
        match dae::write_dae(&path, &bone_meshes, &sections, &model_name) {
            Ok(()) => {
                let meta = pmf2::build_meta(&model_name, &sections, bbox, &bone_meshes);
                let meta_path = path.with_extension("pmf2meta.json");
                let _ = std::fs::write(&meta_path, serde_json::to_string_pretty(&meta).unwrap());
                self.status = format!("Exported DAE: {}", path.display());
            }
            Err(e) => self.status = format!("DAE export failed: {e}"),
        }
    }

    fn replace_stream_dae(&mut self, stream_index: usize) {
        let Some(template_data) = self.get_stream_data(stream_index).map(|d| d.to_vec()) else {
            self.status = "Stream not available".to_string();
            return;
        };
        let Some(path) = rfd::FileDialog::new()
            .add_filter("DAE", &["dae"])
            .pick_file()
        else {
            return;
        };
        let meta = match dae::read_dae_to_meta(&path, None) {
            Ok(m) => m,
            Err(e) => {
                self.status = format!("DAE import failed: {e}");
                return;
            }
        };
        let new_pmf2 =
            match pmf2::patch_pmf2_with_mesh_updates(&template_data, &meta, 0.0) {
                Some(d) => d,
                None => {
                    self.status = "Failed to patch PMF2 from DAE".to_string();
                    return;
                }
            };
        match self.workspace.replace_stream(stream_index, new_pmf2) {
            Ok(()) => self.status = "Replaced PMF2 stream from DAE".to_string(),
            Err(e) => self.status = format!("Stream replace failed: {e}"),
        }
    }

    fn export_stream_png(&mut self, stream_index: usize) {
        let Some(data) = self.get_stream_data(stream_index) else {
            self.status = "Stream not available".to_string();
            return;
        };
        let image = match crate::texture::GimImage::decode(data) {
            Ok(img) => img,
            Err(e) => {
                self.status = format!("GIM decode failed: {e}");
                return;
            }
        };
        let Some(path) = rfd::FileDialog::new()
            .add_filter("PNG", &["png"])
            .set_file_name("texture.png")
            .save_file()
        else {
            return;
        };
        let mut output =
            image::RgbaImage::new(image.metadata.width as u32, image.metadata.height as u32);
        for (i, pixel) in image.rgba.iter().enumerate() {
            let x = (i % image.metadata.width) as u32;
            let y = (i / image.metadata.width) as u32;
            output.put_pixel(x, y, image::Rgba(*pixel));
        }
        match output.save(&path) {
            Ok(()) => self.status = format!("Exported PNG: {}", path.display()),
            Err(e) => self.status = format!("PNG export failed: {e}"),
        }
    }

    fn replace_stream_png(&mut self, stream_index: usize) {
        let Some(data) = self.get_stream_data(stream_index).map(|d| d.to_vec()) else {
            self.status = "Stream not available".to_string();
            return;
        };
        let image = match crate::texture::GimImage::decode(&data) {
            Ok(img) => img,
            Err(e) => {
                self.status = format!("GIM decode failed: {e}");
                return;
            }
        };
        let Some(path) = rfd::FileDialog::new()
            .add_filter("PNG", &["png"])
            .pick_file()
        else {
            return;
        };
        let png_data = match std::fs::read(&path) {
            Ok(d) => d,
            Err(e) => {
                self.status = format!("Failed to read PNG: {e}");
                return;
            }
        };
        let replaced = match image.replace_png_bytes(&png_data) {
            Ok(d) => d,
            Err(e) => {
                self.status = format!("GIM replace failed: {e}");
                return;
            }
        };
        match self.workspace.replace_stream(stream_index, replaced) {
            Ok(()) => self.status = "Replaced GIM stream from PNG".to_string(),
            Err(e) => self.status = format!("Stream replace failed: {e}"),
        }
    }

    fn get_stream_data(&self, index: usize) -> Option<&[u8]> {
        self.workspace
            .open_pzz()
            .and_then(|pzz| pzz.stream_data().get(index))
            .map(Vec::as_slice)
    }
}

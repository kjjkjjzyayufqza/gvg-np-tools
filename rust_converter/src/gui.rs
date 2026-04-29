use crate::{
    afs::{self, AfsInventory},
    pmf2,
    render::{Pmf2PreviewMesh, PreviewCamera, PreviewLineColor, PreviewState, PreviewViewport},
    save::PzzSavePlanner,
    texture::GimImage,
    workspace::ModWorkspace,
};
use anyhow::Result;
use eframe::egui;
use egui_dock::{DockArea, DockState, Style, TabViewer};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
enum WorkspaceTab {
    Preview3d,
    Pmf2Metadata,
    Pmf2Data,
    GimPreview,
    RawData,
    OperationLog,
    SavePlanner,
}

pub struct GvgModdingApp {
    workspace: ModWorkspace,
    afs_path: Option<PathBuf>,
    inventory: Option<AfsInventory>,
    selected_afs_entry: Option<usize>,
    selected_stream: Option<usize>,
    status: String,
    dock_state: DockState<WorkspaceTab>,
    preview_state: PreviewState,
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
            afs_path: None,
            inventory: None,
            selected_afs_entry: None,
            selected_stream: None,
            status: "Ready".to_string(),
            preview_state: PreviewState::default(),
            dock_state: DockState::new(vec![
                WorkspaceTab::Preview3d,
                WorkspaceTab::Pmf2Metadata,
                WorkspaceTab::Pmf2Data,
                WorkspaceTab::GimPreview,
                WorkspaceTab::RawData,
                WorkspaceTab::SavePlanner,
                WorkspaceTab::OperationLog,
            ]),
        }
    }
}

impl eframe::App for GvgModdingApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| self.top_bar(ui));
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| self.bottom_panel(ui));
        egui::SidePanel::left("asset_tree")
            .resizable(true)
            .default_width(300.0)
            .show(ctx, |ui| self.asset_tree(ui));
        egui::SidePanel::right("inspector")
            .resizable(true)
            .default_width(360.0)
            .show(ctx, |ui| self.inspector(ui));
        egui::CentralPanel::default().show(ctx, |ui| {
            let mut viewer = GvgTabViewer {
                workspace: &mut self.workspace,
                selected_stream: self.selected_stream,
                status: &mut self.status,
                preview_state: &mut self.preview_state,
            };
            DockArea::new(&mut self.dock_state)
                .style(Style::from_egui(ui.style().as_ref()))
                .show_inside(ui, &mut viewer);
        });
    }
}

impl GvgModdingApp {
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("Open AFS/Z_DATA.BIN").clicked() {
                self.open_afs_dialog();
            }
            if ui.button("Open PZZ").clicked() {
                self.open_pzz_dialog();
            }
            if ui.button("Save PZZ As").clicked() {
                self.save_pzz_dialog();
            }
            if ui.button("Patch selected AFS entry").clicked() {
                self.patch_selected_afs_entry_dialog();
            }
            if ui.button("Validate").clicked() {
                self.validate_selection();
            }
        });
    }

    fn bottom_panel(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Status:");
            ui.monospace(&self.status);
        });
    }

    fn asset_tree(&mut self, ui: &mut egui::Ui) {
        ui.heading("Assets");
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let entries = self.workspace.afs_entries().to_vec();
                for entry in entries {
                    let selected = self.selected_afs_entry == Some(entry.index);
                    let label =
                        format!("#{:04} {} ({} bytes)", entry.index, entry.name, entry.size);
                    if ui.selectable_label(selected, label).clicked() {
                        self.selected_afs_entry = Some(entry.index);
                        self.selected_stream = None;
                        if entry.kind == crate::workspace::AssetKind::Pzz {
                            self.status = match self.workspace.open_pzz_entry(entry.index) {
                                Ok(()) => format!("Opened {}", entry.name),
                                Err(error) => format!("Failed to open PZZ entry: {error}"),
                            };
                        } else if self
                            .workspace
                            .open_pzz()
                            .and_then(|pzz| pzz.afs_entry_index())
                            .is_some()
                        {
                            self.workspace.close_open_pzz();
                        }
                    }
                }
                if let Some(pzz) = self.workspace.open_pzz() {
                    egui::CollapsingHeader::new(format!("PZZ: {}", pzz.name()))
                        .default_open(true)
                        .show(ui, |ui| {
                            for stream in pzz.streams() {
                                let selected = self.selected_stream == Some(stream.index);
                                let dirty = if stream.dirty { " *" } else { "" };
                                let label = format!(
                                    "stream{:03} {:?} {} bytes{}",
                                    stream.index, stream.kind, stream.size, dirty
                                );
                                if ui.selectable_label(selected, label).clicked() {
                                    self.selected_stream = Some(stream.index);
                                }
                            }
                        });
                }
            });
    }

    fn inspector(&self, ui: &mut egui::Ui) {
        ui.heading("Inspector");
        if let Some(stream) = self.selected_stream.and_then(|i| {
            self.workspace
                .open_pzz()
                .and_then(|pzz| pzz.streams().get(i))
        }) {
            ui.label(format!("Stream: {}", stream.name));
            ui.label(format!("Type: {:?}", stream.kind));
            ui.label(format!("Size: {} bytes", stream.size));
            ui.label(format!("Dirty: {}", stream.dirty));
            return;
        }
        if let Some(index) = self.selected_afs_entry {
            if let Some(entry) = self
                .workspace
                .afs_entries()
                .iter()
                .find(|entry| entry.index == index)
            {
                ui.label(format!("AFS entry: {}", entry.name));
                ui.label(format!("Offset: {}", entry.offset));
                ui.label(format!("Size: {}", entry.size));
                ui.label(format!("Type: {:?}", entry.kind));
                if entry.kind == crate::workspace::AssetKind::Pzz {
                    ui.label("Selecting this entry expands its PZZ streams in the asset tree.");
                }
                return;
            }
        }
        ui.label("Select an AFS entry or PZZ stream.");
    }

    fn open_afs_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("AFS/BIN", &["bin", "afs"])
            .pick_file()
        else {
            return;
        };
        self.status = match self.open_afs(path) {
            Ok(()) => "AFS loaded".to_string(),
            Err(error) => format!("Failed to open AFS: {error}"),
        };
    }

    fn open_pzz_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("PZZ", &["pzz"])
            .pick_file()
        else {
            return;
        };
        self.status = match std::fs::read(&path)
            .map_err(anyhow::Error::from)
            .and_then(|data| {
                let name = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("model.pzz")
                    .to_string();
                self.workspace = ModWorkspace::open_pzz_bytes(name, data)?;
                Ok(())
            }) {
            Ok(()) => "PZZ loaded".to_string(),
            Err(error) => format!("Failed to open PZZ: {error}"),
        };
    }

    fn save_pzz_dialog(&mut self) {
        let Some(pzz) = self.workspace.open_pzz() else {
            self.status = "No PZZ is open".to_string();
            return;
        };
        let Some(path) = rfd::FileDialog::new().set_file_name(pzz.name()).save_file() else {
            return;
        };
        let result = self
            .plan_pzz_save()
            .and_then(|rebuilt| std::fs::write(&path, rebuilt).map_err(anyhow::Error::from));
        self.status = match result {
            Ok(()) => format!("Saved PZZ {}", path.display()),
            Err(error) => format!("Failed to save PZZ: {error}"),
        };
    }

    fn validate_selection(&mut self) {
        self.status = if let Some(pzz) = self.workspace.open_pzz() {
            match self.plan_pzz_save() {
                Ok(rebuilt) => format!(
                    "Validated {} streams, rebuilt size {} bytes",
                    pzz.stream_data().len(),
                    rebuilt.len()
                ),
                Err(error) => format!("Validation failed: {error}"),
            }
        } else {
            "Nothing to validate".to_string()
        };
    }

    fn open_afs(&mut self, path: PathBuf) -> Result<()> {
        let data = std::fs::read(&path)?;
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("Z_DATA.BIN")
            .to_string();
        self.workspace = ModWorkspace::open_afs_bytes(name, data)?;
        let inventory = afs::scan_inventory(&std::fs::read(&path)?, None)?;
        self.inventory = Some(inventory);
        self.afs_path = Some(path);
        self.selected_afs_entry = None;
        self.selected_stream = None;
        Ok(())
    }

    fn patch_selected_afs_entry_dialog(&mut self) {
        let Some(entry_index) = self.selected_afs_entry else {
            self.status = "Select an AFS PZZ entry before patching".to_string();
            return;
        };
        let Some(afs_path) = self.afs_path.as_ref() else {
            self.status = "Open an AFS/Z_DATA.BIN before patching".to_string();
            return;
        };
        let Some(pzz) = self.workspace.open_pzz() else {
            self.status = "Open a PZZ entry before patching".to_string();
            return;
        };
        if pzz.afs_entry_index() != Some(entry_index) {
            self.status = "Open the selected PZZ entry before patching it".to_string();
            return;
        }
        let Some(output_path) = rfd::FileDialog::new()
            .set_file_name("Z_DATA_patched.BIN")
            .save_file()
        else {
            return;
        };
        let result = self.plan_pzz_save().and_then(|rebuilt_pzz| {
            let afs_data = std::fs::read(afs_path)?;
            let patched = afs::patch_entry_bytes(&afs_data, entry_index, &rebuilt_pzz)?;
            std::fs::write(&output_path, patched)?;
            Ok(())
        });
        self.status = match result {
            Ok(()) => format!(
                "Patched entry {} from {} into {}",
                entry_index,
                pzz.name(),
                output_path.display()
            ),
            Err(error) => format!("Failed to patch AFS: {error}"),
        };
    }

    fn plan_pzz_save(&self) -> Result<Vec<u8>> {
        let pzz = self
            .workspace
            .open_pzz()
            .ok_or_else(|| anyhow::anyhow!("no PZZ is open"))?;
        let planner = PzzSavePlanner::new(pzz.original(), pzz.stream_data().to_vec());
        let original_stream_count = crate::pzz::inspect_pzz(pzz.original())?.stream_count;
        if original_stream_count == pzz.stream_data().len() {
            Ok(planner.plan_preserving_layout()?.rebuilt_pzz)
        } else {
            Ok(planner.plan_stream_archive_rebuild()?.rebuilt_pzz)
        }
    }
}

struct GvgTabViewer<'a> {
    workspace: &'a mut ModWorkspace,
    selected_stream: Option<usize>,
    status: &'a mut String,
    preview_state: &'a mut PreviewState,
}

impl TabViewer for GvgTabViewer<'_> {
    type Tab = WorkspaceTab;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        match tab {
            WorkspaceTab::Preview3d => "3D Preview",
            WorkspaceTab::Pmf2Metadata => "PMF2 Metadata",
            WorkspaceTab::Pmf2Data => "PMF2 Data",
            WorkspaceTab::GimPreview => "GIM Preview",
            WorkspaceTab::RawData => "Raw Data",
            WorkspaceTab::OperationLog => "Operation Log",
            WorkspaceTab::SavePlanner => "Save Planner",
        }
        .into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        match tab {
            WorkspaceTab::Preview3d => self.preview_3d(ui),
            WorkspaceTab::Pmf2Metadata => self.pmf2_metadata(ui),
            WorkspaceTab::Pmf2Data => self.pmf2_data(ui),
            WorkspaceTab::GimPreview => self.gim_preview(ui),
            WorkspaceTab::RawData => self.raw_data(ui),
            WorkspaceTab::OperationLog => self.operation_log(ui),
            WorkspaceTab::SavePlanner => self.save_planner(ui),
        }
    }
}

impl GvgTabViewer<'_> {
    fn selected_stream_data(&self) -> Option<&[u8]> {
        let index = self.selected_stream?;
        self.workspace
            .open_pzz()
            .and_then(|pzz| pzz.stream_data().get(index))
            .map(Vec::as_slice)
    }

    fn preview_3d(&mut self, ui: &mut egui::Ui) {
        let Some(data) = self.selected_stream_data() else {
            ui.label("Select a PMF2 stream to preview.");
            return;
        };
        if crate::pzz::classify_stream(data) != "pmf2" {
            ui.label("Selected stream is not PMF2.");
            return;
        }
        let (meshes, _, _, _) = pmf2::extract_per_bone_meshes(data, false);
        match Pmf2PreviewMesh::from_bone_meshes(&meshes) {
            Ok(preview) => {
                self.preview_controls(ui, &preview);
                let (response, painter) =
                    ui.allocate_painter(ui.available_size(), egui::Sense::drag());
                let rect = response.rect;
                if response.dragged() {
                    let delta = ui.input(|input| input.pointer.delta());
                    let camera = self
                        .preview_state
                        .camera
                        .get_or_insert_with(|| PreviewCamera::frame_bounds(preview.bounds));
                    camera.orbit(delta.x * 0.01, delta.y * 0.01);
                    ui.ctx().request_repaint();
                }
                if response.hovered() {
                    let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                    if scroll.abs() > f32::EPSILON {
                        let camera = self
                            .preview_state
                            .camera
                            .get_or_insert_with(|| PreviewCamera::frame_bounds(preview.bounds));
                        camera.zoom((-scroll * 0.001).clamp(-0.5, 0.5));
                        ui.ctx().request_repaint();
                    }
                }
                let camera = *self
                    .preview_state
                    .camera
                    .get_or_insert_with(|| PreviewCamera::frame_bounds(preview.bounds));
                let viewport = PreviewViewport {
                    width: rect.width().max(1.0),
                    height: rect.height().max(1.0),
                };
                match preview.project(&camera, viewport, &self.preview_state.visibility) {
                    Ok(projected) => {
                        for triangle in projected.triangles {
                            let points = triangle
                                .points
                                .map(|p| egui::pos2(rect.left() + p[0], rect.top() + p[1]));
                            if !self.preview_state.wireframe {
                                painter.add(egui::Shape::convex_polygon(
                                    points.to_vec(),
                                    egui::Color32::from_rgba_unmultiplied(60, 130, 190, 60),
                                    egui::Stroke::NONE,
                                ));
                            }
                            painter.line_segment(
                                [points[0], points[1]],
                                egui::Stroke::new(1.0, egui::Color32::LIGHT_BLUE),
                            );
                            painter.line_segment(
                                [points[1], points[2]],
                                egui::Stroke::new(1.0, egui::Color32::LIGHT_BLUE),
                            );
                            painter.line_segment(
                                [points[2], points[0]],
                                egui::Stroke::new(1.0, egui::Color32::LIGHT_BLUE),
                            );
                        }
                        for line in projected.bounds {
                            draw_projected_line(&painter, rect, line, 1.0);
                        }
                        for line in projected.axes {
                            draw_projected_line(&painter, rect, line, 2.0);
                        }
                    }
                    Err(error) => {
                        painter.text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            format!("Projection failed: {error}"),
                            egui::FontId::monospace(14.0),
                            egui::Color32::LIGHT_RED,
                        );
                    }
                }
            }
            Err(error) => {
                ui.label(format!("PMF2 preview unavailable: {error}"));
            }
        }
    }

    fn preview_controls(&mut self, ui: &mut egui::Ui, preview: &Pmf2PreviewMesh) {
        ui.horizontal_wrapped(|ui| {
            ui.label(format!(
                "{} vertices, {} triangles",
                preview.vertices.len(),
                preview.indices.len() / 3
            ));
            if ui.button("Frame").clicked() {
                self.preview_state.camera = Some(PreviewCamera::frame_bounds(preview.bounds));
            }
            ui.checkbox(&mut self.preview_state.wireframe, "Wireframe");
            ui.checkbox(&mut self.preview_state.visibility.show_axes, "Axes");
            ui.checkbox(&mut self.preview_state.visibility.show_bounds, "Bounds");
        });
        ui.collapsing("Bone Visibility", |ui| {
            for bone_index in preview.bones() {
                let mut visible = self.preview_state.visibility.is_bone_visible(bone_index);
                if ui
                    .checkbox(&mut visible, format!("Bone {}", bone_index))
                    .changed()
                {
                    self.preview_state
                        .visibility
                        .set_bone_visible(bone_index, visible);
                }
            }
        });
    }

    fn pmf2_metadata(&self, ui: &mut egui::Ui) {
        let Some(data) = self.selected_stream_data() else {
            ui.label("Select a PMF2 stream.");
            return;
        };
        if crate::pzz::classify_stream(data) != "pmf2" {
            ui.label("Selected stream is not PMF2.");
            return;
        }
        let (sections, bbox) = pmf2::parse_pmf2_sections(data);
        ui.label(format!(
            "BBox scale: {:.6}, {:.6}, {:.6}",
            bbox[0], bbox[1], bbox[2]
        ));
        ui.label(format!("Sections: {}", sections.len()));
        egui::ScrollArea::vertical().show(ui, |ui| {
            for section in sections {
                ui.collapsing(section.name, |ui| {
                    ui.label(format!("Index: {}", section.index));
                    ui.label(format!("Parent: {}", section.parent));
                    ui.label(format!("Has mesh: {}", section.has_mesh));
                    ui.label(format!("Offset: {}", section.offset));
                    ui.label(format!("Size: {}", section.size));
                    ui.label(format!("Category: {}", section.category));
                });
            }
        });
    }

    fn pmf2_data(&self, ui: &mut egui::Ui) {
        let Some(data) = self.selected_stream_data() else {
            ui.label("Select a PMF2 stream.");
            return;
        };
        if crate::pzz::classify_stream(data) != "pmf2" {
            ui.label("Selected stream is not PMF2.");
            return;
        }
        let (meshes, _, _, _) = pmf2::extract_per_bone_meshes(data, false);
        egui::ScrollArea::vertical().show(ui, |ui| {
            for mesh in meshes {
                ui.collapsing(mesh.bone_name, |ui| {
                    ui.label(format!("Vertices: {}", mesh.vertices.len()));
                    ui.label(format!("Faces: {}", mesh.faces.len()));
                    ui.label(format!("UV: {}", mesh.has_uv));
                    ui.label(format!("Normals: {}", mesh.has_normals));
                    ui.label(format!("VTypes: {:?}", mesh.vtypes));
                });
            }
        });
    }

    fn gim_preview(&mut self, ui: &mut egui::Ui) {
        let Some(data) = self.selected_stream_data() else {
            ui.label("Select a GIM stream.");
            return;
        };
        if crate::pzz::classify_stream(data) != "gim" {
            ui.label("Selected stream is not GIM.");
            return;
        }
        match GimImage::decode(data) {
            Ok(image) => {
                ui.label(format!(
                    "{}x{} {:?}",
                    image.metadata.width, image.metadata.height, image.metadata.format
                ));
                ui.horizontal(|ui| {
                    if ui.button("Export PNG").clicked() {
                        *self.status = match self.export_selected_gim_png(&image) {
                            Ok(path) => format!("Exported PNG {}", path.display()),
                            Err(error) => format!("Failed to export PNG: {error}"),
                        };
                    }
                    if ui.button("Replace From PNG").clicked() {
                        *self.status = match self.replace_selected_gim_png(&image) {
                            Ok(()) => "Replaced GIM stream from PNG".to_string(),
                            Err(error) => format!("Failed to replace GIM: {error}"),
                        };
                    }
                });
                let flat = image.rgba.iter().flat_map(|p| *p).collect::<Vec<_>>();
                let color_image = egui::ColorImage::from_rgba_unmultiplied(
                    [image.metadata.width, image.metadata.height],
                    &flat,
                );
                let texture = ui.ctx().load_texture(
                    "gim_preview",
                    color_image,
                    egui::TextureOptions::NEAREST,
                );
                ui.image((texture.id(), texture.size_vec2()));
            }
            Err(error) => {
                ui.label(format!("GIM decode failed: {error}"));
            }
        }
    }

    fn raw_data(&self, ui: &mut egui::Ui) {
        let Some(data) = self.selected_stream_data() else {
            ui.label("Select a stream.");
            return;
        };
        ui.label(format!("{} bytes", data.len()));
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (row, chunk) in data.chunks(16).take(256).enumerate() {
                ui.monospace(format!(
                    "{:08X}: {}",
                    row * 16,
                    chunk
                        .iter()
                        .map(|byte| format!("{byte:02X}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                ));
            }
        });
    }

    fn export_selected_gim_png(&self, image: &GimImage) -> Result<std::path::PathBuf> {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("PNG", &["png"])
            .set_file_name("texture.png")
            .save_file()
        else {
            return Err(anyhow::anyhow!("PNG export cancelled"));
        };
        let mut output =
            image::RgbaImage::new(image.metadata.width as u32, image.metadata.height as u32);
        for (index, pixel) in image.rgba.iter().enumerate() {
            let x = (index % image.metadata.width) as u32;
            let y = (index / image.metadata.width) as u32;
            output.put_pixel(x, y, image::Rgba(*pixel));
        }
        output.save(&path)?;
        Ok(path)
    }

    fn replace_selected_gim_png(&mut self, image: &GimImage) -> Result<()> {
        let stream_index = self
            .selected_stream
            .ok_or_else(|| anyhow::anyhow!("no stream selected"))?;
        let Some(path) = rfd::FileDialog::new()
            .add_filter("PNG", &["png"])
            .pick_file()
        else {
            return Err(anyhow::anyhow!("PNG import cancelled"));
        };
        let png = std::fs::read(&path)?;
        let replaced = image.replace_png_bytes(&png)?;
        self.workspace.replace_stream(stream_index, replaced)
    }

    fn operation_log(&self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            for line in self.workspace.operation_log() {
                ui.label(line);
            }
        });
    }

    fn save_planner(&self, ui: &mut egui::Ui) {
        let Some(pzz) = self.workspace.open_pzz() else {
            ui.label("Open a PZZ to inspect save impact.");
            return;
        };
        let planner = PzzSavePlanner::new(pzz.original(), pzz.stream_data().to_vec());
        let original_stream_count = crate::pzz::inspect_pzz(pzz.original())
            .map(|info| info.stream_count)
            .unwrap_or(0);
        let plan = if original_stream_count == pzz.stream_data().len() {
            planner.plan_preserving_layout()
        } else {
            planner.plan_stream_archive_rebuild()
        };
        match plan {
            Ok(plan) => {
                ui.label(format!("Original size: {} bytes", plan.original_size));
                ui.label(format!("Rebuilt size: {} bytes", plan.rebuilt_size));
                ui.label(format!("Streams: {}", plan.stream_count));
                ui.label(format!("Changed streams: {}", plan.changed_stream_count));
                ui.label(format!("Tail recomputed: {}", plan.tail_recomputed));
                for message in plan.validation_messages {
                    ui.label(message);
                }
            }
            Err(error) => {
                ui.label(format!("Save plan failed: {error}"));
            }
        }
    }
}

fn draw_projected_line(
    painter: &egui::Painter,
    rect: egui::Rect,
    line: crate::render::ProjectedLine,
    width: f32,
) {
    let color = match line.color {
        PreviewLineColor::XAxis => egui::Color32::RED,
        PreviewLineColor::YAxis => egui::Color32::GREEN,
        PreviewLineColor::ZAxis => egui::Color32::BLUE,
        PreviewLineColor::Bounds => egui::Color32::GRAY,
    };
    painter.line_segment(
        [
            egui::pos2(rect.left() + line.start[0], rect.top() + line.start[1]),
            egui::pos2(rect.left() + line.end[0], rect.top() + line.end[1]),
        ],
        egui::Stroke::new(width, color),
    );
}

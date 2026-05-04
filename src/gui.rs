mod asset_tree;
mod editors;
mod fonts;
mod gim_preview_cache;
mod inspector;
mod persist;
mod preview;
mod status;

use crate::{
    afs, dae,
    gpu_renderer::{GpuMesh, GpuRenderStats, GpuRenderer},
    pmf2, pzz,
    render::PreviewState,
    save::rebuild_pzz_payload_cached,
    texture::{GimImage, GimReplaceFormat},
    workspace::{ModWorkspace, PzzWorkspace},
};
use anyhow::Result;
use asset_tree::{AssetTreeState, TreeAction};
use editors::EditorWindows;
use eframe::egui;
use eframe::egui_wgpu::wgpu;
use std::path::Path;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Instant;

pub(super) fn remember_parent_dir(memory: &mut Option<std::path::PathBuf>, path: &Path) {
    if let Some(dir) = path.parent() {
        *memory = Some(dir.to_path_buf());
    }
}

pub(super) fn touch_dialog_dir_parent(
    slot: &mut Option<std::path::PathBuf>,
    path: &Path,
    gui_dirty: &mut bool,
) {
    remember_parent_dir(slot, path);
    *gui_dirty = true;
}

pub struct WgpuState {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub renderer: std::sync::Arc<egui::mutex::RwLock<eframe::egui_wgpu::Renderer>>,
}

pub struct GvgModdingApp {
    workspace: ModWorkspace,
    tree_state: AssetTreeState,
    preview_state: PreviewState,
    editors: EditorWindows,
    inspector_gim_cache: gim_preview_cache::GimPreviewCache,
    inspector_pmf2_cache: inspector::Pmf2SummaryCache,
    status: String,
    show_left_panel: bool,
    show_right_panel: bool,
    gpu_renderer: Option<GpuRenderer>,
    gpu_mesh: Option<GpuMesh>,
    gpu_texture_bind_group: Option<wgpu::BindGroup>,
    gpu_mesh_stream_index: Option<usize>,
    gpu_mesh_pzz_revision: Option<u64>,
    gpu_mesh_visibility_key: u64,
    last_preview_render_stats: Option<GpuRenderStats>,
    wgpu_state: Option<WgpuState>,
    recent_afs_paths: Vec<std::path::PathBuf>,
    last_dir_open_afs: Option<std::path::PathBuf>,
    last_dir_open_pzz: Option<std::path::PathBuf>,
    last_dir_save_pzz_as: Option<std::path::PathBuf>,
    last_dir_patch_afs_entry: Option<std::path::PathBuf>,
    last_dir_write_modified_pzz_to_afs: Option<std::path::PathBuf>,
    last_dir_cwcheat: Option<std::path::PathBuf>,
    last_dir_export_entry_raw: Option<std::path::PathBuf>,
    last_dir_export_stream_raw: Option<std::path::PathBuf>,
    last_dir_export_stream_dae: Option<std::path::PathBuf>,
    last_dir_replace_stream_dae: Option<std::path::PathBuf>,
    last_dir_replace_stream_pmf2: Option<std::path::PathBuf>,
    last_dir_export_stream_png: Option<std::path::PathBuf>,
    last_dir_replace_stream_png: Option<std::path::PathBuf>,
    gim_replace_format: GimReplaceFormat,
    pending_dae_replace_config: Option<PendingDaeReplaceConfig>,
    pending_gim_replace_config: Option<PendingGimReplaceConfig>,
    /// Persisted CW cheat INI path; also drives the CWCheat Editor buffer reload.
    cwcheat_file_path: Option<std::path::PathBuf>,
    auto_update_cwcheat_on_save_afs: bool,
    cwcheat_settings_modal_open: bool,
    gui_state_dirty: bool,
    /// User-dismissable error dialog; same text is mirrored in `status`.
    pending_alert: Option<String>,
    save_afs_job: Option<SaveAfsJob>,
    /// After the real eframe/winit loop starts, fit **once** to monitor (cannot probe before `run_native`).
    #[cfg(not(target_arch = "wasm32"))]
    needs_initial_window_fit: bool,
    #[cfg(not(target_arch = "wasm32"))]
    initial_window_fit_miss_frames: u32,
}

struct SaveAfsJob {
    receiver: Receiver<Result<usize, String>>,
    output_path: std::path::PathBuf,
    dirty_count: usize,
}

struct PendingDaeReplaceConfig {
    stream_index: usize,
    template_data: Vec<u8>,
    dae_path: std::path::PathBuf,
    flip_uv_v: bool,
}

struct PendingGimReplaceConfig {
    stream_index: usize,
    source_image: GimImage,
    png_path: std::path::PathBuf,
    png_data: Vec<u8>,
    preview_image: egui::ColorImage,
    preview_texture: Option<egui::TextureHandle>,
    selected_format: GimReplaceFormat,
}

/// Default logical inner size (**1920×1080**, Full HD).
const DEFAULT_WINDOW_INNER_W: f32 = 1920.0;
const DEFAULT_WINDOW_INNER_H: f32 = 1080.0;
/// Applied to egui-reported monitor logical size; leaves margin for decorations / taskbar.
const WINDOW_FIT_OCCUPY_FRAC: f32 = 0.94;

#[cfg(not(target_arch = "wasm32"))]
fn inner_size_points_for_monitor(mon: egui::Vec2) -> egui::Vec2 {
    let avail_w = mon.x * WINDOW_FIT_OCCUPY_FRAC;
    let avail_h = mon.y * WINDOW_FIT_OCCUPY_FRAC;
    let s = (avail_w / DEFAULT_WINDOW_INNER_W)
        .min(avail_h / DEFAULT_WINDOW_INNER_H)
        .min(1.0);
    egui::vec2(DEFAULT_WINDOW_INNER_W * s, DEFAULT_WINDOW_INNER_H * s)
}

pub fn run_native() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([DEFAULT_WINDOW_INNER_W, DEFAULT_WINDOW_INNER_H])
            .with_min_inner_size([960.0, 640.0])
            .with_clamp_size_to_monitor_size(true)
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "GVG Modding Tool",
        options,
        Box::new(|cc| Ok(Box::new(GvgModdingApp::new(cc)))),
    )
}

impl GvgModdingApp {
    #[cfg(not(target_arch = "wasm32"))]
    fn maybe_apply_initial_window_fit(&mut self, ctx: &egui::Context) {
        if !self.needs_initial_window_fit {
            return;
        }

        let monitor_size = ctx.input(|i| i.viewport().monitor_size);
        let Some(mon) = monitor_size else {
            self.initial_window_fit_miss_frames =
                self.initial_window_fit_miss_frames.saturating_add(1);
            if self.initial_window_fit_miss_frames > 120 {
                self.needs_initial_window_fit = false;
            }
            return;
        };

        let size = inner_size_points_for_monitor(mon);
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        self.needs_initial_window_fit = false;
    }

    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (gpu_renderer, wgpu_state) = if let Some(rs) = cc.wgpu_render_state.as_ref() {
            let renderer = GpuRenderer::new(&rs.device, &rs.queue);
            eprintln!("[gui] wgpu renderer initialized");
            let state = WgpuState {
                device: rs.device.clone(),
                queue: rs.queue.clone(),
                renderer: std::sync::Arc::clone(&rs.renderer),
            };
            (Some(renderer), Some(state))
        } else {
            eprintln!("[gui] WARNING: no wgpu render state available, 3D preview disabled");
            (None, None)
        };

        fonts::install_cjk_fonts(&cc.egui_ctx);

        let persisted = persist::load();
        if let Some(p) = persist::state_file_path() {
            eprintln!("[gui] GUI persistence: {}", p.display());
        }

        let mut app = Self {
            workspace: ModWorkspace::default(),
            tree_state: AssetTreeState::default(),
            preview_state: PreviewState::default(),
            editors: EditorWindows::default(),
            inspector_gim_cache: gim_preview_cache::GimPreviewCache::default(),
            inspector_pmf2_cache: inspector::Pmf2SummaryCache::default(),
            status: "Ready".to_string(),
            show_left_panel: true,
            show_right_panel: true,
            gpu_renderer,
            gpu_mesh: None,
            gpu_texture_bind_group: None,
            gpu_mesh_stream_index: None,
            gpu_mesh_pzz_revision: None,
            gpu_mesh_visibility_key: 0,
            last_preview_render_stats: None,
            wgpu_state,
            recent_afs_paths: persisted.recent_afs_paths.clone(),
            last_dir_open_afs: persisted.last_dir_open_afs,
            last_dir_open_pzz: persisted.last_dir_open_pzz,
            last_dir_save_pzz_as: persisted.last_dir_save_pzz_as,
            last_dir_patch_afs_entry: persisted.last_dir_patch_afs_entry,
            last_dir_write_modified_pzz_to_afs: persisted.last_dir_write_modified_pzz_to_afs,
            last_dir_cwcheat: persisted.last_dir_cwcheat,
            last_dir_export_entry_raw: persisted.last_dir_export_entry_raw,
            last_dir_export_stream_raw: persisted.last_dir_export_stream_raw,
            last_dir_export_stream_dae: persisted.last_dir_export_stream_dae,
            last_dir_replace_stream_dae: persisted.last_dir_replace_stream_dae,
            last_dir_replace_stream_pmf2: persisted.last_dir_replace_stream_pmf2,
            last_dir_export_stream_png: persisted.last_dir_export_stream_png,
            last_dir_replace_stream_png: persisted.last_dir_replace_stream_png,
            gim_replace_format: persisted
                .gim_replace_format
                .as_deref()
                .and_then(GimReplaceFormat::from_key)
                .unwrap_or(GimReplaceFormat::Auto),
            pending_dae_replace_config: None,
            pending_gim_replace_config: None,
            cwcheat_file_path: persisted.cwcheat_file_path.clone(),
            auto_update_cwcheat_on_save_afs: persisted.auto_update_cwcheat_on_save_afs,
            cwcheat_settings_modal_open: false,
            gui_state_dirty: false,
            pending_alert: None,
            save_afs_job: None,
            #[cfg(not(target_arch = "wasm32"))]
            needs_initial_window_fit: true,
            #[cfg(not(target_arch = "wasm32"))]
            initial_window_fit_miss_frames: 0,
        };
        app.reload_cwcheat_editor_text_from_path();
        app
    }
}

impl eframe::App for GvgModdingApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(not(target_arch = "wasm32"))]
        self.maybe_apply_initial_window_fit(ctx);

        self.poll_save_afs_job(ctx);

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
                        &mut self.inspector_gim_cache,
                        &mut self.inspector_pmf2_cache,
                        &mut self.preview_state.visibility,
                    );
                });
        }

        self.update_gpu_mesh();

        egui::CentralPanel::default().show(ctx, |ui| {
            self.show_3d_preview(ui, ctx);
        });

        let editor_result = editors::show_editor_windows(
            ctx,
            &mut self.workspace,
            &mut self.editors,
            &mut self.last_dir_save_pzz_as,
            &mut self.last_dir_patch_afs_entry,
            &mut self.last_dir_export_stream_png,
            &mut self.last_dir_replace_stream_png,
            &mut self.last_dir_cwcheat,
            &mut self.cwcheat_file_path,
            self.gim_replace_format,
        );
        self.gui_state_dirty |= editor_result.dirs_changed;
        if editor_result.preview_changed {
            self.gpu_mesh_stream_index = None;
            self.gpu_mesh = None;
            self.gpu_texture_bind_group = None;
            self.preview_state.camera = None;
        }
        if let Some(msg) = editor_result.status {
            self.status = msg.clone();
            if editor_result.error_modal {
                self.pending_alert = Some(msg);
            }
        }

        self.show_dae_replace_config_modal(ctx);
        self.show_gim_replace_config_modal(ctx);
        Self::show_error_modal(ctx, &mut self.pending_alert);
        self.show_cwcheat_settings_modal(ctx);

        self.try_flush_gui_state_disk();
    }

    fn on_exit(&mut self) {
        if let Err(e) = persist::save(&self.gui_state_snapshot()) {
            eprintln!("[gui] final GUI persistence write failed: {}", e);
        }
    }
}

impl GvgModdingApp {
    fn show_error_modal(ctx: &egui::Context, pending: &mut Option<String>) {
        let Some(message) = pending.clone() else {
            return;
        };

        let modal = egui::Modal::new(egui::Id::new("gvg_gui_error_modal")).show(ctx, |ui| {
            ui.set_min_width(380.0);
            egui::ScrollArea::vertical()
                .max_height(280.0)
                .show(ui, |ui| {
                    ui.label(&message);
                });
            ui.separator();
            ui.button("OK").clicked()
        });

        let closed = modal.inner || modal.should_close();
        if closed {
            *pending = None;
        }
    }

    fn notify_error(&mut self, msg: String) {
        self.status = msg.clone();
        self.pending_alert = Some(msg);
    }

    fn reload_cwcheat_editor_text_from_path(&mut self) {
        let Some(p) = self.cwcheat_file_path.as_ref() else {
            self.editors.set_cwcheat_editor_text(String::new());
            return;
        };
        if !p.exists() {
            self.editors.set_cwcheat_editor_text(String::new());
            return;
        }
        match std::fs::read_to_string(p) {
            Ok(text) => self.editors.set_cwcheat_editor_text(text),
            Err(e) => {
                let msg = format!(
                    "CW cheat reload failed (could not read {}): {e}",
                    p.display()
                );
                eprintln!("[gui] {}", msg);
                self.workspace.push_log(msg);
                self.editors.set_cwcheat_editor_text(String::new());
            }
        }
    }

    fn maybe_auto_update_cwcheat_after_afs_save(&mut self) {
        if !self.auto_update_cwcheat_on_save_afs {
            return;
        }
        let Some(path) = self.cwcheat_file_path.as_ref().filter(|p| p.exists()) else {
            return;
        };
        let Ok(current_text) = std::fs::read_to_string(path) else {
            self.workspace.push_log(
                "CW cheat auto-update skipped: could not read the cheat file.".to_string(),
            );
            return;
        };
        if current_text != self.editors.cwcheat_editor_text() {
            self.workspace.push_log(
                "CW cheat auto-update skipped: editor buffer differs from file on disk (save or reload to sync)."
                    .to_string(),
            );
            return;
        }
        match editors::update_cwcheat_body_sizes(&current_text, &mut self.workspace) {
            Ok((updated, entry_count)) => {
                if let Err(e) = std::fs::write(path, &updated) {
                    self.workspace.push_log(format!(
                        "CW cheat auto-update failed writing {}: {e}",
                        path.display()
                    ));
                    return;
                }
                self.editors.set_cwcheat_editor_text(updated);
                self.workspace.push_log(format!(
                    "CW cheat auto-updated ({} body_size lines) -> {}",
                    entry_count,
                    path.display()
                ));
            }
            Err(e) => {
                self.workspace
                    .push_log(format!("CW cheat auto-update skipped: {e}"));
            }
        }
    }

    fn show_cwcheat_settings_modal(&mut self, ctx: &egui::Context) {
        if !self.cwcheat_settings_modal_open {
            return;
        }

        let modal = egui::Modal::new(egui::Id::new("gvg_cwcheat_settings_modal")).show(ctx, |ui| {
            ui.set_min_width(440.0);
            ui.heading("CW Cheat");
            ui.separator();

            if !editors::cwcheat_ini_path_resolves(&self.cwcheat_file_path) {
                ui.label(egui::RichText::new(editors::CWCHEAT_UNRESOLVED_PATH_HINT).weak());
                ui.add_space(8.0);
            }

            if ui
                .checkbox(
                    &mut self.auto_update_cwcheat_on_save_afs,
                    "After Save AFS As, auto-compute body sizes and update this CW cheat file",
                )
                .changed()
            {
                self.mark_gui_state_dirty();
            }
            ui.label(
                egui::RichText::new(
                    "When enabled, each successful Save AFS As reads the cheat file from disk, regenerates PZZ Modding body_size entries from the current workspace, writes back, and refreshes the editor — only if the editor buffer still matches the file (save or reload after manual edits).",
                )
                .weak()
                .small(),
            );

            ui.add_space(8.0);
            ui.label("CW cheat file (.ini):");
            match &self.cwcheat_file_path {
                Some(p) => {
                    ui.label(egui::RichText::new(p.display().to_string()).weak().monospace());
                }
                None => {
                    ui.label(egui::RichText::new("(none)").weak());
                }
            }

            ui.horizontal(|ui| {
                if ui.button("Choose CW cheat (.ini)…").clicked() {
                    if let Some(path) = editors::cwcheat_pick_ini_open(
                        &self.last_dir_cwcheat,
                        &self.cwcheat_file_path,
                    ) {
                        touch_dialog_dir_parent(
                            &mut self.last_dir_cwcheat,
                            &path,
                            &mut self.gui_state_dirty,
                        );
                        self.cwcheat_file_path = Some(path);
                        self.reload_cwcheat_editor_text_from_path();
                    }
                }
            });

            ui.separator();
            ui.button("Close").clicked()
        });

        let closed = modal.inner || modal.should_close();
        if closed {
            self.cwcheat_settings_modal_open = false;
        }
    }

    fn poll_save_afs_job(&mut self, ctx: &egui::Context) {
        let Some(job) = self.save_afs_job.as_ref() else {
            return;
        };
        match job.receiver.try_recv() {
            Ok(result) => {
                let job = self.save_afs_job.take().expect("save job exists");
                match result {
                    Ok(byte_count) => {
                        touch_dialog_dir_parent(
                            &mut self.last_dir_write_modified_pzz_to_afs,
                            &job.output_path,
                            &mut self.gui_state_dirty,
                        );
                        let mib = byte_count as f64 / (1024.0 * 1024.0);
                        let summary = format!(
                            "Saved AFS with {} modified PZZ entries ({:.1} MB) -> {}",
                            job.dirty_count,
                            mib,
                            job.output_path.display()
                        );
                        self.workspace.push_log(summary.clone());
                        self.status = summary;
                        self.maybe_auto_update_cwcheat_after_afs_save();
                    }
                    Err(e) => self.notify_error(format!("Failed to save AFS: {e}")),
                }
            }
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
            Err(TryRecvError::Disconnected) => {
                self.save_afs_job = None;
                self.notify_error("Failed to save AFS: background worker stopped.".to_owned());
            }
        }
    }

    fn gui_state_snapshot(&self) -> persist::PersistedGuiState {
        persist::PersistedGuiState {
            last_dir_open_afs: self.last_dir_open_afs.clone(),
            last_dir_open_pzz: self.last_dir_open_pzz.clone(),
            last_dir_save_pzz_as: self.last_dir_save_pzz_as.clone(),
            last_dir_patch_afs_entry: self.last_dir_patch_afs_entry.clone(),
            last_dir_write_modified_pzz_to_afs: self.last_dir_write_modified_pzz_to_afs.clone(),
            last_dir_cwcheat: self.last_dir_cwcheat.clone(),
            last_dir_export_entry_raw: self.last_dir_export_entry_raw.clone(),
            last_dir_export_stream_raw: self.last_dir_export_stream_raw.clone(),
            last_dir_export_stream_dae: self.last_dir_export_stream_dae.clone(),
            last_dir_replace_stream_dae: self.last_dir_replace_stream_dae.clone(),
            last_dir_replace_stream_pmf2: self.last_dir_replace_stream_pmf2.clone(),
            last_dir_export_stream_png: self.last_dir_export_stream_png.clone(),
            last_dir_replace_stream_png: self.last_dir_replace_stream_png.clone(),
            gim_replace_format: Some(self.gim_replace_format.key().to_string()),
            cwcheat_file_path: self.cwcheat_file_path.clone(),
            auto_update_cwcheat_on_save_afs: self.auto_update_cwcheat_on_save_afs,
            recent_afs_paths: self.recent_afs_paths.clone(),
        }
    }

    fn mark_gui_state_dirty(&mut self) {
        self.gui_state_dirty = true;
    }

    fn record_recent_afs_open_success(&mut self, path: std::path::PathBuf) {
        self.recent_afs_paths.retain(|p| p != &path);
        self.recent_afs_paths.insert(0, path);
        self.recent_afs_paths.truncate(persist::RECENT_AFS_MAX);
        self.mark_gui_state_dirty();
    }

    fn try_flush_gui_state_disk(&mut self) {
        if !self.gui_state_dirty {
            return;
        }
        match persist::save(&self.gui_state_snapshot()) {
            Ok(()) => self.gui_state_dirty = false,
            Err(e) => eprintln!("[gui] could not persist GUI state: {}", e),
        }
    }

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
                ui.menu_button("Recent AFS", |ui| {
                    if self.recent_afs_paths.is_empty() {
                        ui.label(egui::RichText::new("(empty)").weak());
                    } else {
                        for path in self.recent_afs_paths.clone() {
                            let missing = !path.exists();
                            let fname = path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("(invalid)")
                                .to_string();
                            let label = fname + if missing { " (missing)" } else { "" };
                            let txt = if missing {
                                egui::RichText::new(label).weak()
                            } else {
                                egui::RichText::new(label)
                            };
                            let r = ui.button(txt).on_hover_text(path.display().to_string());
                            if r.clicked() {
                                self.open_afs_path(path);
                                ui.close();
                            }
                        }
                        ui.separator();
                        if ui.button("Clear recent list").clicked() {
                            self.recent_afs_paths.clear();
                            self.mark_gui_state_dirty();
                        }
                    }
                });
                ui.separator();
                let has_afs = self.workspace.afs_path().is_some();
                let can_save_afs = has_afs && self.save_afs_job.is_none();
                if ui
                    .add_enabled(can_save_afs, egui::Button::new("Save AFS As..."))
                    .clicked()
                {
                    self.save_afs_as_dialog();
                    ui.close();
                }
                if self.save_afs_job.is_some() {
                    ui.label(egui::RichText::new("AFS save is running...").weak());
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
            ui.menu_button("Tools", |ui| {
                if ui.button("CWCheat Editor...").clicked() {
                    self.editors.cwcheat_editor = true;
                    ui.close();
                }
            });

            ui.add_space(6.0);
            if ui
                .button("Settings")
                .on_hover_text("CW cheat file path and Save AFS auto-update options")
                .clicked()
            {
                self.cwcheat_settings_modal_open = true;
            }
        });
    }

    fn open_afs_dialog(&mut self) {
        let mut dialog = rfd::FileDialog::new().add_filter("AFS/BIN", &["bin", "afs"]);
        if let Some(dir) = &self.last_dir_open_afs {
            dialog = dialog.set_directory(dir);
        }
        let Some(path) = dialog.pick_file() else {
            return;
        };
        self.open_afs_path(path);
    }

    fn open_afs_path(&mut self, path: std::path::PathBuf) {
        touch_dialog_dir_parent(
            &mut self.last_dir_open_afs,
            &path,
            &mut self.gui_state_dirty,
        );
        let path_for_recent = path.clone();
        match ModWorkspace::open_afs_file(path) {
            Ok(ws) => {
                let count = ws.afs_entries().len();
                self.record_recent_afs_open_success(path_for_recent);
                self.workspace = ws;
                self.tree_state = AssetTreeState::default();
                self.preview_state = PreviewState::default();
                self.editors = EditorWindows::default();
                self.reload_cwcheat_editor_text_from_path();
                self.gpu_mesh = None;
                self.gpu_texture_bind_group = None;
                self.gpu_mesh_stream_index = None;
                self.gpu_mesh_pzz_revision = None;
                self.status = format!("Loaded AFS ({} entries)", count);
            }
            Err(e) => {
                self.notify_error(format!("Failed to open AFS: {e}"));
            }
        }
    }

    fn save_afs_as_dialog(&mut self) {
        let Some(afs_path) = self.workspace.afs_path().cloned() else {
            self.notify_error("No AFS file is open.".to_owned());
            return;
        };
        let dirty_count = self.workspace.dirty_pzz_entry_count();

        let default_name = afs_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("output.bin")
            .to_string();
        let mut save_dialog = rfd::FileDialog::new()
            .add_filter("AFS/BIN", &["bin", "afs"])
            .set_file_name(&default_name);
        if let Some(dir) = &self.last_dir_write_modified_pzz_to_afs {
            save_dialog = save_dialog.set_directory(dir);
        }
        let Some(output_path) = save_dialog.save_file() else {
            return;
        };

        if dirty_count == 0 {
            match copy_afs_with_retry(&afs_path, &output_path) {
                Ok(bytes) => {
                    touch_dialog_dir_parent(
                        &mut self.last_dir_write_modified_pzz_to_afs,
                        &output_path,
                        &mut self.gui_state_dirty,
                    );
                    if paths_point_to_same_file(&afs_path, &output_path) {
                        self.status = "Save target is the same file as the open AFS.".to_owned();
                        return;
                    }
                    self.status = format!(
                        "Saved AFS ({:.1} MB) -> {}",
                        bytes as f64 / (1024.0 * 1024.0),
                        output_path.display()
                    );
                    self.maybe_auto_update_cwcheat_after_afs_save();
                }
                Err(e) => self.notify_error(format!("Failed to save AFS: {e}")),
            }
            return;
        }

        let dirty_entries = self
            .workspace
            .dirty_pzz_entries()
            .into_iter()
            .map(|(entry_index, pzz)| (entry_index, pzz.clone()))
            .collect::<Vec<_>>();
        let (sender, receiver) = std::sync::mpsc::channel();
        let afs_path_for_job = afs_path.clone();
        let output_path_for_job = output_path.clone();
        std::thread::spawn(move || {
            let result =
                build_patched_afs_with_dirty_pzz_entry_clones(dirty_entries, &afs_path_for_job)
                    .and_then(|patched| {
                        std::fs::write(&output_path_for_job, &patched)
                            .map_err(anyhow::Error::from)?;
                        Ok(patched.len())
                    })
                    .map_err(|e| e.to_string());
            let _ = sender.send(result);
        });
        self.status = format!(
            "Saving AFS with {} modified PZZ entries in background...",
            dirty_count
        );
        self.save_afs_job = Some(SaveAfsJob {
            receiver,
            output_path,
            dirty_count,
        });
    }

    fn open_pzz_dialog(&mut self) {
        let mut dialog = rfd::FileDialog::new().add_filter("PZZ", &["pzz"]);
        if let Some(dir) = &self.last_dir_open_pzz {
            dialog = dialog.set_directory(dir);
        }
        let Some(path) = dialog.pick_file() else {
            return;
        };
        touch_dialog_dir_parent(
            &mut self.last_dir_open_pzz,
            &path,
            &mut self.gui_state_dirty,
        );
        match ModWorkspace::open_pzz_file(path) {
            Ok(ws) => {
                self.workspace = ws;
                self.tree_state = AssetTreeState::default();
                self.preview_state = PreviewState::default();
                self.editors = EditorWindows::default();
                self.reload_cwcheat_editor_text_from_path();
                self.gpu_mesh = None;
                self.gpu_texture_bind_group = None;
                self.gpu_mesh_stream_index = None;
                self.gpu_mesh_pzz_revision = None;
                self.status = "Loaded PZZ".to_string();
            }
            Err(e) => {
                self.notify_error(format!("Failed to open PZZ: {e}"));
            }
        }
    }

    fn handle_tree_action(&mut self, action: TreeAction) {
        match action {
            TreeAction::SelectAfsEntry(index) => {
                eprintln!("[gui] SelectAfsEntry: index={}", index);
                self.tree_state.selected_afs_entry = Some(index);
                self.tree_state.selected_stream = None;
            }
            TreeAction::OpenPzz(index) => {
                eprintln!(
                    "[gui] OpenPzz: index={}, currently_expanded={:?}",
                    index,
                    self.workspace.expanded_pzz_entry()
                );
                if self.workspace.expanded_pzz_entry() == Some(index) {
                    self.workspace.close_open_pzz();
                    self.tree_state.selected_stream = None;
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
                        self.status =
                            format!("Opened PZZ entry {} ({} streams)", index, stream_count);
                        eprintln!(
                            "[gui]   => OK: {} streams, expanded_pzz_entry={:?}",
                            stream_count,
                            self.workspace.expanded_pzz_entry()
                        );
                    }
                    Err(e) => {
                        self.notify_error(format!("Failed to open PZZ entry {}: {e}", index));
                        eprintln!("[gui]   => FAILED: {}", e);
                    }
                }
            }
            TreeAction::SelectStream(index) => {
                eprintln!("[gui] SelectStream: index={}", index);
                self.tree_state.selected_stream = Some(index);
                self.preview_state.camera = None;
                self.gpu_mesh_stream_index = None;
                self.gpu_mesh_pzz_revision = None;
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
            TreeAction::ReplaceStreamPmf2(index) => {
                self.replace_stream_pmf2(index);
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
        }
    }

    fn export_entry_raw(&mut self, entry_index: usize) {
        let Some(afs_path) = self.workspace.afs_path().cloned() else {
            self.notify_error("No AFS file is open.".to_owned());
            return;
        };
        let Some(entry) = self
            .workspace
            .afs_entries()
            .iter()
            .find(|e| e.index == entry_index)
        else {
            self.notify_error(format!("Entry {} not found", entry_index));
            return;
        };
        let mut dialog = rfd::FileDialog::new().set_file_name(&entry.name);
        if let Some(dir) = &self.last_dir_export_entry_raw {
            dialog = dialog.set_directory(dir);
        }
        let Some(path) = dialog.save_file() else {
            return;
        };
        touch_dialog_dir_parent(
            &mut self.last_dir_export_entry_raw,
            &path,
            &mut self.gui_state_dirty,
        );
        match afs::read_entry_from_file(&afs_path, entry.offset, entry.size) {
            Ok(data) => match std::fs::write(&path, &data) {
                Ok(()) => {
                    self.status = format!("Exported: {}", path.display());
                }
                Err(e) => {
                    self.notify_error(format!("Write failed: {e}"));
                }
            },
            Err(e) => {
                self.notify_error(format!("Read failed: {e}"));
            }
        }
    }

    fn export_stream_raw(&mut self, stream_index: usize) {
        let Some(data) = self.get_stream_data(stream_index).map(|d| d.to_vec()) else {
            self.notify_error("Stream not available.".to_owned());
            return;
        };
        let name = self
            .workspace
            .open_pzz()
            .and_then(|p| p.streams().get(stream_index))
            .map(|s| s.name.clone())
            .unwrap_or_else(|| format!("stream{:03}.bin", stream_index));
        let mut dialog = rfd::FileDialog::new().set_file_name(&name);
        if let Some(dir) = &self.last_dir_export_stream_raw {
            dialog = dialog.set_directory(dir);
        }
        let Some(path) = dialog.save_file() else {
            return;
        };
        touch_dialog_dir_parent(
            &mut self.last_dir_export_stream_raw,
            &path,
            &mut self.gui_state_dirty,
        );
        match std::fs::write(&path, data) {
            Ok(()) => self.status = format!("Exported: {}", path.display()),
            Err(e) => self.notify_error(format!("Write failed: {e}")),
        }
    }

    fn export_stream_dae(&mut self, stream_index: usize) {
        let Some(data) = self.get_stream_data(stream_index).map(|d| d.to_vec()) else {
            self.notify_error("Stream not available.".to_owned());
            return;
        };
        if pzz::classify_stream(&data) != "pmf2" {
            self.notify_error("Not a PMF2 stream.".to_owned());
            return;
        }
        let model_name = format!("stream{:03}", stream_index);
        let mut dialog = rfd::FileDialog::new()
            .add_filter("DAE", &["dae"])
            .set_file_name(&format!("{}.dae", model_name));
        if let Some(dir) = &self.last_dir_export_stream_dae {
            dialog = dialog.set_directory(dir);
        }
        let Some(path) = dialog.save_file() else {
            return;
        };
        touch_dialog_dir_parent(
            &mut self.last_dir_export_stream_dae,
            &path,
            &mut self.gui_state_dirty,
        );
        let (bone_meshes, sections, bbox, _) = pmf2::extract_per_bone_meshes(&data, true);
        if bone_meshes.is_empty() {
            self.notify_error("No mesh data in PMF2.".to_owned());
            return;
        }
        match dae::write_dae(&path, &bone_meshes, &sections, &model_name) {
            Ok(()) => {
                let meta = pmf2::build_meta(&model_name, &sections, bbox, &bone_meshes);
                let meta_path = path.with_extension("pmf2meta.json");
                let _ = std::fs::write(&meta_path, serde_json::to_string_pretty(&meta).unwrap());
                self.status = format!("Exported DAE: {}", path.display());
            }
            Err(e) => self.notify_error(format!("DAE export failed: {e}")),
        }
    }

    fn replace_stream_dae(&mut self, stream_index: usize) {
        let Some(template_data) = self.get_stream_data(stream_index).map(|d| d.to_vec()) else {
            self.notify_error("Stream not available.".to_owned());
            return;
        };
        let mut dialog = rfd::FileDialog::new().add_filter("DAE", &["dae"]);
        if let Some(dir) = &self.last_dir_replace_stream_dae {
            dialog = dialog.set_directory(dir);
        }
        let Some(path) = dialog.pick_file() else {
            return;
        };
        touch_dialog_dir_parent(
            &mut self.last_dir_replace_stream_dae,
            &path,
            &mut self.gui_state_dirty,
        );
        self.pending_dae_replace_config = Some(PendingDaeReplaceConfig {
            stream_index,
            template_data,
            dae_path: path.clone(),
            flip_uv_v: false,
        });
        self.status = format!("Selected DAE: {} (configure UV flip in dialog)", path.display());
    }

    fn replace_stream_pmf2(&mut self, stream_index: usize) {
        let Some(_) = self.get_stream_data(stream_index) else {
            self.notify_error("Stream not available.".to_owned());
            return;
        };
        let mut dialog = rfd::FileDialog::new().add_filter("PMF2", &["pmf2"]);
        if let Some(dir) = &self.last_dir_replace_stream_pmf2 {
            dialog = dialog.set_directory(dir);
        }
        let Some(path) = dialog.pick_file() else {
            return;
        };
        touch_dialog_dir_parent(
            &mut self.last_dir_replace_stream_pmf2,
            &path,
            &mut self.gui_state_dirty,
        );
        let new_pmf2 = match std::fs::read(&path) {
            Ok(d) => d,
            Err(e) => {
                self.notify_error(format!("Failed to read PMF2 file: {e}"));
                return;
            }
        };
        if pzz::classify_stream(&new_pmf2) != "pmf2" {
            self.notify_error(
                "Selected file is not PMF2 (first 4 bytes must be \"PMF2\").".to_owned(),
            );
            return;
        }
        match self.workspace.replace_stream(stream_index, new_pmf2) {
            Ok(()) => {
                self.gpu_mesh_stream_index = None;
                self.gpu_mesh_pzz_revision = None;
                self.status = format!("Replaced PMF2 stream from {}", path.display());
            }
            Err(e) => self.notify_error(format!("Stream replace failed: {e}")),
        }
    }

    fn export_stream_png(&mut self, stream_index: usize) {
        let Some(data) = self.get_stream_data(stream_index).map(|d| d.to_vec()) else {
            self.notify_error("Stream not available.".to_owned());
            return;
        };
        let image = match crate::texture::GimImage::decode(&data) {
            Ok(img) => img,
            Err(e) => {
                self.notify_error(format!("GIM decode failed: {e}"));
                return;
            }
        };
        let mut dialog = rfd::FileDialog::new()
            .add_filter("PNG", &["png"])
            .set_file_name("texture.png");
        if let Some(dir) = &self.last_dir_export_stream_png {
            dialog = dialog.set_directory(dir);
        }
        let Some(path) = dialog.save_file() else {
            return;
        };
        touch_dialog_dir_parent(
            &mut self.last_dir_export_stream_png,
            &path,
            &mut self.gui_state_dirty,
        );
        let mut output =
            image::RgbaImage::new(image.metadata.width as u32, image.metadata.height as u32);
        for (i, pixel) in image.rgba.iter().enumerate() {
            let x = (i % image.metadata.width) as u32;
            let y = (i / image.metadata.width) as u32;
            output.put_pixel(x, y, image::Rgba(*pixel));
        }
        match output.save(&path) {
            Ok(()) => self.status = format!("Exported PNG: {}", path.display()),
            Err(e) => self.notify_error(format!("PNG export failed: {e}")),
        }
    }

    fn replace_stream_png(&mut self, stream_index: usize) {
        let decode_started = Instant::now();
        let Some(data) = self.get_stream_data(stream_index).map(|d| d.to_vec()) else {
            self.notify_error("Stream not available.".to_owned());
            return;
        };
        let image = match crate::texture::GimImage::decode(&data) {
            Ok(img) => img,
            Err(e) => {
                self.notify_error(format!("GIM decode failed: {e}"));
                return;
            }
        };
        eprintln!(
            "[gui] decoded current GIM stream {} before PNG replace in {:?}",
            stream_index,
            decode_started.elapsed()
        );
        let mut dialog = rfd::FileDialog::new().add_filter("PNG", &["png"]);
        if let Some(dir) = &self.last_dir_replace_stream_png {
            dialog = dialog.set_directory(dir);
        }
        let Some(path) = dialog.pick_file() else {
            return;
        };
        touch_dialog_dir_parent(
            &mut self.last_dir_replace_stream_png,
            &path,
            &mut self.gui_state_dirty,
        );
        let png_read_started = Instant::now();
        let png_data = match std::fs::read(&path) {
            Ok(d) => d,
            Err(e) => {
                self.notify_error(format!("Failed to read PNG: {e}"));
                return;
            }
        };
        eprintln!(
            "[gui] read replacement PNG {} ({} bytes) in {:?}",
            path.display(),
            png_data.len(),
            png_read_started.elapsed()
        );
        let preview_image = match decode_png_preview_color_image(&png_data) {
            Ok(img) => img,
            Err(e) => {
                self.notify_error(format!("Failed to decode PNG preview: {e}"));
                return;
            }
        };

        self.pending_gim_replace_config = Some(PendingGimReplaceConfig {
            stream_index,
            source_image: image,
            png_path: path.clone(),
            png_data,
            preview_image,
            preview_texture: None,
            selected_format: self.gim_replace_format,
        });
        self.status = format!("Selected PNG: {} (choose GIM format in dialog)", path.display());
    }

    fn show_dae_replace_config_modal(&mut self, ctx: &egui::Context) {
        let Some(pending) = self.pending_dae_replace_config.as_ref() else {
            return;
        };
        let mut apply = false;
        let mut cancel = false;
        let mut open = true;
        let dialog_path = pending.dae_path.clone();
        egui::Window::new("Replace PMF2 from DAE")
            .id(egui::Id::new("gvg_dae_replace_config_modal"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size(egui::vec2(560.0, 220.0))
            .min_size(egui::vec2(460.0, 180.0))
            .show(ctx, |ui| {
                ui.label("Configure UV import behavior before patching PMF2:");
                ui.label(egui::RichText::new(dialog_path.display().to_string()).weak().monospace());
                ui.add_space(8.0);
                if let Some(state) = self.pending_dae_replace_config.as_mut() {
                    ui.checkbox(&mut state.flip_uv_v, "Flip UV V (v = 1 - v)");
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                    if ui.button("Apply Replace").clicked() {
                        apply = true;
                    }
                });
            });
        if !open {
            cancel = true;
        }
        if cancel {
            self.pending_dae_replace_config = None;
            return;
        }
        if !apply {
            return;
        }

        let Some(state) = self.pending_dae_replace_config.take() else {
            return;
        };
        let meta = match dae::read_dae_to_meta_with_uv_flip(&state.dae_path, None, state.flip_uv_v) {
            Ok(m) => m,
            Err(e) => {
                self.notify_error(format!("DAE import failed: {e}"));
                return;
            }
        };
        let new_pmf2 = match pmf2::patch_pmf2_with_mesh_updates(&state.template_data, &meta, 0.0) {
            Some(d) => d,
            None => {
                self.notify_error("Failed to patch PMF2 from DAE.".to_owned());
                return;
            }
        };
        match self.workspace.replace_stream(state.stream_index, new_pmf2) {
            Ok(()) => {
                self.gpu_mesh_stream_index = None;
                self.gpu_mesh_pzz_revision = None;
                self.status = if state.flip_uv_v {
                    "Replaced PMF2 stream from DAE (UV V flipped)".to_string()
                } else {
                    "Replaced PMF2 stream from DAE (UV V unchanged)".to_string()
                };
            }
            Err(e) => self.notify_error(format!("Stream replace failed: {e}")),
        }
    }

    fn show_gim_replace_config_modal(&mut self, ctx: &egui::Context) {
        let Some(pending) = self.pending_gim_replace_config.as_ref() else {
            return;
        };
        let mut apply = false;
        let mut cancel = false;
        let mut open = true;
        let dialog_path = pending.png_path.clone();
        egui::Window::new("Replace GIM from PNG")
            .id(egui::Id::new("gvg_gim_replace_config_modal"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size(egui::vec2(900.0, 680.0))
            .min_size(egui::vec2(560.0, 420.0))
            .show(ctx, |ui| {
                if let Some(state) = self.pending_gim_replace_config.as_mut() {
                    if state.preview_texture.is_none() {
                        let texture_name =
                            format!("gim_replace_preview_{}_{}", state.stream_index, state.png_path.display());
                        state.preview_texture = Some(ctx.load_texture(
                            texture_name,
                            state.preview_image.clone(),
                            egui::TextureOptions::LINEAR,
                        ));
                    }
                }
                ui.label(egui::RichText::new(dialog_path.display().to_string()).weak().monospace());
                ui.horizontal(|ui| {
                    ui.label("Format:");
                    if let Some(state) = self.pending_gim_replace_config.as_mut() {
                        egui::ComboBox::from_id_salt("gim_replace_modal_format")
                            .selected_text(state.selected_format.label())
                            .show_ui(ui, |ui| {
                                for format in GimReplaceFormat::all() {
                                    ui.selectable_value(
                                        &mut state.selected_format,
                                        *format,
                                        format.label(),
                                    );
                                }
                            });
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                    if ui.button("Apply Replace").clicked() {
                        apply = true;
                    }
                });
                ui.separator();
                let preview_size = ui.available_size();
                if let Some(state) = self.pending_gim_replace_config.as_mut() {
                    if let Some(texture) = state.preview_texture.as_ref() {
                        ui.add_sized(
                            preview_size,
                            egui::Image::new(texture).fit_to_exact_size(preview_size),
                        );
                    } else {
                        ui.allocate_ui(preview_size, |ui| {
                            ui.centered_and_justified(|ui| {
                                ui.label("PNG preview unavailable.");
                            });
                        });
                    }
                }
            });
        if !open {
            cancel = true;
        }
        if cancel {
            self.pending_gim_replace_config = None;
            return;
        }
        if !apply {
            return;
        }

        let Some(state) = self.pending_gim_replace_config.take() else {
            return;
        };
        let replace_started = Instant::now();
        let replaced = match state
            .source_image
            .replace_png_bytes_with_format(&state.png_data, state.selected_format)
        {
            Ok(d) => d,
            Err(e) => {
                self.notify_error(format!("GIM replace failed: {e}"));
                return;
            }
        };
        eprintln!(
            "[gui] rebuilt replacement GIM stream {} in {:?}",
            state.stream_index,
            replace_started.elapsed()
        );
        let workspace_replace_started = Instant::now();
        match self.workspace.replace_stream(state.stream_index, replaced) {
            Ok(()) => {
                eprintln!(
                    "[gui] replaced workspace stream {} in {:?}",
                    state.stream_index,
                    workspace_replace_started.elapsed()
                );
                if self.gim_replace_format != state.selected_format {
                    self.gim_replace_format = state.selected_format;
                    self.mark_gui_state_dirty();
                }
                self.gpu_mesh_stream_index = None;
                self.gpu_mesh_pzz_revision = None;
                self.gpu_texture_bind_group = None;
                self.status = format!(
                    "Replaced GIM stream from PNG ({})",
                    state.selected_format.label()
                );
            }
            Err(e) => self.notify_error(format!("Stream replace failed: {e}")),
        }
    }

    fn get_stream_data(&self, index: usize) -> Option<&[u8]> {
        self.workspace
            .open_pzz()
            .and_then(|pzz| pzz.stream_data().get(index))
            .map(Vec::as_slice)
    }

    fn show_3d_preview(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Some(gpu_mesh) = self.gpu_mesh.as_ref() else {
            ui.centered_and_justified(|ui| {
                ui.label("Select a PMF2 stream to preview.");
            });
            return;
        };

        let rs = match &self.wgpu_state {
            Some(rs) => rs,
            None => {
                ui.centered_and_justified(|ui| {
                    ui.label("3D preview unavailable (no wgpu).");
                });
                return;
            }
        };

        preview::preview_controls(ui, gpu_mesh, &mut self.preview_state);
        let fps = preview_fps(ctx);
        preview::preview_perf_label(
            ui,
            fps,
            gpu_mesh,
            self.last_preview_render_stats.map(|s| s.total_ms),
        );

        let available = ui.available_size();
        let vw = (available.x as u32).max(1);
        let vh = (available.y as u32).max(1);

        let camera = *self
            .preview_state
            .camera
            .get_or_insert_with(|| {
                crate::render::PreviewCamera::frame_bounds_with_target(
                    gpu_mesh.bounds,
                    gpu_mesh.focus_target,
                )
            });

        let (render_stats, texture_id) = {
            let renderer = match &mut self.gpu_renderer {
                Some(r) => r,
                None => return,
            };
            renderer.ensure_viewport(&rs.device, &mut rs.renderer.write(), vw, vh);
            let render_stats = renderer.render(
                &rs.device,
                &rs.queue,
                &camera,
                gpu_mesh,
                self.gpu_texture_bind_group.as_ref(),
                self.preview_state.wireframe,
                self.preview_state.visibility.show_axes,
                self.preview_state.visibility.show_grid,
            );
            (render_stats, renderer.egui_texture_id)
        };
        self.last_preview_render_stats = render_stats;

        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(vw as f32, vh as f32),
            egui::Sense::click_and_drag(),
        );

        if let Some(texture_id) = texture_id {
            ui.painter().image(
                texture_id,
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }

        if response.dragged() {
            let delta = ui.input(|input| input.pointer.delta());
            let cam = self
                .preview_state
                .camera
                .get_or_insert_with(|| {
                    crate::render::PreviewCamera::frame_bounds_with_target(
                        gpu_mesh.bounds,
                        gpu_mesh.focus_target,
                    )
                });
            cam.orbit(delta.x * 0.01, -delta.y * 0.01);
            ctx.request_repaint();
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() > f32::EPSILON {
                let cam = self.preview_state.camera.get_or_insert_with(|| {
                    crate::render::PreviewCamera::frame_bounds_with_target(
                        gpu_mesh.bounds,
                        gpu_mesh.focus_target,
                    )
                });
                cam.zoom((-scroll * 0.001).clamp(-0.5, 0.5));
                ctx.request_repaint();
            }
        }
    }

    fn update_gpu_mesh(&mut self) {
        let selected = self.tree_state.selected_stream;
        let current_pzz_revision = self.workspace.open_pzz().map(PzzWorkspace::revision);
        let current_visibility_key = self.preview_state.visibility.mesh_visibility_key();
        if gpu_mesh_cache_is_current(
            self.gpu_mesh_stream_index,
            self.gpu_mesh_pzz_revision,
            self.gpu_mesh_visibility_key,
            selected,
            current_pzz_revision,
            current_visibility_key,
        ) {
            return;
        }

        let Some(stream_index) = selected else {
            self.gpu_mesh = None;
            self.gpu_texture_bind_group = None;
            self.gpu_mesh_stream_index = None;
            self.gpu_mesh_pzz_revision = None;
            self.gpu_mesh_visibility_key = current_visibility_key;
            return;
        };

        let data = match self.get_stream_data(stream_index) {
            Some(d) if pzz::classify_stream(d) == "pmf2" => d,
            _ => {
                self.gpu_mesh = None;
                self.gpu_texture_bind_group = None;
                self.gpu_mesh_stream_index = Some(stream_index);
                self.gpu_mesh_pzz_revision = current_pzz_revision;
                self.gpu_mesh_visibility_key = current_visibility_key;
                return;
            }
        };

        let (renderer, rs) = match (&self.gpu_renderer, &self.wgpu_state) {
            (Some(r), Some(rs)) => (r, rs),
            _ => return,
        };
        let device = &rs.device;
        let queue = &rs.queue;

        let (mut visible_meshes, _, _, _) = pmf2::extract_per_bone_meshes(data, false);
        let total_mesh_count = visible_meshes.len();
        visible_meshes.retain(|mesh| self.preview_state.visibility.is_bone_visible(mesh.bone_index));
        self.gpu_mesh = renderer.upload_mesh(device, &visible_meshes);
        eprintln!(
            "[gui] GPU mesh uploaded for stream {}: {} total bone meshes, {} visible, gpu_mesh={}",
            stream_index,
            total_mesh_count,
            visible_meshes.len(),
            self.gpu_mesh.is_some()
        );

        self.gpu_texture_bind_group = None;
        let gim_index = stream_index + 1;
        if let Some(gim_data) = self.get_stream_data(gim_index) {
            if pzz::classify_stream(gim_data) == "gim" {
                if let Ok(image) = crate::texture::GimImage::decode(gim_data) {
                    self.gpu_texture_bind_group = Some(renderer.upload_texture(
                        device,
                        queue,
                        &image.rgba,
                        image.metadata.width as u32,
                        image.metadata.height as u32,
                    ));
                    eprintln!(
                        "[gui] GIM texture uploaded from stream {}: {}x{}",
                        gim_index, image.metadata.width, image.metadata.height
                    );
                }
            }
        }

        self.gpu_mesh_stream_index = Some(stream_index);
        self.gpu_mesh_pzz_revision = current_pzz_revision;
        self.gpu_mesh_visibility_key = current_visibility_key;
    }
}

fn gpu_mesh_cache_is_current(
    cached_stream_index: Option<usize>,
    cached_pzz_revision: Option<u64>,
    cached_visibility_key: u64,
    selected_stream: Option<usize>,
    current_pzz_revision: Option<u64>,
    current_visibility_key: u64,
) -> bool {
    cached_stream_index == selected_stream
        && selected_stream.is_some()
        && cached_pzz_revision == current_pzz_revision
        && cached_visibility_key == current_visibility_key
}

fn preview_fps(ctx: &egui::Context) -> f32 {
    let dt = ctx.input(|i| i.stable_dt).max(1.0 / 240.0);
    1.0 / dt
}

fn build_patched_afs_with_dirty_pzz_entry_clones(
    dirty_entries: Vec<(usize, PzzWorkspace)>,
    afs_path: &Path,
) -> Result<Vec<u8>> {
    if dirty_entries.is_empty() {
        anyhow::bail!("no modified PZZ entries are staged");
    }

    let started = std::time::Instant::now();
    let mut rebuilt_entries = Vec::with_capacity(dirty_entries.len());
    for (entry_index, mut pzz) in dirty_entries {
        rebuilt_entries.push((entry_index, rebuild_pzz_payload_cached(&mut pzz)?));
    }
    rebuilt_entries.sort_by_key(|(entry_index, _)| *entry_index);
    eprintln!(
        "[gui] rebuilt {} dirty PZZ entries in {} ms",
        rebuilt_entries.len(),
        started.elapsed().as_millis()
    );

    let read_started = std::time::Instant::now();
    let original = std::fs::read(afs_path)?;
    eprintln!(
        "[gui] read AFS {} bytes in {} ms",
        original.len(),
        read_started.elapsed().as_millis()
    );
    let patch_started = std::time::Instant::now();
    let replacement_refs = rebuilt_entries
        .iter()
        .map(|(entry_index, rebuilt)| (*entry_index, rebuilt.as_slice()))
        .collect::<Vec<_>>();
    let patched = afs::patch_entries_bytes(&original, &replacement_refs)?;
    eprintln!(
        "[gui] patched {} AFS entries in {} ms",
        replacement_refs.len(),
        patch_started.elapsed().as_millis()
    );
    Ok(patched)
}

fn paths_point_to_same_file(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

fn copy_afs_with_retry(src: &Path, dst: &Path) -> std::io::Result<u64> {
    if paths_point_to_same_file(src, dst) {
        return Ok(std::fs::metadata(src)?.len());
    }
    match std::fs::copy(src, dst) {
        Ok(n) => Ok(n),
        Err(e) if retry_copy_via_temp_disk(&e) => copy_via_temp_rename(src, dst),
        Err(e) => Err(e),
    }
}

fn retry_copy_via_temp_disk(e: &std::io::Error) -> bool {
    #[cfg(windows)]
    {
        e.raw_os_error() == Some(32)
    }
    #[cfg(not(windows))]
    {
        matches!(e.kind(), std::io::ErrorKind::ResourceBusy)
            || matches!(e.kind(), std::io::ErrorKind::PermissionDenied)
    }
}

fn copy_via_temp_rename(src: &Path, dst: &Path) -> std::io::Result<u64> {
    let parent = dst.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "destination has no parent directory",
        )
    })?;
    let stem = dst.file_name().and_then(|s| s.to_str()).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "destination file name is invalid",
        )
    })?;
    let tmp = parent.join(format!("_gvgafs_partial_{}.{stem}.tmp", std::process::id()));

    let nbytes = std::fs::copy(src, &tmp).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e
    })?;

    #[cfg(windows)]
    if dst.exists() {
        std::fs::remove_file(dst).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            e
        })?;
    }

    std::fs::rename(&tmp, dst).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e
    })?;
    Ok(nbytes)
}

fn decode_png_preview_color_image(png_data: &[u8]) -> Result<egui::ColorImage> {
    let png = image::load_from_memory(png_data)
        .map_err(|e| anyhow::anyhow!("decode png failed: {e}"))?
        .to_rgba8();
    let size = [png.width() as usize, png.height() as usize];
    Ok(egui::ColorImage::from_rgba_unmultiplied(
        size,
        png.as_raw(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_mesh_cache_invalidates_when_selected_stream_revision_changes() {
        assert!(!gpu_mesh_cache_is_current(
            Some(0),
            Some(1),
            0x10,
            Some(0),
            Some(2),
            0x10,
        ));
    }

    #[test]
    fn decode_png_preview_preserves_image_dimensions() {
        let mut rgba = image::RgbaImage::new(2, 1);
        rgba.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
        rgba.put_pixel(1, 0, image::Rgba([0, 255, 0, 255]));
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgba8(rgba)
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();

        let color_image = decode_png_preview_color_image(&bytes).unwrap();
        assert_eq!(color_image.size, [2, 1]);
    }
}

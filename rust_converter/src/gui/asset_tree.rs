use crate::workspace::{AssetKind, EntryValidation, ModWorkspace, StreamNode};
use eframe::egui;

#[derive(Clone, Debug)]
pub enum TreeAction {
    SelectAfsEntry(usize),
    OpenPzz(usize),
    SelectStream(usize),
    ExportEntryRaw(usize),
    ExportStreamDae(usize),
    ReplaceStreamDae(usize),
    ExportStreamPng(usize),
    ReplaceStreamPng(usize),
    ExportStreamRaw(usize),
    OpenPmf2Metadata(usize),
    OpenPmf2Data(usize),
    OpenGimPreview(usize),
    OpenHexView(usize),
    OpenSavePlanner,
}

pub struct AssetTreeState {
    pub search_filter: String,
    pub selected_afs_entry: Option<usize>,
    pub selected_stream: Option<usize>,
}

impl Default for AssetTreeState {
    fn default() -> Self {
        Self {
            search_filter: String::new(),
            selected_afs_entry: None,
            selected_stream: None,
        }
    }
}

pub fn show_asset_tree(
    ui: &mut egui::Ui,
    workspace: &ModWorkspace,
    state: &mut AssetTreeState,
) -> Vec<TreeAction> {
    let mut actions = Vec::new();

    ui.horizontal(|ui| {
        ui.label("Search:");
        ui.text_edit_singleline(&mut state.search_filter);
    });
    ui.separator();

    let entries = workspace.afs_entries();
    let expanded_pzz = workspace.expanded_pzz_entry();
    let pzz_streams: Option<&[StreamNode]> = workspace.open_pzz().map(|p| p.streams());

    let filter_lower = state.search_filter.to_lowercase();
    let has_filter = !filter_lower.is_empty();

    let mut rows: Vec<TreeRow> = Vec::new();

    for entry in entries {
        if has_filter && !entry.name.to_lowercase().contains(&filter_lower) {
            continue;
        }
        rows.push(TreeRow::AfsEntry {
            index: entry.index,
            name: entry.name.clone(),
            size: entry.size,
            kind: entry.kind,
            validation: entry.validation,
            is_expanded: expanded_pzz == Some(entry.index),
        });
        if expanded_pzz == Some(entry.index) {
            if let Some(streams) = pzz_streams {
                for stream in streams {
                    if has_filter && !stream.name.to_lowercase().contains(&filter_lower) {
                        continue;
                    }
                    rows.push(TreeRow::Stream {
                        index: stream.index,
                        name: stream.name.clone(),
                        size: stream.size,
                        kind: stream.kind,
                        dirty: stream.dirty,
                    });
                }
            }
        }
    }

    if entries.is_empty() && workspace.open_pzz().is_some() {
        if let Some(streams) = pzz_streams {
            for stream in streams {
                if has_filter && !stream.name.to_lowercase().contains(&filter_lower) {
                    continue;
                }
                rows.push(TreeRow::Stream {
                    index: stream.index,
                    name: stream.name.clone(),
                    size: stream.size,
                    kind: stream.kind,
                    dirty: stream.dirty,
                });
            }
        }
    }

    let row_height = 20.0;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, row_height, rows.len(), |ui, row_range| {
            for row in &rows[row_range] {
                match row {
                    TreeRow::AfsEntry {
                        index,
                        name,
                        size,
                        kind,
                        validation,
                        is_expanded,
                    } => {
                        let selected = state.selected_afs_entry == Some(*index);
                        let expand_icon = if *kind == AssetKind::Pzz {
                            if *is_expanded { "▼ " } else { "▶ " }
                        } else {
                            "  "
                        };
                        let validation_icon = match validation {
                            EntryValidation::Ok => "",
                            EntryValidation::Empty => "○ ",
                            EntryValidation::ExceedsBounds => "⚠ ",
                            EntryValidation::Overlapping => "⚠ ",
                        };
                        let label = format!(
                            "{}{}#{:04} {} ({})",
                            validation_icon,
                            expand_icon,
                            index,
                            name,
                            format_size(*size)
                        );
                        let response = ui.selectable_label(selected, label);
                        if response.clicked() {
                            state.selected_afs_entry = Some(*index);
                            state.selected_stream = None;
                            actions.push(TreeAction::SelectAfsEntry(*index));
                            if *kind == AssetKind::Pzz {
                                actions.push(TreeAction::OpenPzz(*index));
                            }
                        }
                        response.context_menu(|ui| {
                            afs_entry_context_menu(ui, *index, *kind, &mut actions);
                        });
                    }
                    TreeRow::Stream {
                        index,
                        name,
                        size,
                        kind,
                        dirty,
                    } => {
                        let selected = state.selected_stream == Some(*index);
                        let type_tag = match kind {
                            AssetKind::Pmf2 => "[PMF2]",
                            AssetKind::Gim => "[GIM] ",
                            AssetKind::Sad => "[SAD] ",
                            _ => "[BIN] ",
                        };
                        let dirty_mark = if *dirty { " *" } else { "" };
                        let label = format!(
                            "    {} {} ({}){dirty_mark}",
                            type_tag,
                            name,
                            format_size(*size)
                        );
                        let response = ui.selectable_label(selected, label);
                        if response.clicked() {
                            state.selected_stream = Some(*index);
                            actions.push(TreeAction::SelectStream(*index));
                        }
                        response.context_menu(|ui| {
                            stream_context_menu(ui, *index, *kind, &mut actions);
                        });
                    }
                }
            }
        });

    actions
}

fn afs_entry_context_menu(
    ui: &mut egui::Ui,
    index: usize,
    kind: AssetKind,
    actions: &mut Vec<TreeAction>,
) {
    if kind == AssetKind::Pzz {
        if ui.button("Open PZZ").clicked() {
            actions.push(TreeAction::OpenPzz(index));
            ui.close();
        }
    }
    if ui.button("Export Raw").clicked() {
        actions.push(TreeAction::ExportEntryRaw(index));
        ui.close();
    }
}

fn stream_context_menu(
    ui: &mut egui::Ui,
    index: usize,
    kind: AssetKind,
    actions: &mut Vec<TreeAction>,
) {
    match kind {
        AssetKind::Pmf2 => {
            if ui.button("Preview 3D").clicked() {
                actions.push(TreeAction::SelectStream(index));
                ui.close();
            }
            if ui.button("Export DAE").clicked() {
                actions.push(TreeAction::ExportStreamDae(index));
                ui.close();
            }
            if ui.button("Replace from DAE").clicked() {
                actions.push(TreeAction::ReplaceStreamDae(index));
                ui.close();
            }
            ui.separator();
            if ui.button("View Metadata").clicked() {
                actions.push(TreeAction::OpenPmf2Metadata(index));
                ui.close();
            }
            if ui.button("View Data").clicked() {
                actions.push(TreeAction::OpenPmf2Data(index));
                ui.close();
            }
        }
        AssetKind::Gim => {
            if ui.button("Preview Texture").clicked() {
                actions.push(TreeAction::OpenGimPreview(index));
                ui.close();
            }
            if ui.button("Export PNG").clicked() {
                actions.push(TreeAction::ExportStreamPng(index));
                ui.close();
            }
            if ui.button("Replace from PNG").clicked() {
                actions.push(TreeAction::ReplaceStreamPng(index));
                ui.close();
            }
        }
        _ => {}
    }
    ui.separator();
    if ui.button("View Hex").clicked() {
        actions.push(TreeAction::OpenHexView(index));
        ui.close();
    }
    if ui.button("Export Raw").clicked() {
        actions.push(TreeAction::ExportStreamRaw(index));
        ui.close();
    }
}

fn format_size(bytes: usize) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

enum TreeRow {
    AfsEntry {
        index: usize,
        name: String,
        size: usize,
        kind: AssetKind,
        validation: EntryValidation,
        is_expanded: bool,
    },
    Stream {
        index: usize,
        name: String,
        size: usize,
        kind: AssetKind,
        dirty: bool,
    },
}

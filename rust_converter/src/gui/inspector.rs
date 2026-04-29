use crate::{
    pmf2,
    pzz,
    texture::GimImage,
    workspace::{AssetKind, EntryValidation, ModWorkspace},
};
use eframe::egui;

pub fn show_inspector(
    ui: &mut egui::Ui,
    workspace: &ModWorkspace,
    selected_afs_entry: Option<usize>,
    selected_stream: Option<usize>,
) {
    ui.heading("Inspector");
    ui.separator();

    if let Some(stream_index) = selected_stream {
        if let Some(pzz) = workspace.open_pzz() {
            if let Some(stream_node) = pzz.streams().get(stream_index) {
                show_stream_inspector(ui, stream_node, pzz.stream_data().get(stream_index));
                return;
            }
        }
    }

    if let Some(entry_index) = selected_afs_entry {
        if let Some(entry) = workspace.afs_entries().iter().find(|e| e.index == entry_index) {
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
        AssetKind::Gim => show_gim_summary(ui, data),
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

    let (meshes, _, _, _) = pmf2::extract_per_bone_meshes(data, false);
    let total_verts: usize = meshes.iter().map(|m| m.vertices.len()).sum();
    let total_faces: usize = meshes.iter().map(|m| m.faces.len()).sum();
    labeled_row(ui, "Total vertices", &format!("{}", total_verts));
    labeled_row(ui, "Total faces", &format!("{}", total_faces));
}

fn show_gim_summary(ui: &mut egui::Ui, data: &[u8]) {
    ui.strong("GIM Summary");
    match GimImage::decode(data) {
        Ok(image) => {
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
            ui.separator();
            let flat: Vec<u8> = image.rgba.iter().flat_map(|p| *p).collect();
            let color_image = egui::ColorImage::from_rgba_unmultiplied(
                [image.metadata.width, image.metadata.height],
                &flat,
            );
            let texture = ui.ctx().load_texture(
                "inspector_gim_thumb",
                color_image,
                egui::TextureOptions::NEAREST,
            );
            let max_side = 200.0;
            let scale = (max_side / image.metadata.width as f32)
                .min(max_side / image.metadata.height as f32)
                .min(1.0);
            let display_size = egui::vec2(
                image.metadata.width as f32 * scale,
                image.metadata.height as f32 * scale,
            );
            ui.image((texture.id(), display_size));
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
            .map(|c| if c.is_ascii_graphic() || c == ' ' { c } else { '.' })
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

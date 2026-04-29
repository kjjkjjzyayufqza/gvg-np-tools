use crate::{
    pmf2,
    render::{Pmf2PreviewMesh, PreviewCamera, PreviewLineColor, PreviewState, PreviewViewport},
    workspace::ModWorkspace,
};
use eframe::egui;

pub fn show_preview(
    ui: &mut egui::Ui,
    workspace: &ModWorkspace,
    selected_stream: Option<usize>,
    preview_state: &mut PreviewState,
) {
    let data = selected_stream.and_then(|i| {
        workspace
            .open_pzz()
            .and_then(|pzz| pzz.stream_data().get(i))
            .map(Vec::as_slice)
    });

    let data = match data {
        Some(d) if crate::pzz::classify_stream(d) == "pmf2" => Some(d),
        _ => None,
    };

    let Some(data) = data else {
        ui.centered_and_justified(|ui| {
            ui.label("Select a PMF2 stream to preview.");
        });
        return;
    };

    let (meshes, _, _, _) = pmf2::extract_per_bone_meshes(data, false);
    let preview = match Pmf2PreviewMesh::from_bone_meshes(&meshes) {
        Ok(p) => p,
        Err(e) => {
            ui.centered_and_justified(|ui| {
                ui.label(format!("PMF2 preview unavailable: {e}"));
            });
            return;
        }
    };

    preview_controls(ui, &preview, preview_state);

    let (response, painter) = ui.allocate_painter(ui.available_size(), egui::Sense::drag());
    let rect = response.rect;

    if response.dragged() {
        let delta = ui.input(|input| input.pointer.delta());
        let camera = preview_state
            .camera
            .get_or_insert_with(|| PreviewCamera::frame_bounds(preview.bounds));
        camera.orbit(delta.x * 0.01, delta.y * 0.01);
        ui.ctx().request_repaint();
    }
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > f32::EPSILON {
            let camera = preview_state
                .camera
                .get_or_insert_with(|| PreviewCamera::frame_bounds(preview.bounds));
            camera.zoom((-scroll * 0.001).clamp(-0.5, 0.5));
            ui.ctx().request_repaint();
        }
    }

    let camera = *preview_state
        .camera
        .get_or_insert_with(|| PreviewCamera::frame_bounds(preview.bounds));
    let viewport = PreviewViewport {
        width: rect.width().max(1.0),
        height: rect.height().max(1.0),
    };

    match preview.project(&camera, viewport, &preview_state.visibility) {
        Ok(projected) => {
            for triangle in projected.triangles {
                let points =
                    triangle
                        .points
                        .map(|p| egui::pos2(rect.left() + p[0], rect.top() + p[1]));
                if !preview_state.wireframe {
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
        Err(e) => {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                format!("Projection failed: {e}"),
                egui::FontId::monospace(14.0),
                egui::Color32::LIGHT_RED,
            );
        }
    }
}

fn preview_controls(
    ui: &mut egui::Ui,
    preview: &Pmf2PreviewMesh,
    state: &mut PreviewState,
) {
    ui.horizontal_wrapped(|ui| {
        ui.label(format!(
            "{} vertices, {} triangles",
            preview.vertices.len(),
            preview.indices.len() / 3
        ));
        if ui.button("Frame").clicked() {
            state.camera = Some(PreviewCamera::frame_bounds(preview.bounds));
        }
        ui.checkbox(&mut state.wireframe, "Wireframe");
        ui.checkbox(&mut state.visibility.show_axes, "Axes");
        ui.checkbox(&mut state.visibility.show_bounds, "Bounds");
    });
    ui.collapsing("Bone Visibility", |ui| {
        for bone_index in preview.bones() {
            let mut visible = state.visibility.is_bone_visible(bone_index);
            if ui
                .checkbox(&mut visible, format!("Bone {}", bone_index))
                .changed()
            {
                state.visibility.set_bone_visible(bone_index, visible);
            }
        }
    });
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

use crate::{
    gpu_renderer::GpuMesh,
    render::{PreviewCamera, PreviewState},
};
use eframe::egui;

pub fn preview_controls(ui: &mut egui::Ui, mesh: &GpuMesh, state: &mut PreviewState) {
    ui.horizontal_wrapped(|ui| {
        if ui.button("Frame").clicked() {
            state.camera = Some(PreviewCamera::frame_bounds_with_target(
                mesh.bounds,
                mesh.focus_target,
            ));
        }
        ui.checkbox(&mut state.wireframe, "Wireframe");
        ui.checkbox(&mut state.visibility.show_grid, "Grid");
        ui.checkbox(&mut state.visibility.show_axes, "Axes");
        ui.checkbox(&mut state.visibility.show_bounds, "Bounds");
    });
}

pub fn preview_perf_label(ui: &mut egui::Ui, fps: f32, mesh: &GpuMesh, render_ms: Option<f64>) {
    ui.label(format_preview_perf_line(
        fps,
        mesh.vertex_count(),
        mesh.face_count(),
        mesh.triangle_index_count(),
        mesh.wireframe_index_count(),
        render_ms,
    ));
}

pub fn format_preview_perf_line(
    fps: f32,
    vertices: u32,
    faces: u32,
    triangle_indices: u32,
    wireframe_indices: u32,
    render_ms: Option<f64>,
) -> String {
    let render = render_ms
        .map(|ms| format!(" | render {ms:.2}ms"))
        .unwrap_or_default();
    format!(
        "FPS {fps:.1} | verts {vertices} | faces {faces} | tri idx {triangle_indices} | wire idx {wireframe_indices}{render}"
    )
}

#[cfg(test)]
mod tests {
    use super::format_preview_perf_line;

    #[test]
    fn preview_perf_line_includes_fps_and_mesh_counts() {
        let line = format_preview_perf_line(59.6, 231_435, 77_145, 231_435, 462_870, Some(7.25));

        assert!(line.contains("FPS 59.6"));
        assert!(line.contains("verts 231435"));
        assert!(line.contains("faces 77145"));
        assert!(line.contains("wire idx 462870"));
        assert!(line.contains("render 7.25ms"));
    }
}

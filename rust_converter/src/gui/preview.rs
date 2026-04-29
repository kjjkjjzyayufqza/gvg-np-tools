use crate::{
    gpu_renderer::GpuMesh,
    render::{PreviewCamera, PreviewState},
};
use eframe::egui;

pub fn preview_controls(ui: &mut egui::Ui, mesh: &GpuMesh, state: &mut PreviewState) {
    ui.horizontal_wrapped(|ui| {
        if ui.button("Frame").clicked() {
            state.camera = Some(PreviewCamera::frame_bounds(mesh.bounds));
        }
        ui.checkbox(&mut state.wireframe, "Wireframe");
        ui.checkbox(&mut state.visibility.show_grid, "Grid");
        ui.checkbox(&mut state.visibility.show_axes, "Axes");
        ui.checkbox(&mut state.visibility.show_bounds, "Bounds");
    });
}

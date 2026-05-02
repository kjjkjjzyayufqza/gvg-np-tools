use eframe::egui;

pub fn show_status_bar(ui: &mut egui::Ui, status: &str) {
    ui.horizontal(|ui| {
        ui.label("Status:");
        ui.monospace(status);
    });
}

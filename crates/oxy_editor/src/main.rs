#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod app;
mod graph_ui;
mod labels;
#[cfg(test)]
mod native_qa;
mod studio;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_title(concat!("OXY Engine ", env!("CARGO_PKG_VERSION")))
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([920.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "OXY Engine",
        options,
        Box::new(|cc| Ok(Box::new(app::Editor::new(cc)))),
    )
}

//! ekaza-cam — app desktop para descobrir câmeras ONVIF/Tuya na rede,
//! resolver o stream RTSP e assistir com player embutido.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod config;
mod crypto;
mod net;
mod onvif;
mod rtsp;
mod video;

fn load_icon() -> Option<egui::IconData> {
    let icon_bytes = include_bytes!("../assets/icon.png");
    let img = image::load_from_memory(icon_bytes).ok()?.into_rgba8();
    let (width, height) = img.dimensions();
    let rgba = img.into_raw();
    Some(egui::IconData {
        rgba,
        width,
        height,
    })
}

fn main() -> eframe::Result<()> {
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_inner_size([1200.0, 800.0])
        .with_min_inner_size([760.0, 480.0])
        .with_title("ekaza-cam");

    if let Some(icon) = load_icon() {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "ekaza-cam",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}

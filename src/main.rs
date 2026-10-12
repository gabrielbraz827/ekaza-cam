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

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_min_inner_size([760.0, 480.0])
            .with_title("ekaza-cam"),
        ..Default::default()
    };
    eframe::run_native(
        "ekaza-cam",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}

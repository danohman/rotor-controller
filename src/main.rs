#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod config;
mod grid;
mod icon;
mod drivers;
mod link;
mod qrz;
mod server;
mod theme;
mod update;
mod winfit;

fn main() -> eframe::Result {
    // Default size for first run; later runs restore the saved window geometry.
    let (size, min) = app::layout_sizes(config::Config::load().compact);
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title(concat!("Rotor Controller ", env!("CARGO_PKG_VERSION")))
            .with_app_id("rotor-controller")
            .with_icon(icon::window_icon())
            .with_inner_size(size)
            .with_min_inner_size(min),
        persist_window: true,
        ..Default::default()
    };
    eframe::run_native("Rotor Controller", options, Box::new(|cc| Ok(Box::new(app::App::new(cc)))))
}

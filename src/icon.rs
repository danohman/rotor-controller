//! Application icon, derived from `assets/icon.png` at build time (see build.rs).

use eframe::egui;

const PNG: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/icon256.png"));

pub fn window_icon() -> egui::IconData {
    let img = image::load_from_memory(PNG).expect("embedded icon").to_rgba8();
    egui::IconData { width: img.width(), height: img.height(), rgba: img.into_raw() }
}

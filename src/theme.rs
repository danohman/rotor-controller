//! Fonts (same approach as the sibling amplifier controller).

use eframe::egui;
use std::sync::atomic::{AtomicBool, Ordering};

static BOLD: AtomicBool = AtomicBool::new(false);

/// Bold UI font (Segoe UI Bold where available, else the regular font).
pub fn bold(size: f32) -> egui::FontId {
    if BOLD.load(Ordering::Relaxed) {
        egui::FontId::new(size, egui::FontFamily::Name("bold".into()))
    } else {
        egui::FontId::proportional(size)
    }
}

/// Read the first font file found under any system font directory.
fn load_system_font(files: &[&str]) -> Option<Vec<u8>> {
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    #[cfg(windows)]
    {
        if let Some(w) = std::env::var_os("WINDIR") {
            dirs.push(std::path::PathBuf::from(w).join("Fonts"));
        }
        if let Some(l) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(std::path::PathBuf::from(l).join("Microsoft").join("Windows").join("Fonts"));
        }
    }
    files.iter().find_map(|f| dirs.iter().find_map(|d| std::fs::read(d.join(f)).ok()))
}

/// Prefer Segoe UI (UI text, bold headings) and Cascadia Mono / Consolas (monospace) when the
/// system has them (Windows). They cannot be embedded, so on other platforms, or if a file is
/// missing, egui's bundled fonts are used. The bundled fonts stay as fallbacks for symbols.
pub fn install_fonts(ctx: &egui::Context) {
    use egui::{FontData, FontFamily, FontId, TextStyle};
    let mut fonts = egui::FontDefinitions::default();
    let mut add = |name: &str, files: &[&str], family: FontFamily| -> bool {
        let Some(bytes) = load_system_font(files) else { return false };
        fonts.font_data.insert(name.to_owned(), std::sync::Arc::new(FontData::from_owned(bytes)));
        fonts.families.entry(family).or_default().insert(0, name.to_owned());
        true
    };
    add("ui-regular", &["segoeui.ttf"], FontFamily::Proportional);
    add("mono-regular", &["CascadiaMono.ttf", "consola.ttf"], FontFamily::Monospace);
    let bold = add("ui-bold", &["segoeuib.ttf"], FontFamily::Name("bold".into()));
    if bold {
        // Symbols and non-Latin glyphs still come from the bundled fonts.
        let fallback = fonts.families[&FontFamily::Proportional].clone();
        let list = fonts.families.get_mut(&FontFamily::Name("bold".into())).unwrap();
        list.extend(fallback.into_iter().filter(|n| n != "ui-regular"));
    }
    BOLD.store(bold, Ordering::Relaxed);
    ctx.set_fonts(fonts);
    if bold {
        for theme in [egui::Theme::Dark, egui::Theme::Light] {
            ctx.style_mut_of(theme, |s| {
                let size = s.text_styles.get(&TextStyle::Heading).map_or(20.0, |f| f.size);
                s.text_styles.insert(TextStyle::Heading, FontId::new(size, FontFamily::Name("bold".into())));
            });
        }
    }
}

/// Colors used throughout the interface.
#[derive(Clone, Copy)]
pub struct Pal {
    pub dark: bool,
    pub bg: egui::Color32,
    pub card: egui::Color32,
    pub card_hi: egui::Color32,
    pub well: egui::Color32,
    pub stroke: egui::Color32,
    pub text: egui::Color32,
    pub dim: egui::Color32,
    pub accent: egui::Color32,
    pub amber: egui::Color32,
    pub green: egui::Color32,
    pub red: egui::Color32,
}

pub fn pal(v: &egui::Visuals) -> Pal {
    use egui::Color32 as C;
    if v.dark_mode {
        Pal {
            dark: true,
            bg: C::from_rgb(13, 15, 21),
            card: C::from_rgb(22, 26, 35),
            card_hi: C::from_rgb(31, 36, 48),
            well: C::from_rgb(10, 12, 17),
            stroke: C::from_rgb(43, 49, 64),
            text: C::from_rgb(232, 236, 244),
            dim: C::from_rgb(133, 143, 163),
            accent: C::from_rgb(78, 140, 255),
            amber: C::from_rgb(245, 176, 65),
            green: C::from_rgb(62, 207, 142),
            red: C::from_rgb(235, 87, 87),
        }
    } else {
        Pal {
            dark: false,
            bg: C::from_rgb(236, 239, 245),
            card: C::from_rgb(255, 255, 255),
            card_hi: C::from_rgb(243, 246, 251),
            well: C::from_rgb(226, 231, 240),
            stroke: C::from_rgb(210, 217, 228),
            text: C::from_rgb(22, 27, 38),
            dim: C::from_rgb(98, 108, 128),
            accent: C::from_rgb(37, 99, 235),
            amber: C::from_rgb(206, 120, 10),
            green: C::from_rgb(18, 150, 84),
            red: C::from_rgb(214, 48, 48),
        }
    }
}

/// Light or dark theme: palette, rounded flat widgets, roomier spacing, larger type.
pub fn apply(ctx: &egui::Context, dark: bool) {
    use egui::{Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle};
    let theme = if dark { egui::Theme::Dark } else { egui::Theme::Light };
    ctx.set_theme(theme);
    let mut v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    let p = pal(&v);
    v.panel_fill = p.bg;
    v.window_fill = p.card;
    v.window_stroke = Stroke::new(1.0, p.stroke);
    v.window_corner_radius = CornerRadius::same(12);
    v.menu_corner_radius = CornerRadius::same(10);
    v.extreme_bg_color = p.well;
    v.faint_bg_color = p.card;
    v.override_text_color = None;
    v.hyperlink_color = p.accent;
    let r = CornerRadius::same(9);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.card;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.stroke);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.inactive.bg_fill = if dark { Color32::from_rgb(52, 60, 78) } else { Color32::from_rgb(196, 204, 218) };
    w.inactive.weak_bg_fill = p.card_hi;
    w.inactive.bg_stroke = Stroke::new(1.0, p.stroke);
    w.inactive.fg_stroke = Stroke::new(1.0, p.text);
    let hov = if dark { Color32::from_rgb(42, 49, 66) } else { Color32::from_rgb(231, 237, 247) };
    w.hovered.bg_fill = hov;
    w.hovered.weak_bg_fill = hov;
    w.hovered.bg_stroke = Stroke::new(1.0, p.accent.gamma_multiply(0.7));
    w.hovered.fg_stroke = Stroke::new(1.5, p.text);
    w.active.bg_fill = p.accent.gamma_multiply(0.85);
    w.active.weak_bg_fill = p.accent.gamma_multiply(0.85);
    w.active.bg_stroke = Stroke::new(1.0, p.accent);
    w.active.fg_stroke = Stroke::new(1.5, Color32::WHITE);
    w.open.bg_fill = p.card_hi;
    w.open.weak_bg_fill = p.card_hi;
    for x in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        x.corner_radius = r;
    }
    v.selection.bg_fill = p.accent.gamma_multiply(if dark { 0.55 } else { 0.35 });
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.popup_shadow.color = Color32::from_black_alpha(if dark { 120 } else { 40 });
    v.window_shadow.color = Color32::from_black_alpha(if dark { 140 } else { 50 });
    ctx.set_visuals_of(theme, v);
    ctx.style_mut_of(theme, |s| {
        s.spacing.item_spacing = egui::vec2(8.0, 8.0);
        s.spacing.button_padding = egui::vec2(12.0, 6.0);
        s.spacing.interact_size = egui::vec2(36.0, 30.0);
        s.spacing.combo_width = 120.0;
        s.spacing.text_edit_width = 160.0;
        s.spacing.menu_margin = egui::Margin::same(12);
        let prop = |size: f32| FontId::new(size, FontFamily::Proportional);
        s.text_styles.insert(TextStyle::Body, prop(14.5));
        s.text_styles.insert(TextStyle::Button, prop(14.5));
        s.text_styles.insert(TextStyle::Small, prop(12.0));
        s.text_styles.insert(TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace));
        s.text_styles.insert(TextStyle::Heading, bold(24.0));
    });
}

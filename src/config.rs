//! Persistent settings (JSON in the per-user config directory).

use crate::drivers::Proto;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Number of preset buttons.
pub const PRESETS: usize = 5;

#[derive(Clone, Serialize, Deserialize)]
pub struct Preset {
    pub label: String,
    pub grid: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub protocol: Proto,
    pub port: String,
    pub baud: u32,
    /// rotctld host and port (network protocol).
    pub host: String,
    pub net_port: u16,
    /// Rotor has an elevation axis (and the protocol can carry it).
    pub elevation: bool,
    pub el_max: u16,
    /// Share the rotor with other programs through a rotctld-compatible TCP server.
    pub serve: bool,
    /// Connect at start-up and reconnect whenever the link drops.
    pub auto_connect: bool,
    pub serve_port: u16,
    pub my_grid: String,
    /// Maximum rotor travel in degrees (450 for the G-800DXA, 360 for most others).
    pub max_travel: u16,
    /// Degrees added to every commanded bearing (mounting correction).
    pub offset: i16,
    /// Multiplier applied to the interface azimuth (corrects an interface whose full scale is wrong).
    pub span_scale: f64,
    pub long_path: bool,
    pub compact: bool,
    /// Last window position (top-left) in each mode, restored when switching between them.
    pub pos_full: Option<[f32; 2]>,
    pub pos_compact: Option<[f32; 2]>,
    pub always_on_top: bool,
    pub dark_mode: bool,
    /// Show distances in miles instead of kilometers.
    pub miles: bool,
    /// Look for a newer release on GitHub when the program starts.
    pub check_updates: bool,
    /// QRZ.com login used to look up callsigns (stored in the config file as plain text).
    pub qrz_user: String,
    pub qrz_pass: String,
    pub presets: Vec<Preset>,
}

impl Default for Config {
    fn default() -> Self {
        let p = |l: &str, g: &str| Preset { label: l.into(), grid: g.into() };
        Self {
            protocol: Proto::Gs232,
            port: String::new(),
            baud: 9600,
            host: "127.0.0.1".into(),
            net_port: 4533,
            elevation: false,
            el_max: 180,
            serve: false,
            auto_connect: true,
            serve_port: 4533,
            my_grid: String::new(),
            max_travel: 450,
            offset: 0,
            span_scale: 1.0,
            long_path: false,
            compact: false,
            pos_full: None,
            pos_compact: None,
            always_on_top: false,
            dark_mode: true,
            miles: false,
            check_updates: true,
            qrz_user: String::new(),
            qrz_pass: String::new(),
            presets: vec![p("EU", "JO50"), p("Japan", "PM95"), p("W1", "FN42"), p("W6", "CM97"), p("VK", "QF22")],
        }
    }
}

impl Config {
    fn path() -> Option<PathBuf> {
        let dirs = directories::ProjectDirs::from("", "", "rotor-controller")?;
        Some(dirs.config_dir().join("config.json"))
    }

    pub fn load() -> Self {
        let mut c: Self = Self::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        c.presets.resize_with(PRESETS, || Preset { label: String::new(), grid: String::new() });
        c
    }

    pub fn save(&self) {
        let Some(p) = Self::path() else { return };
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(p, s);
        }
    }

    /// Elevation is active only when enabled and the protocol supports it.
    pub fn has_elevation(&self) -> bool {
        self.elevation && self.protocol.elevation_capable()
    }
}

//! egui user interface: full and compact modes.

use crate::config::Config;
use crate::drivers::{self, Cal, DriverCfg, Jog, Pos, Proto};
use crate::link::{self, Cmd, Event, Transport};
use crate::server::{self, Remote, Server, Shared};
use crate::grid;
use crossbeam_channel::{Receiver, Sender};
use crate::theme;
use crate::update::UpdateStatus;
use eframe::egui::{self, Align2, Color32, FontId, Pos2, RichText, Sense, Stroke, vec2};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const FULL_SIZE: [f32; 2] = [560.0, 632.0];
pub const FULL_MIN: [f32; 2] = [500.0, 600.0];
/// The compact view is a fixed 260 x 218 window.
pub const COMPACT_SIZE: [f32; 2] = [260.0, 194.0];
pub const COMPACT_MIN: [f32; 2] = [260.0, 194.0];

const BAUDS: [u32; 9] = [600, 1200, 2400, 4800, 9600, 19200, 38400, 57600, 115200];

pub fn layout_sizes(compact: bool) -> ([f32; 2], [f32; 2]) {
    if compact { (COMPACT_SIZE, COMPACT_MIN) } else { (FULL_SIZE, FULL_MIN) }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    Connection,
    Rotor,
    Location,
    Presets,
    Sharing,
    Calibrate,
    Appearance,
    Log,
    About,
}

impl SettingsTab {
    const ALL: [SettingsTab; 9] = [
        SettingsTab::Connection,
        SettingsTab::Rotor,
        SettingsTab::Location,
        SettingsTab::Presets,
        SettingsTab::Sharing,
        SettingsTab::Calibrate,
        SettingsTab::Appearance,
        SettingsTab::Log,
        SettingsTab::About,
    ];

    fn label(self) -> &'static str {
        match self {
            SettingsTab::Connection => "Connection",
            SettingsTab::Rotor => "Rotor",
            SettingsTab::Location => "Location",
            SettingsTab::Presets => "Presets",
            SettingsTab::Sharing => "Sharing",
            SettingsTab::Calibrate => "Calibrate",
            SettingsTab::Appearance => "Appearance",
            SettingsTab::Log => "Log",
            SettingsTab::About => "About",
        }
    }

    fn blurb(self) -> &'static str {
        match self {
            SettingsTab::Connection => "How the program talks to the rotor.",
            SettingsTab::Rotor => "Travel, heading correction and elevation.",
            SettingsTab::Location => "Where you are, so presets can find their bearing.",
            SettingsTab::Presets => "Label and grid square for each preset button.",
            SettingsTab::Sharing => "Let other programs use this rotor.",
            SettingsTab::Calibrate => "Line the GS-232B interface up with the control box.",
            SettingsTab::Appearance => "Theme and window behavior.",
            SettingsTab::Log => "What the program sent and received.",
            SettingsTab::About => "Version, updates and license.",
        }
    }
}

/// Result of a background QRZ.com request.
enum QrzMsg {
    Lookup(Result<crate::qrz::Station, String>),
    Test(Result<(), String>),
}

pub struct App {
    cfg: Config,
    tx: Sender<Cmd>,
    rx: Receiver<Event>,
    connected: Option<String>,
    /// The user wants a real (serial/network) link; reconnect when it is down.
    want_link: bool,
    sent_title: String,
    sent_on_top: Option<bool>,
    last_move: Instant,
    move_dir: Option<Jog>,
    hist: std::collections::VecDeque<(Instant, f64)>,
    first_frame: bool,
    /// Keep nudging the active window back onto its monitor until this moment.
    fit_until: Option<Instant>,
    settings_pos: Option<Pos2>,
    edit_preset: Option<usize>,
    edit_pos: Option<Pos2>,
    was_settings: bool,
    update: UpdateStatus,
    update_rx: Option<std::sync::mpsc::Receiver<UpdateStatus>>,
    install_rx: Option<std::sync::mpsc::Receiver<Result<std::path::PathBuf, String>>>,
    update_error: Option<String>,
    qrz_rx: Option<std::sync::mpsc::Receiver<QrzMsg>>,
    /// Callsign being looked up, with the elevation to use once it is found.
    qrz_busy: Option<(String, Option<f64>)>,
    qrz_cache: std::collections::HashMap<String, crate::qrz::Station>,
    qrz_error: Option<String>,
    qrz_test: Option<Result<(), String>>,
    started: Instant,
    next_try: Instant,
    /// Raw rotor azimuth / elevation as reported (before the heading offset is removed).
    heading: Option<f64>,
    el: Option<f64>,
    /// Compass bearing / elevation last commanded, shown on the dial.
    target: Option<f64>,
    target_el: Option<f64>,
    manual: String,
    ports: Vec<String>,
    log: Vec<String>,
    jog: Option<Jog>,
    show_settings: bool,
    settings_tab: SettingsTab,
    applied_dark: Option<bool>,
    cal_ok: bool,
    cal_armed: Option<Cal>,
    server: Option<(Server, (u16, bool, u16))>,
    shared: Shared,
    remote_tx: Sender<Remote>,
    remote_rx: Receiver<Remote>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        crate::theme::install_fonts(&cc.egui_ctx);
        let (tx, rx) = link::spawn(cc.egui_ctx.clone());
        let (remote_tx, remote_rx) = crossbeam_channel::unbounded();
        let cfg = Config::load();
        let want_link = cfg.auto_connect && !std::env::args().any(|a| a == "--demo");
        crate::update::cleanup_old();
        let mut app = Self {
            cfg,
            tx,
            rx,
            connected: None,
            want_link,
            sent_title: String::new(),
            sent_on_top: None,
            last_move: Instant::now() - Duration::from_secs(10),
            move_dir: None,
            hist: std::collections::VecDeque::new(),
            first_frame: true,
            fit_until: Some(Instant::now() + Duration::from_millis(1500)),
            settings_pos: None,
            edit_preset: None,
            edit_pos: None,
            was_settings: false,
            update: UpdateStatus::Idle,
            update_rx: None,
            install_rx: None,
            update_error: None,
            qrz_rx: None,
            qrz_busy: None,
            qrz_cache: std::collections::HashMap::new(),
            qrz_error: None,
            qrz_test: None,
            started: Instant::now(),
            next_try: Instant::now(),
            heading: None,
            el: None,
            target: None,
            target_el: None,
            manual: String::new(),
            ports: link::list_ports(),
            log: Vec::new(),
            jog: None,
            show_settings: false,
            settings_tab: SettingsTab::Connection,
            applied_dark: None,
            cal_ok: false,
            cal_armed: None,
            server: None,
            shared: Arc::new(Mutex::new(None)),
            remote_tx,
            remote_rx,
        };
        if app.cfg.check_updates {
            app.start_update_check(&cc.egui_ctx);
        }
        app
    }

    fn start_update_check(&mut self, ctx: &egui::Context) {
        self.update = UpdateStatus::Checking;
        self.update_rx = Some(crate::update::spawn_check(ctx.clone()));
    }

    fn start_install(&mut self, ctx: &egui::Context) {
        let UpdateStatus::Available { asset: Some(asset), .. } = &self.update else { return };
        let asset = asset.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let c = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(crate::update::install(&asset));
            c.request_repaint();
        });
        self.install_rx = Some(rx);
        self.update_error = None;
    }

    fn start_qrz(&mut self, ctx: &egui::Context, call: Option<(String, Option<f64>)>) {
        let (user, pass) = (self.cfg.qrz_user.clone(), self.cfg.qrz_pass.clone());
        let (tx, rx) = std::sync::mpsc::channel();
        let c = ctx.clone();
        self.qrz_error = None;
        match call {
            Some((call, el)) => {
                self.qrz_busy = Some((call.clone(), el));
                std::thread::spawn(move || {
                    let _ = tx.send(QrzMsg::Lookup(crate::qrz::lookup(&user, &pass, &call)));
                    c.request_repaint();
                });
            }
            None => {
                self.qrz_test = None;
                std::thread::spawn(move || {
                    let _ = tx.send(QrzMsg::Test(crate::qrz::test(&user, &pass)));
                    c.request_repaint();
                });
            }
        }
        self.qrz_rx = Some(rx);
    }

    fn poll_qrz(&mut self) {
        let Some(rx) = &self.qrz_rx else { return };
        let Ok(msg) = rx.try_recv() else { return };
        self.qrz_rx = None;
        match msg {
            QrzMsg::Test(r) => self.qrz_test = Some(r),
            QrzMsg::Lookup(r) => {
                let busy = self.qrz_busy.take();
                match r {
                    Ok(st) => {
                        let el = busy.and_then(|(_, el)| el);
                        match self.bearing_to_point((st.lat, st.lon)) {
                            Some((b, _)) => self.go(b, el),
                            None => self.qrz_error = Some("Set your own grid square in Settings → Location".into()),
                        }
                        self.qrz_cache.insert(st.call.clone(), st);
                    }
                    Err(e) => self.qrz_error = Some(e),
                }
            }
        }
    }

    /// Bearing and distance (km) from my grid square to a latitude / longitude.
    fn bearing_to_point(&self, to: (f64, f64)) -> Option<(f64, f64)> {
        let (brg, dist) = grid::bearing_distance(grid::to_latlon(&self.cfg.my_grid)?, to);
        Some(if self.cfg.long_path { ((brg + 180.0) % 360.0, 2.0 * std::f64::consts::PI * 6371.0 - dist) } else { (brg, dist) })
    }

    fn poll_update(&mut self) {
        if let Some(rx) = &self.install_rx
            && let Ok(res) = rx.try_recv()
        {
            self.install_rx = None;
            match res {
                Ok(exe) => {
                    // The new version is in place: save settings, start it and quit this copy.
                    self.cfg.save();
                    let _ = std::process::Command::new(&exe).spawn();
                    std::process::exit(0);
                }
                Err(e) => self.update_error = Some(e),
            }
        }
        let Some(rx) = &self.update_rx else { return };
        if let Ok(st) = rx.try_recv() {
            self.update_rx = None;
            self.update = st;
        }
    }

    /// "New version available" button: installs in place on Windows, otherwise opens the release page.
    fn update_chip(&mut self, ui: &mut egui::Ui, p: &theme::Pal, full_width: bool) {
        let UpdateStatus::Available { version, url, asset } = self.update.clone() else { return };
        if self.install_rx.is_some() {
            ui.add(egui::Spinner::new().size(14.0));
            ui.label(RichText::new("Downloading…").size(12.5));
            return;
        }
        let can_install = cfg!(windows) && asset.is_some();
        let failed = self.update_error.is_some();
        let text = if failed {
            "Update failed, retry".to_string()
        } else if can_install {
            format!("Upgrade to {version}")
        } else {
            format!("Version {version}")
        };
        let tip = match (&self.update_error, can_install) {
            (Some(e), _) => e.clone(),
            (None, true) => "Download the new version and restart".to_string(),
            (None, false) => "Open the release page".to_string(),
        };
        let btn = primary_button(text, p);
        let r = if full_width { ui.add_sized([ui.available_width(), 24.0], btn) } else { ui.add(btn) };
        if r.on_hover_text(tip).clicked() {
            if can_install {
                self.start_install(ui.ctx());
            } else {
                ui.ctx().open_url(egui::OpenUrl::new_tab(url));
            }
        }
    }

    fn log(&mut self, l: String) {
        let l = format!("{:8.1} {l}", self.started.elapsed().as_secs_f32());
        if !(l.contains(" > ") || l.contains(" < ")) && self.log.last().map(|x| &x[9..]) == Some(&l[9..]) {
            return; // collapse repeated reconnect errors
        }
        self.log.push(l);
        if self.log.len() > 200 {
            self.log.remove(0);
        }
    }

    fn poll(&mut self) {
        while let Ok(e) = self.rx.try_recv() {
            match e {
                Event::Position(Pos { az, el }) => {
                    let new = az * self.cfg.span_scale;
                    // Moving means a real change, not the 1-2 degree flicker of the interface:
                    // at least 3 degrees of spread over the last 2 seconds.
                    let now = Instant::now();
                    self.hist.push_back((now, new));
                    while self.hist.front().is_some_and(|(t, _)| now.duration_since(*t) > Duration::from_secs(2)) {
                        self.hist.pop_front();
                    }
                    let lo = self.hist.iter().map(|x| x.1).fold(f64::MAX, f64::min);
                    let hi = self.hist.iter().map(|x| x.1).fold(f64::MIN, f64::max);
                    if hi - lo >= 3.0 {
                        let first = self.hist.front().map_or(new, |x| x.1);
                        self.move_dir = Some(if new >= first { Jog::Cw } else { Jog::Ccw });
                        self.last_move = now;
                    }
                    self.heading = Some(new);
                    if el.is_some() {
                        self.el = el;
                    }
                }
                Event::Connected(c) => {
                    if c.is_none() {
                        self.hist.clear();
                        self.heading = None;
                        self.el = None;
                    }
                    if c.is_some() {
                        self.send_speed();
                    }
                    self.connected = c;
                }
                Event::Log(l) => self.log(l),
            }
        }
        while let Ok(r) = self.remote_rx.try_recv() {
            match r {
                Remote::Goto { az, el } => self.go(az, el),
                Remote::Stop => self.stop(),
                Remote::Move(j) => {
                    self.target = None;
                    let _ = self.tx.send(Cmd::Jog(j));
                }
            }
        }
        *self.shared.lock().unwrap() = self.compass_heading().map(|a| (a, self.el.unwrap_or(0.0)));
    }

    /// Start or stop the rotctld-compatible server to match the settings.
    fn sync_server(&mut self, ctx: &egui::Context) {
        let want = self.cfg.serve.then_some((self.cfg.serve_port, self.cfg.has_elevation(), self.cfg.max_travel));
        if self.server.as_ref().map(|s| s.1) == want {
            return;
        }
        self.server = None;
        if let Some((port, elev, max)) = want {
            match server::start(port, self.shared.clone(), self.remote_tx.clone(), ctx.clone(), elev, max) {
                Ok(s) => {
                    self.server = Some((s, (port, elev, max)));
                    self.log(format!("Sharing the rotor on TCP port {port}"));
                }
                Err(e) => {
                    self.cfg.serve = false;
                    self.log(format!("Cannot listen on port {port}: {e}"));
                }
            }
        }
    }

    /// Tell a GS-232 interface the chosen rotation speed (it keeps it until changed).
    fn send_speed(&self) {
        if self.cfg.protocol == Proto::Gs232 && self.cfg.speed > 0 {
            let _ = self.tx.send(Cmd::Calibrate(Cal::Speed(self.cfg.speed)));
        }
    }

    fn driver_cfg(&self) -> DriverCfg {
        DriverCfg {
            proto: self.cfg.protocol,
            elevation: self.cfg.has_elevation(),
            az_max: self.cfg.max_travel as f64,
            el_max: self.cfg.el_max as f64,
        }
    }

    /// Re-open the link every few seconds while it is wanted but down.
    fn auto_reconnect(&mut self, ctx: &egui::Context) {
        let ready = self.cfg.protocol.is_network() || !self.cfg.port.is_empty();
        if !self.cfg.auto_connect || !self.want_link || self.connected.is_some() || !ready {
            return;
        }
        if Instant::now() >= self.next_try {
            self.next_try = Instant::now() + Duration::from_secs(3);
            if !self.cfg.protocol.is_network() {
                self.ports = link::list_ports();
            }
            self.connect();
        }
        ctx.request_repaint_after(Duration::from_secs(1));
    }

    fn connect(&mut self) {
        let transport = if self.cfg.protocol.is_network() {
            Transport::Tcp { addr: format!("{}:{}", self.cfg.host, self.cfg.net_port) }
        } else {
            Transport::Serial { port: self.cfg.port.clone(), baud: self.cfg.baud }
        };
        let _ = self.tx.send(Cmd::Connect { transport, drv: self.driver_cfg() });
    }

    /// Rotor azimuth command for a compass bearing, or None until the heading is known.
    fn command_for(&self, bearing: f64) -> Option<f64> {
        let b = (bearing + self.cfg.offset as f64).rem_euclid(360.0);
        Some(drivers::pick_target(b, self.heading?, self.cfg.max_travel))
    }

    /// Compass heading (offset removed, 0..360) from the rotor's raw reading.
    fn compass_heading(&self) -> Option<f64> {
        self.heading.map(|h| (h - self.cfg.offset as f64).rem_euclid(360.0))
    }

    /// Turn to a compass bearing, optionally changing elevation too.
    fn go(&mut self, bearing: f64, el: Option<f64>) {
        let Some(az) = self.command_for(bearing) else { return };
        if self.cfg.has_elevation() {
            let max = self.cfg.el_max as f64;
            self.target_el = el.map(|e| e.clamp(0.0, max)).or(self.target_el).or(self.el);
        }
        self.target = Some(bearing.rem_euclid(360.0));
        let el = self.cfg.has_elevation().then(|| self.target_el.unwrap_or(0.0));
        let _ = self.tx.send(Cmd::GoTo { az: az / self.cfg.span_scale, el });
    }

    /// Change elevation only; the azimuth target stays where the rotor is now.
    fn set_elevation(&mut self, el: f64) {
        let Some(az) = self.heading else { return };
        let el = el.clamp(0.0, self.cfg.el_max as f64);
        self.target_el = Some(el);
        let _ = self.tx.send(Cmd::GoTo { az: az / self.cfg.span_scale, el: Some(el) });
    }

    /// Distance in the unit chosen in Settings.
    fn dist(&self, km: f64) -> String {
        if self.cfg.miles { format!("{:.0} mi", km * 0.621_371) } else { format!("{km:.0} km") }
    }

    fn bearing_to(&self, grid_sq: &str) -> Option<(f64, f64)> {
        grid::between(&self.cfg.my_grid, grid_sq, self.cfg.long_path)
    }

    fn stop(&mut self) {
        self.target = None;
        self.target_el = None;
        let _ = self.tx.send(Cmd::Stop);
    }

    // ------------------------------------------------------------------ status helpers

    /// Direction the antenna is turning now (jog held, or the reading changed a moment ago).
    fn moving(&self) -> Option<Jog> {
        if self.jog.is_some() {
            return self.jog;
        }
        if self.last_move.elapsed() < Duration::from_millis(900) { self.move_dir } else { None }
    }

    fn status_text(&self) -> (String, StatusKind) {
        match &self.connected {
            Some(n) if n == "Demo rotor" => ("Demo rotor".into(), StatusKind::Demo),
            Some(n) => (format!("Connected · {n}"), StatusKind::Ok),
            None if self.want_link && self.cfg.auto_connect => ("Connecting…".into(), StatusKind::Busy),
            None => ("Not connected".into(), StatusKind::Off),
        }
    }

    fn heading_text(&self) -> String {
        match (self.compass_heading(), self.heading) {
            (Some(c), Some(raw)) if raw > 360.0 => format!("{c:.0}° (overlap)"),
            (Some(c), _) => format!("{c:.0}°"),
            _ => "---".into(),
        }
    }

    fn title(&self) -> String {
        let mut t = format!("Rotor Controller {}", self.heading_text());
        if self.cfg.has_elevation()
            && let Some(e) = self.el
        {
            t.push_str(&format!(" / {e:.0}°"));
        }
        t
    }

    // ------------------------------------------------------------------ main window pieces

    /// Top bar of the full window: connection status, quick connect, settings, compact toggle.
    fn top_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let p = theme::pal(ui.visuals());
        ui.horizontal(|ui| {
            let (text, kind) = self.status_text();
            status_pill(ui, &p, &text, kind);
            if self.connected.is_none() {
                if !self.cfg.protocol.is_network() {
                    let sel = if self.cfg.port.is_empty() { "Select port".to_string() } else { self.cfg.port.clone() };
                    egui::ComboBox::from_id_salt("port").selected_text(sel).width(96.0).show_ui(ui, |ui| {
                        for pt in self.ports.clone() {
                            if ui.selectable_label(self.cfg.port == pt, &pt).clicked() {
                                self.cfg.port = pt;
                            }
                        }
                    });
                }
                let ok = self.cfg.protocol.is_network() || !self.cfg.port.is_empty();
                if ui.add_enabled(ok, primary_button("Connect", &p)).clicked() {
                    self.want_link = true;
                    self.connect();
                }
                if ui.button("Demo").on_hover_text("Simulated rotor, no hardware needed").clicked() {
                    self.want_link = false;
                    let _ = self.tx.send(Cmd::Connect { transport: Transport::Demo, drv: self.driver_cfg() });
                }
            } else if ui.button("Disconnect").clicked() {
                self.want_link = false;
                let _ = self.tx.send(Cmd::Disconnect);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_button(ui, Icon::Gear, "Settings").clicked() {
                    self.show_settings = !self.show_settings;
                }
                if icon_button(ui, Icon::Shrink, "Compact view").clicked() {
                    self.set_compact(ctx, true);
                }
                self.update_chip(ui, &p, false);
            });
        });
    }

    /// The azimuth target while the rotor is still on its way; `None` once it has arrived.
    fn pending_target(&self) -> Option<f64> {
        let t = self.target?;
        match self.compass_heading() {
            Some(h) if ((t - h + 540.0).rem_euclid(360.0) - 180.0).abs() < 1.5 => None,
            _ => Some(t),
        }
    }

    /// Right-hand column of the compact view: link dot, icons, stacked controls, target.
    fn compact_side(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let p = theme::pal(ui.visuals());
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            ui.horizontal(|ui| {
                let (text, kind) = self.status_text();
                let (r, resp) = ui.allocate_exact_size(vec2(12.0, 26.0), Sense::hover());
                ui.painter().circle_filled(r.center(), 4.5, kind.color(&p));
                resp.on_hover_text(text);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    if icon_button(ui, Icon::Gear, "Settings").clicked() {
                        self.show_settings = !self.show_settings;
                    }
                    if icon_button(ui, Icon::Expand, "Full view").clicked() {
                        self.set_compact(ctx, false);
                    }
                });
            });
            self.controls_row(ui, 28.0, true);
            if self.connected.is_none() && (self.cfg.protocol.is_network() || !self.cfg.port.is_empty()) {
                if ui.add_sized([ui.available_width(), 24.0], primary_button("Connect", &p)).clicked() {
                    self.want_link = true;
                    self.connect();
                }
            } else if matches!(self.update, UpdateStatus::Available { .. }) {
                self.update_chip(ui, &p, true);
            } else if let Some(t) = self.pending_target() {
                ui.label(RichText::new(format!("Target {t:.0}°")).size(12.5).color(p.amber));
            } else {
                ui.label(RichText::new("Click the rose").size(12.0).color(p.dim));
            }
        });
    }

    /// CCW / STOP / CW (and elevation) buttons. Hold to move; the buttons pulse while moving.
    fn controls_row(&mut self, ui: &mut egui::Ui, h: f32, stacked: bool) {
        let p = theme::pal(ui.visuals());
        let moving = self.moving();
        let pulse = moving.map(|_| pulse_value(ui.ctx()));
        let el = self.cfg.has_elevation();
        let gap = ui.spacing().item_spacing.x;
        let weights: &[f32] = if el { &[1.25, 1.0, 1.25, 0.7, 0.7] } else { &[1.25, 1.0, 1.25] };
        let unit = (ui.available_width() - gap * (weights.len() as f32 - 1.0)) / weights.iter().sum::<f32>();
        let mut want: Option<Jog> = None;
        let pulse_for = |j: Jog| if moving == Some(j) { pulse } else { None };
        if stacked {
            // Three full-width rows so the labels have room: CCW, STOP, CW (and elevation).
            let w = ui.available_width();
            let half = (w - gap) / 2.0;
            let ccw = jog_button(ui, vec2(w, h), "CCW", Some(Dir::Left), p.card_hi, p.amber, pulse_for(Jog::Ccw), &p);
            if jog_button(ui, vec2(w, h), "STOP", None, p.red.gamma_multiply(0.85), p.red, None, &p).clicked() {
                self.stop();
            }
            let cw = jog_button(ui, vec2(w, h), "CW", Some(Dir::Right), p.card_hi, p.amber, pulse_for(Jog::Cw), &p);
            if ccw.is_pointer_button_down_on() {
                want = Some(Jog::Ccw);
            } else if cw.is_pointer_button_down_on() {
                want = Some(Jog::Cw);
            }
            if el {
                ui.horizontal(|ui| {
                    let up = jog_button(ui, vec2(half, h), "", Some(Dir::Up), p.card_hi, p.amber, pulse_for(Jog::Up), &p);
                    let dn = jog_button(ui, vec2(half, h), "", Some(Dir::Down), p.card_hi, p.amber, pulse_for(Jog::Down), &p);
                    if up.is_pointer_button_down_on() {
                        want = Some(Jog::Up);
                    } else if dn.is_pointer_button_down_on() {
                        want = Some(Jog::Down);
                    }
                });
            }
        } else {
            ui.horizontal(|ui| {
                let sz = |i: usize| vec2(unit * weights[i], h);
                let ccw = jog_button(ui, sz(0), "CCW", Some(Dir::Left), p.card_hi, p.amber, pulse_for(Jog::Ccw), &p);
                let stop = jog_button(ui, sz(1), "STOP", None, p.red.gamma_multiply(0.85), p.red, None, &p);
                let cw = jog_button(ui, sz(2), "CW", Some(Dir::Right), p.card_hi, p.amber, pulse_for(Jog::Cw), &p);
                if stop.clicked() {
                    self.stop();
                }
                if ccw.is_pointer_button_down_on() {
                    want = Some(Jog::Ccw);
                } else if cw.is_pointer_button_down_on() {
                    want = Some(Jog::Cw);
                }
                if el {
                    let up = jog_button(ui, sz(3), "", Some(Dir::Up), p.card_hi, p.amber, pulse_for(Jog::Up), &p);
                    let dn = jog_button(ui, sz(4), "", Some(Dir::Down), p.card_hi, p.amber, pulse_for(Jog::Down), &p);
                    if up.is_pointer_button_down_on() {
                        want = Some(Jog::Up);
                    } else if dn.is_pointer_button_down_on() {
                        want = Some(Jog::Down);
                    }
                }
            });
        }
        // Once a jog has started, keep it until the mouse button is really released, whatever
        // happens to the widget under the pointer (layout changes, repaints).
        if want.is_none() && self.jog.is_some() && ui.input(|i| i.pointer.primary_down()) {
            want = self.jog;
        }
        if want != self.jog {
            self.jog = want;
            match want {
                Some(j) => {
                    self.target = None;
                    let _ = self.tx.send(Cmd::Jog(j));
                }
                None => {
                    let _ = self.tx.send(Cmd::Stop);
                }
            }
        }
    }

    fn elevation_bar(&mut self, ui: &mut egui::Ui) {
        let p = theme::pal(ui.visuals());
        let max = self.cfg.el_max as f32;
        let (resp, painter) = ui.allocate_painter(vec2(ui.available_width(), 30.0), Sense::click());
        let r = resp.rect;
        painter.rect_filled(r, 9.0, p.well);
        if let Some(e) = self.el {
            let w = (r.width() * (e as f32 / max).clamp(0.0, 1.0)).max(10.0);
            painter.rect_filled(egui::Rect::from_min_size(r.min, vec2(w, r.height())), 9.0, p.accent.gamma_multiply(0.55));
        }
        if let Some(t) = self.target_el {
            let x = r.left() + r.width() * (t as f32 / max).clamp(0.0, 1.0);
            painter.line_segment([egui::pos2(x, r.top() + 4.0), egui::pos2(x, r.bottom() - 4.0)], Stroke::new(2.5, p.amber));
        }
        painter.rect_stroke(r, 9.0, Stroke::new(1.0, p.stroke), egui::StrokeKind::Inside);
        let txt = self.el.map_or("Elevation  ---".to_string(), |e| format!("Elevation  {e:.0}°"));
        painter.text(r.center(), Align2::CENTER_CENTER, txt, theme::bold(13.5), p.text);
        if resp.hovered() && self.heading.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if self.heading.is_some()
            && let Some(pt) = resp.interact_pointer_pos().filter(|_| resp.clicked())
        {
            self.set_elevation(((pt.x - r.left()) / r.width()).clamp(0.0, 1.0) as f64 * max as f64);
        }
    }

    /// The preset buttons. Left click turns the rotor; right click edits the preset.
    fn presets(&mut self, ui: &mut egui::Ui, compact: bool) {
        let p = theme::pal(ui.visuals());
        let have_home = grid::to_latlon(&self.cfg.my_grid).is_some();
        let cols = 5usize;
        let gap = if compact { 4.0 } else { 8.0 };
        let w = (ui.available_width() - gap * (cols as f32 - 1.0)) / cols as f32;
        let h = if compact { 30.0 } else { 58.0 };
        let mut go: Option<f64> = None;
        for row in 0..1 {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for i in row * cols..(row + 1) * cols {
                    let pr = self.cfg.presets[i].clone();
                    let info = self.bearing_to(&pr.grid);
                    let empty = pr.label.trim().is_empty() && pr.grid.trim().is_empty();
                    let enabled = info.is_some() && self.heading.is_some();
                    let sub = match (info, empty) {
                        (Some((b, d)), _) => Some(format!("{b:.0}°  {}", self.dist(d))),
                        (None, true) => Some("right-click".to_string()),
                        (None, false) if !have_home => Some("set my grid".to_string()),
                        (None, false) if pr.grid.trim().is_empty() => Some("set grid".to_string()),
                        (None, false) => Some("check grid".to_string()),
                    };
                    let label = if pr.label.trim().is_empty() { format!("{}", i + 1) } else { pr.label.clone() };
                    let active = self.target.is_some_and(|t| info.is_some_and(|(b, _)| (t - b).abs() < 0.6));
                    let r = preset_button(ui, vec2(w, h), &label, if compact { None } else { sub }, enabled, active, empty, &p);
                    let tip = match info {
                        Some((b, d)) => {
                            let cmd = self.command_for(b).map_or(String::new(), |a| format!(", rotor {a:.0}°"));
                            format!("{} → {:.0}°, {}{}\nRight-click to edit", pr.grid.to_uppercase(), b, self.dist(d), cmd)
                        }
                        None if empty => "Empty. Right-click to set a label and grid square".to_string(),
                        None if !have_home => "Set your grid square in Settings → Location. Right-click to edit".to_string(),
                        None => "Invalid grid square. Right-click to edit".to_string(),
                    };
                    let r = r.on_hover_text(tip);
                    if r.clicked() && enabled {
                        go = info.map(|(b, _)| b);
                    }
                    if compact {
                        if r.secondary_clicked() {
                            self.edit_preset = Some(i);
                            self.edit_pos = ui.ctx().input(|inp| inp.viewport().outer_rect).map(|o| Pos2::new(o.right() + 12.0, o.top()));
                            self.fit_until = Some(Instant::now() + Duration::from_millis(1500));
                        }
                    } else {
                        egui::Popup::context_menu(&r).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                            if self.preset_menu(ui, i) {
                                ui.close();
                            }
                        });
                    }
                }
            });
        }
        if let Some(b) = go {
            self.go(b, None);
        }
    }

    /// Right-click editor for one preset.
    fn preset_menu(&mut self, ui: &mut egui::Ui, i: usize) -> bool {
        let mut done = false;
        let p = theme::pal(ui.visuals());
        ui.set_width(240.0);
        ui.spacing_mut().button_padding = vec2(12.0, 6.0);
        ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
        ui.label(RichText::new(format!("Preset {}", i + 1)).font(theme::bold(15.0)));
        ui.add_space(2.0);
        ui.label(RichText::new("Label").size(12.0).color(p.dim));
        ui.add_sized([240.0, 30.0], egui::TextEdit::singleline(&mut self.cfg.presets[i].label).char_limit(14).hint_text("e.g. Japan").margin(egui::Margin::symmetric(8, 6)));
        ui.label(RichText::new("Grid square").size(12.0).color(p.dim));
        ui.add_sized([240.0, 30.0], egui::TextEdit::singleline(&mut self.cfg.presets[i].grid).char_limit(8).hint_text("e.g. PM95").margin(egui::Margin::symmetric(8, 6)));
        let grid_txt = self.cfg.presets[i].grid.trim().to_string();
        if grid_txt.is_empty() {
            ui.label(RichText::new("Enter a 4 or 6 character locator").size(12.0).color(p.dim));
        } else {
            match self.bearing_to(&grid_txt) {
                Some((b, d)) => ui.label(RichText::new(format!("{b:.0}°   ·   {}", self.dist(d))).color(p.green)),
                None if grid::to_latlon(&grid_txt).is_some() => ui.label(RichText::new("Set your own grid in Settings → Location").size(12.0).color(p.amber)),
                None => ui.label(RichText::new("Not a valid grid square").color(p.red)),
            };
        }
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            let ok = self.heading.is_some() && self.bearing_to(&grid_txt).is_some();
            if ui.add_enabled(ok, primary_button("Turn here", &p)).clicked() {
                if let Some((b, _)) = self.bearing_to(&grid_txt) {
                    self.go(b, None);
                }
                done = true;
            }
            if ui.button("Clear").clicked() {
                self.cfg.presets[i].label.clear();
                self.cfg.presets[i].grid.clear();
            }
            if ui.button("Done").clicked() {
                done = true;
            }
        });
        done
    }

    /// Compact mode has no room for a popup (it would be clipped to the small window), so the
    /// preset editor opens as its own small window beside the app.
    fn preset_window(&mut self, ctx: &egui::Context) {
        let Some(i) = self.edit_preset else { return };
        let mut builder = egui::ViewportBuilder::default()
            .with_title(format!("Edit preset {}", i + 1))
            .with_inner_size([296.0, 232.0])
            .with_resizable(false);
        if let Some(pos) = self.edit_pos {
            builder = builder.with_position(pos);
        }
        let mut close = false;
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("preset_viewport"), builder, |ui, _class| {
            let p = theme::pal(ui.visuals());
            if ui.ctx().input(|i| i.viewport().close_requested() || i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
            egui::CentralPanel::default().frame(egui::Frame::new().fill(p.card).inner_margin(egui::Margin::same(16))).show(ui, |ui| {
                if self.preset_menu(ui, i) {
                    close = true;
                }
            });
        });
        if close {
            self.edit_preset = None;
        }
    }

    /// Round compass dial: heading needle, target marker, sweep to the target, click to turn.
    fn dial(&mut self, ui: &mut egui::Ui, size: f32) {
        let p = theme::pal(ui.visuals());
        let (resp, painter) = ui.allocate_painter(vec2(size, size), Sense::click());
        let c = resp.rect.center();
        let r = size / 2.0 - 8.0;
        let moving = self.moving().is_some();
        let k = (size / 320.0).clamp(0.45, 1.0);
        let pt = |deg: f64, rad: f32| c + vec2((deg.to_radians().sin() as f32) * rad, -(deg.to_radians().cos() as f32) * rad);
        if moving {
            let k = pulse_value(ui.ctx());
            painter.circle_stroke(c, r + 3.0 + 2.0 * k, Stroke::new(2.0 + 2.0 * k, p.amber.gamma_multiply(0.25 + 0.5 * k)));
        }
        painter.circle_filled(c, r, p.well);
        painter.circle_stroke(c, r, Stroke::new(2.0, p.stroke));
        painter.circle_stroke(c, r * 0.62, Stroke::new(1.0, p.stroke.gamma_multiply(0.6)));
        for d in (0..360).step_by(5) {
            let (len, w, col) = if d % 30 == 0 {
                (11.0 * k.max(0.6), 2.0 * k.max(0.7), p.dim)
            } else if d % 10 == 0 {
                (7.0 * k.max(0.6), 1.2, p.dim.gamma_multiply(0.8))
            } else {
                (4.0 * k.max(0.6), 1.0, p.dim.gamma_multiply(0.45))
            };
            painter.line_segment([pt(d as f64, r - 2.0), pt(d as f64, r - 2.0 - len)], Stroke::new(w, col));
        }
        let card_font = theme::bold((size / 15.0).clamp(10.0, 19.0));
        for (d, t) in [(0, "N"), (90, "E"), (180, "S"), (270, "W")] {
            let col = if d == 0 { p.red } else { p.text };
            painter.text(pt(d as f64, r - 25.0 * k.max(0.55)), Align2::CENTER_CENTER, t, card_font.clone(), col);
        }
        let ord_font = theme::bold((size / 24.0).clamp(8.5, 13.0));
        for (d, t) in [(45, "NE"), (135, "SE"), (225, "SW"), (315, "NW")] {
            painter.text(pt(d as f64, r - 25.0 * k.max(0.55)), Align2::CENTER_CENTER, t, ord_font.clone(), p.dim);
        }
        if size > 230.0 {
            for d in (30..360).step_by(30).filter(|d| d % 90 != 0) {
                painter.text(pt(d as f64, r - 25.0), Align2::CENTER_CENTER, d.to_string(), FontId::proportional(10.5), p.dim);
            }
        }
        let hd = self.compass_heading();
        let pending = self.pending_target();
        if let (Some(t), Some(h)) = (pending, hd) {
            let delta = (t - h + 540.0).rem_euclid(360.0) - 180.0;
            let steps = (delta.abs() / 3.0).ceil().max(1.0) as usize;
            let pts: Vec<Pos2> = (0..=steps).map(|k| pt(h + delta * k as f64 / steps as f64, r * 0.74)).collect();
            painter.add(egui::Shape::line(pts, Stroke::new(5.0, p.amber.gamma_multiply(0.55))));
        }
        if let Some(t) = pending {
            let a = pt(t, r + 1.0);
            let b1 = pt(t - 3.5, r - 14.0);
            let b2 = pt(t + 3.5, r - 14.0);
            painter.add(egui::Shape::convex_polygon(vec![a, b2, b1], p.amber, Stroke::NONE));
            painter.line_segment([pt(t, r * 0.30), pt(t, r - 14.0)], Stroke::new(1.5, p.amber.gamma_multiply(0.7)));
        }
        if let Some(h) = hd {
            let tip = pt(h, r * 0.80);
            let off = pt(h - 90.0, (6.0 * k).max(3.2)) - c;
            let left = c + off;
            let right = c - off;
            let tail = pt(h + 180.0, r * 0.18);
            painter.add(egui::Shape::convex_polygon(vec![tip, left, tail, right], p.accent, Stroke::new(1.0, p.accent.gamma_multiply(0.6))));
            painter.line_segment([c, tip], Stroke::new(1.0, Color32::WHITE.gamma_multiply(0.5)));
        }
        painter.circle_filled(c, 8.0 * k.max(0.6), p.card);
        painter.circle_stroke(c, 8.0 * k.max(0.6), Stroke::new(2.0, p.accent));
        let txt = self.heading_text().replace(" (overlap)", "");
        painter.text(c + vec2(0.0, r * 0.45), Align2::CENTER_CENTER, txt, theme::bold(size / if size < 200.0 { 6.0 } else { 7.5 }), p.text);
        let sub = if self.heading.is_some_and(|h| h > 360.0) {
            "overlap".to_string()
        } else if self.heading.is_none() {
            "no reading".to_string()
        } else {
            String::new()
        };
        if !sub.is_empty() {
            let pos = c + vec2(0.0, r * 0.45 + size / 11.0);
            if sub == "overlap" {
                // In the middle of the rose, above the heading number so that stays visible.
                let sub = "OVERLAP".to_string();
                let pos = c + vec2(0.0, r * 0.17);
                // Red chip; the text color follows the theme so it stays readable.
                let fg = if p.dark { Color32::BLACK } else { Color32::WHITE };
                let galley = painter.layout_no_wrap(sub, theme::bold(12.0), fg);
                let chip = egui::Rect::from_center_size(pos, galley.size() + vec2(16.0, 6.0));
                painter.rect_filled(chip, chip.height() / 2.0, p.red);
                painter.galley(chip.center() - galley.size() / 2.0, galley, fg);
            } else {
                painter.text(pos, Align2::CENTER_CENTER, sub, FontId::proportional(12.0), p.amber);
            }
        }
        if resp.hovered() && self.heading.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if self.heading.is_some()
            && let Some(pos) = resp.interact_pointer_pos().filter(|_| resp.clicked())
        {
            let b = ((pos.x - c.x) as f64).atan2(-((pos.y - c.y) as f64)).to_degrees().rem_euclid(360.0);
            self.manual = format!("{:.0}", b);
            self.go(b.round(), None);
        }
    }

    fn manual_row(&mut self, ui: &mut egui::Ui) {
        let p = theme::pal(ui.visuals());
        ui.horizontal(|ui| {
            let hint = if self.cfg.has_elevation() { "Callsign, grid or bearing, e.g. W1AW, JO50 or 270 [elevation]" } else { "Callsign, grid square or bearing, e.g. W1AW" };
            let go_w = 64.0;
            let r = ui.add(egui::TextEdit::singleline(&mut self.manual).desired_width(ui.available_width() - go_w - ui.spacing().item_spacing.x - 2.0).hint_text(hint).margin(egui::Margin::symmetric(10, 7)));
            if r.changed() {
                self.qrz_error = None;
            }
            let go = ui.add_enabled(self.heading.is_some(), primary_button("Go", &p).min_size(vec2(go_w, 32.0))).clicked()
                || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && self.heading.is_some());
            let txt = self.manual.clone();
            let mut tok = txt.split_whitespace();
            let first = tok.next().unwrap_or("");
            let el = tok.next().and_then(|t| t.parse::<f64>().ok()).filter(|_| self.cfg.has_elevation());
            let brg = first.parse::<f64>().ok().map(|b| (b, None)).or_else(|| self.bearing_to(first).map(|(b, d)| (b, Some(d))));
            if go && let Some((b, _)) = brg {
                self.go(b, el);
            } else if go && crate::qrz::looks_like_callsign(first) && self.qrz_busy.is_none() {
                let call = first.to_uppercase();
                if let Some(st) = self.qrz_cache.get(&call).cloned() {
                    match self.bearing_to_point((st.lat, st.lon)) {
                        Some((b, _)) => self.go(b, el),
                        None => self.qrz_error = Some("Set your own grid square in Settings → Location".into()),
                    }
                } else if self.cfg.qrz_user.trim().is_empty() || self.cfg.qrz_pass.is_empty() {
                    self.qrz_error = Some("Add your QRZ.com login in Settings → Location to look up callsigns".into());
                } else {
                    self.start_qrz(&ui.ctx().clone(), Some((call, el)));
                }
            }
        });
        let txt = self.manual.clone();
        let first = txt.split_whitespace().next().unwrap_or("");
        let call_info = self.qrz_cache.get(&first.to_uppercase()).cloned().and_then(|s| self.bearing_to_point((s.lat, s.lon)).map(|(b, d)| (s, b, d)));
        ui.horizontal(|ui| {
            match (first.parse::<f64>().ok(), self.bearing_to(first)) {
                _ if self.qrz_busy.is_some() => ui.label(RichText::new("Looking up the callsign on QRZ.com…").size(12.5).color(p.dim)),
                _ if self.qrz_error.is_some() => ui.label(RichText::new(self.qrz_error.clone().unwrap_or_default()).size(12.5).color(p.red)),
                (None, None) if call_info.is_some() => {
                    let (s, b, d) = call_info.unwrap();
                    let who = if s.name.is_empty() { s.call.clone() } else { format!("{} ({})", s.call, s.name) };
                    ui.label(RichText::new(format!("{who} → {b:.0}°  ·  {}", self.dist(d))).size(12.5).color(p.dim))
                }
                (None, None) if crate::qrz::looks_like_callsign(first) && grid::to_latlon(&self.cfg.my_grid).is_some() => {
                    ui.label(RichText::new(format!("Press Go to look up {} on QRZ.com", first.to_uppercase())).size(12.5).color(p.dim))
                }
                (Some(b), _) => ui.label(RichText::new(format!("Bearing {b:.0}°")).size(12.5).color(p.dim)),
                (None, Some((b, d))) => ui.label(RichText::new(format!("{} → {b:.0}°  ·  {}", first.to_uppercase(), self.dist(d))).size(12.5).color(p.dim)),
                (None, None) if first.is_empty() => ui.label(RichText::new("Click the dial, a preset, or type a destination").size(12.5).color(p.dim)),
                (None, None) if grid::to_latlon(&self.cfg.my_grid).is_none() => ui.label(RichText::new("Set your own grid square in Settings → Location").size(12.5).color(p.amber)),
                (None, None) => ui.label(RichText::new("Not a grid square or bearing").size(12.5).color(p.red)),
            };
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                toggle(ui, &mut self.cfg.long_path, &p);
                ui.label(RichText::new("Long path").size(12.5).color(p.dim));
            });
        });
    }

    // ------------------------------------------------------------------ settings window

    fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let before = serde_json::to_string(&self.cfg).unwrap_or_default();
        let mut open = true;
        let mut builder = egui::ViewportBuilder::default()
            .with_title("Rotor Controller – Settings")
            .with_inner_size([820.0, 620.0])
            .with_min_inner_size([700.0, 480.0]);
        if let Some(pos) = self.settings_pos {
            builder = builder.with_position(pos);
        }
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("settings_viewport"), builder, |ui, _class| {
            let p = theme::pal(ui.visuals());
            if ui.ctx().input(|i| i.viewport().close_requested() || i.key_pressed(egui::Key::Escape)) {
                open = false;
            }
            egui::Panel::left("settings_nav")
                .resizable(false)
                .exact_size(184.0)
                .frame(egui::Frame::new().fill(p.card).inner_margin(egui::Margin::symmetric(12, 16)).stroke(Stroke::new(1.0, p.stroke)))
                .show(ui, |ui| {
                    ui.label(RichText::new("Settings").font(theme::bold(20.0)));
                    ui.add_space(8.0);
                    for tab in SettingsTab::ALL {
                        let sel = self.settings_tab == tab;
                        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::click());
                        let bg = if sel {
                            p.accent.gamma_multiply(if p.dark { 0.22 } else { 0.14 })
                        } else if resp.hovered() {
                            p.card_hi
                        } else {
                            Color32::TRANSPARENT
                        };
                        ui.painter().rect_filled(rect, 9.0, bg);
                        if sel {
                            ui.painter().rect_filled(egui::Rect::from_min_size(rect.left_top() + vec2(0.0, 8.0), vec2(3.5, rect.height() - 16.0)), 2.0, p.accent);
                        }
                        let col = if sel { p.accent } else { p.text };
                        ui.painter().text(rect.left_center() + vec2(16.0, 0.0), Align2::LEFT_CENTER, tab.label(), if sel { theme::bold(14.5) } else { FontId::proportional(14.5) }, col);
                        if resp.clicked() {
                            self.settings_tab = tab;
                        }
                    }
                    ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                        ui.label(RichText::new(format!("Rotor Controller {}", env!("CARGO_PKG_VERSION"))).size(11.5).color(p.dim));
                    });
                });
            egui::CentralPanel::default().frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin::symmetric(24, 20))).show(ui, |ui| {
                ui.label(RichText::new(self.settings_tab.label()).font(theme::bold(24.0)));
                ui.label(RichText::new(self.settings_tab.blurb()).size(13.0).color(p.dim));
                ui.add_space(8.0);
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.set_max_width(ui.available_width());
                    self.settings_page(ui, &p);
                    ui.add_space(12.0);
                });
            });
        });
        self.show_settings = open;
        if serde_json::to_string(&self.cfg).unwrap_or_default() != before {
            self.cfg.save();
        }
    }

    fn settings_page(&mut self, ui: &mut egui::Ui, p: &theme::Pal) {
        match self.settings_tab {
            SettingsTab::Connection => self.page_connection(ui, p),
            SettingsTab::Rotor => self.page_rotor(ui, p),
            SettingsTab::Location => {
                card(ui, p, "Your station", |ui| {
                    form(ui, "loc", |ui| {
                        field(ui, "My grid square", |ui| {
                            ui.add(egui::TextEdit::singleline(&mut self.cfg.my_grid).desired_width(110.0).char_limit(8).hint_text("e.g. FN31pr"));
                            match grid::to_latlon(&self.cfg.my_grid) {
                                Some((lat, lon)) => ui.label(RichText::new(format!("{lat:.3}°, {lon:.3}°")).color(p.green)),
                                None if self.cfg.my_grid.trim().is_empty() => ui.label(RichText::new("Required for the preset buttons").color(p.amber)),
                                None => ui.label(RichText::new("Not a valid locator").color(p.red)),
                            };
                        });
                        field(ui, "Long path", |ui| {
                            toggle(ui, &mut self.cfg.long_path, p);
                            ui.label(RichText::new("Turn the other way round the globe").color(p.dim));
                        });
                    });
                });
                card(ui, p, "QRZ.com callsign lookup", |ui| {
                    ui.label(RichText::new("Type a callsign in the Go box and the rotor turns toward its QRZ.com location. Needs a QRZ.com account with the XML data subscription.").color(p.dim));
                    ui.add_space(6.0);
                    form(ui, "qrz", |ui| {
                        field(ui, "Username", |ui| {
                            ui.add(egui::TextEdit::singleline(&mut self.cfg.qrz_user).desired_width(200.0).hint_text("your callsign"));
                        });
                        field(ui, "Password", |ui| {
                            ui.add(egui::TextEdit::singleline(&mut self.cfg.qrz_pass).password(true).desired_width(200.0));
                        });
                        field(ui, "Check", |ui| {
                            let busy = self.qrz_rx.is_some();
                            if ui.add_enabled(!busy && !self.cfg.qrz_user.trim().is_empty() && !self.cfg.qrz_pass.is_empty(), egui::Button::new("Test login")).clicked() {
                                let ctx = ui.ctx().clone();
                                self.start_qrz(&ctx, None);
                            }
                            match &self.qrz_test {
                                _ if busy => ui.label(RichText::new("Checking…").color(p.dim)),
                                Some(Ok(())) => ui.label(RichText::new("Login works").color(p.green)),
                                Some(Err(e)) => ui.label(RichText::new(e.as_str()).color(p.red)),
                                None => ui.label(""),
                            };
                        });
                    });
                    note(ui, p, p.dim, "The password is saved in the settings file as plain text; it is only sent to QRZ.com.");
                });
            }
            SettingsTab::Presets => {
                card(ui, p, "The 5 preset buttons", |ui| {
                    ui.label(RichText::new("Each button turns the antenna towards a grid square. You can also right-click any preset on the main window to edit it.").color(p.dim));
                    ui.add_space(6.0);
                    egui::Grid::new("preset_edit").num_columns(5).spacing([10.0, 8.0]).show(ui, |ui| {
                        for h in ["", "Label", "Grid square", "Result", ""] {
                            ui.label(RichText::new(h).size(12.0).color(p.dim));
                        }
                        ui.end_row();
                        for i in 0..self.cfg.presets.len() {
                            ui.label(RichText::new(format!("{}", i + 1)).font(theme::bold(14.0)).color(p.dim));
                            ui.add_sized([150.0, 28.0], egui::TextEdit::singleline(&mut self.cfg.presets[i].label).char_limit(14).margin(egui::Margin::symmetric(8, 5)));
                            ui.add_sized([110.0, 28.0], egui::TextEdit::singleline(&mut self.cfg.presets[i].grid).char_limit(8).margin(egui::Margin::symmetric(8, 5)));
                            match self.bearing_to(&self.cfg.presets[i].grid) {
                                Some((b, d)) => ui.label(RichText::new(format!("{b:.0}°  ·  {}", self.dist(d))).color(p.green)),
                                None if self.cfg.presets[i].grid.trim().is_empty() => ui.label(RichText::new("—").color(p.dim)),
                                None => ui.label(RichText::new("check grid").color(p.red)),
                            };
                            if ui.small_button("Clear").clicked() {
                                self.cfg.presets[i].label.clear();
                                self.cfg.presets[i].grid.clear();
                            }
                            ui.end_row();
                        }
                    });
                });
            }
            SettingsTab::Sharing => {
                card(ui, p, "rotctld server", |ui| {
                    form(ui, "share", |ui| {
                        field(ui, "Share this rotor", |ui| {
                            toggle(ui, &mut self.cfg.serve, p);
                            let (t, c) = if self.server.is_some() { (format!("Listening on port {}", self.cfg.serve_port), p.green) } else { ("Off".to_string(), p.dim) };
                            ui.label(RichText::new(t).color(c));
                        });
                        field(ui, "TCP port", |ui| {
                            ui.add(egui::DragValue::new(&mut self.cfg.serve_port).range(1..=65535));
                        });
                    });
                    ui.add_space(4.0);
                    ui.label(RichText::new("Other programs (loggers, trackers) can connect to this port using the rotctld protocol, e.g. Hamlib rotator model 2 at 127.0.0.1.").color(p.dim));
                });
            }
            SettingsTab::Calibrate => {
                if self.cfg.protocol == Proto::Gs232 {
                    self.calibrate_ui(ui, p);
                } else {
                    note(ui, p, p.dim, "Interface calibration is available for the Yaesu GS-232 protocol only.");
                }
            }
            SettingsTab::Appearance => {
                card(ui, p, "Look and feel", |ui| {
                    form(ui, "appearance", |ui| {
                        field(ui, "Theme", |ui| {
                            segmented(ui, p, &mut self.cfg.dark_mode, &[(true, "Dark"), (false, "Light")]);
                        });
                        field(ui, "Distance units", |ui| {
                            segmented(ui, p, &mut self.cfg.miles, &[(false, "Kilometers"), (true, "Miles")]);
                        });
                        field(ui, "Always on top", |ui| {
                            toggle(ui, &mut self.cfg.always_on_top, p);
                        });
                    });
                });
            }
            SettingsTab::About => self.page_about(ui, p),
            SettingsTab::Log => {
                card(ui, p, "Rotor traffic and events", |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("Clear").clicked() {
                            self.log.clear();
                        }
                        ui.label(RichText::new("> sent to the rotor   < received").size(12.0).color(p.dim));
                    });
                    egui::Frame::new().fill(p.well).corner_radius(9).inner_margin(10).show(ui, |ui| {
                        egui::ScrollArea::vertical().stick_to_bottom(true).max_height(340.0).auto_shrink([false, true]).show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            if self.log.is_empty() {
                                ui.label(RichText::new("Nothing yet").color(p.dim));
                            }
                            for l in &self.log {
                                ui.label(RichText::new(l).monospace().size(12.5));
                            }
                        });
                    });
                });
            }
        }
    }

    fn page_about(&mut self, ui: &mut egui::Ui, p: &theme::Pal) {
        card(ui, p, "Rotor Controller", |ui| {
            ui.label(RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION"))).font(theme::bold(15.0)));
            ui.hyperlink_to("Source code", "https://github.com/danohman/rotor-controller");
        });
        card(ui, p, "Updates", |ui| {
            ui.horizontal_wrapped(|ui| {
                let checking = matches!(self.update, UpdateStatus::Checking);
                if ui.add_enabled(!checking, egui::Button::new("Check for new version")).clicked() {
                    self.start_update_check(ui.ctx());
                }
                match &self.update {
                    UpdateStatus::Idle => {}
                    UpdateStatus::Checking => {
                        ui.add(egui::Spinner::new());
                        ui.label(RichText::new("Checking…").color(p.dim));
                    }
                    UpdateStatus::UpToDate => {
                        ui.label(RichText::new("You are running the latest version.").color(p.green));
                    }
                    UpdateStatus::Available { version, .. } => {
                        ui.label(RichText::new(format!("Version {version} is available.")).color(p.amber).strong());
                    }
                    UpdateStatus::Failed(why) => {
                        ui.label(RichText::new(format!("Check failed: {why}")).color(p.red));
                    }
                }
            });
            if let UpdateStatus::Available { url, .. } = &self.update {
                ui.hyperlink_to("Open the release page", url.as_str());
            }
            let can_upgrade = cfg!(windows) && self.install_rx.is_none() && matches!(&self.update, UpdateStatus::Available { asset: Some(_), .. });
            if can_upgrade && ui.add(primary_button("Upgrade now", p)).on_hover_text("Download the new version and restart").clicked() {
                self.start_install(ui.ctx());
            }
            if let Some(e) = &self.update_error {
                ui.label(RichText::new(format!("Update failed: {e}")).color(p.red));
            }
            ui.horizontal(|ui| {
                toggle(ui, &mut self.cfg.check_updates, p);
                ui.label("Check for a new version when the program starts");
            });
            ui.label(RichText::new("Contacts GitHub (api.github.com) only; no personal information is sent.").size(12.0).color(p.dim));
        });
        card(ui, p, "Copyright and license", |ui| {
            ui.label(RichText::new("Copyright © 2026 Dan Muntz (WD4DAN)").font(theme::bold(14.0)));
            ui.add_space(2.0);
            ui.label("This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.");
            ui.add_space(4.0);
            ui.label("This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.");
            ui.hyperlink_to("GNU General Public License, version 3", "https://www.gnu.org/licenses/gpl-3.0.html");
            ui.hyperlink_to("Source code and LICENSE file", "https://github.com/danohman/rotor-controller");
        });
    }

    fn page_connection(&mut self, ui: &mut egui::Ui, p: &theme::Pal) {
        card(ui, p, "Rotor interface", |ui| {
            form(ui, "conn", |ui| {
                field(ui, "Protocol", |ui| {
                    let cur = self.cfg.protocol;
                    egui::ComboBox::from_id_salt("proto").selected_text(cur.label()).width(380.0).show_ui(ui, |ui| {
                        for pr in Proto::ALL {
                            if ui.selectable_label(cur == pr, pr.label()).clicked() && cur != pr {
                                self.cfg.protocol = pr;
                                if pr.default_baud() != 0 {
                                    self.cfg.baud = pr.default_baud();
                                }
                                if !pr.elevation_capable() {
                                    self.cfg.elevation = false;
                                }
                            }
                        }
                    });
                });
                if self.cfg.protocol.is_network() {
                    field(ui, "rotctld host", |ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.cfg.host).desired_width(170.0));
                        ui.label(RichText::new("port").color(p.dim));
                        ui.add(egui::DragValue::new(&mut self.cfg.net_port).range(1..=65535));
                    });
                } else {
                    field(ui, "Serial port", |ui| {
                        let sel = if self.cfg.port.is_empty() { "Select port".to_string() } else { self.cfg.port.clone() };
                        egui::ComboBox::from_id_salt("s_port").selected_text(sel).width(150.0).show_ui(ui, |ui| {
                            for pt in self.ports.clone() {
                                ui.selectable_value(&mut self.cfg.port, pt.clone(), pt);
                            }
                        });
                        if ui.button("⟳").on_hover_text("Refresh the port list").clicked() {
                            self.ports = link::list_ports();
                        }
                    });
                    field(ui, "Baud rate", |ui| {
                        egui::ComboBox::from_id_salt("baud").selected_text(self.cfg.baud.to_string()).width(110.0).show_ui(ui, |ui| {
                            for b in BAUDS {
                                ui.selectable_value(&mut self.cfg.baud, b, b.to_string());
                            }
                        });
                    });
                }
                field(ui, "Auto-connect", |ui| {
                    toggle(ui, &mut self.cfg.auto_connect, p);
                    ui.label(RichText::new("Connect at start-up and reconnect if the link drops").color(p.dim));
                });
            });
            if !self.cfg.protocol.verified() {
                note(ui, p, p.amber, "This protocol was built from its published description and has not been tried on real hardware.");
            }
        });
        card(ui, p, "Connection", |ui| {
            ui.horizontal_wrapped(|ui| {
                let (text, kind) = self.status_text();
                status_pill(ui, p, &text, kind);
                let ready = self.cfg.protocol.is_network() || !self.cfg.port.is_empty();
                if self.connected.is_some() {
                    if ui.button("Disconnect").clicked() {
                        self.want_link = false;
                        let _ = self.tx.send(Cmd::Disconnect);
                    }
                } else if ui.add_enabled(ready, primary_button("Connect now", p)).clicked() {
                    self.want_link = true;
                    self.connect();
                }
            });
        });
    }

    fn page_rotor(&mut self, ui: &mut egui::Ui, p: &theme::Pal) {
        card(ui, p, "Azimuth", |ui| {
            form(ui, "rotor", |ui| {
                field(ui, "Rotor travel", |ui| {
                    segmented(ui, p, &mut self.cfg.max_travel, &[(360, "360°"), (450, "450°")]);
                    ui.label(RichText::new("450° rotors can overlap past north").color(p.dim));
                });
                field(ui, "Heading offset", |ui| {
                    ui.add(egui::Slider::new(&mut self.cfg.offset, -180..=180).suffix("°"));
                    if ui.small_button("Reset").clicked() {
                        self.cfg.offset = 0;
                    }
                });
                if self.cfg.protocol == Proto::Gs232 {
                    field(ui, "Speed", |ui| {
                        let before = self.cfg.speed;
                        segmented(ui, p, &mut self.cfg.speed, &[(0, "Default"), (1, "1"), (2, "2"), (3, "3"), (4, "4")]);
                        if self.cfg.speed != before {
                            self.send_speed();
                        }
                        ui.label(RichText::new("X1 slowest to X4 fastest; not all rotors respond").color(p.dim));
                    });
                }
                field(ui, "Span scale", |ui| {
                    ui.add(egui::DragValue::new(&mut self.cfg.span_scale).range(0.5..=2.0).speed(0.001).fixed_decimals(4));
                    if ui.small_button("Reset").clicked() {
                        self.cfg.span_scale = 1.0;
                    }
                    ui.label(RichText::new("Set it on the Calibrate tab").color(p.dim));
                });
            });
            ui.add_space(2.0);
            ui.label(RichText::new("Offset is added to every command and removed from the displayed heading. A rotor that starts at south needs 180°.").size(12.5).color(p.dim));
        });
        card(ui, p, "Elevation", |ui| {
            form(ui, "rotor_el", |ui| {
                field(ui, "Elevation axis", |ui| {
                    let capable = self.cfg.protocol.elevation_capable();
                    ui.add_enabled_ui(capable, |ui| toggle(ui, &mut self.cfg.elevation, p));
                    if !capable {
                        ui.label(RichText::new("Not available with this protocol").color(p.dim));
                    }
                });
                if self.cfg.has_elevation() {
                    field(ui, "Elevation range", |ui| {
                        segmented(ui, p, &mut self.cfg.el_max, &[(90, "0–90°"), (180, "0–180°")]);
                    });
                }
            });
        });
    }

    /// Guided GS-232B zero-point / full-scale calibration. Nothing is stored without a second click.
    fn calibrate_ui(&mut self, ui: &mut egui::Ui, p: &theme::Pal) {
        note(ui, p, p.amber, "Do the control box's own calibration first. Run the rotor to each end stop, then store that position in the interface. Nothing is stored without a confirm click.");
        card(ui, p, "Before you start", |ui| {
            ui.horizontal(|ui| {
                toggle(ui, &mut self.cal_ok, p);
                ui.label("I am at the rotor and the antenna has clear space");
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Interface reading").color(p.dim));
                ui.label(RichText::new(self.heading.map_or("---".to_string(), |h| format!("{h:.0}° (scaled)"))).font(theme::bold(16.0)));
            });
        });
        let enabled = self.cal_ok && self.connected.is_some();
        if !enabled {
            self.cal_armed = None;
        }
        let steps = [
            ("Step 1 · zero point", "Run fully counter-clockwise to the stop, then store zero.", Jog::Ccw, "Run CCW", "Store zero (O)", Cal::Zero),
            ("Step 2 · full scale", "Run fully clockwise to the stop, then store full scale.", Jog::Cw, "Run CW", "Store full scale (F)", Cal::Full),
        ];
        for (title, text, jog, run, store, cal) in steps {
            card(ui, p, title, |ui| {
                ui.label(RichText::new(text).color(p.dim));
                ui.horizontal_wrapped(|ui| {
                    if ui.add_enabled(enabled, egui::Button::new(run)).clicked() {
                        self.cal_armed = None;
                        let _ = self.tx.send(Cmd::Jog(jog));
                    }
                    if ui.add_enabled(self.connected.is_some(), egui::Button::new("Stop")).clicked() {
                        self.cal_armed = None;
                        let _ = self.tx.send(Cmd::Stop);
                    }
                    if self.cal_armed == Some(cal) {
                        if ui.add_enabled(enabled, egui::Button::new(RichText::new("Confirm – store now").strong().color(Color32::WHITE)).fill(p.red)).clicked() {
                            let _ = self.tx.send(Cmd::Calibrate(cal));
                            self.cal_armed = None;
                        }
                        if ui.button("Cancel").clicked() {
                            self.cal_armed = None;
                        }
                    } else if ui.add_enabled(enabled, primary_button(store, p)).on_hover_text("Only when the rotor is stopped at the end stop").clicked() {
                        self.cal_armed = Some(cal);
                    }
                });
            });
        }
        card(ui, p, "Software span correction", |ui| {
            ui.label(RichText::new("If the interface cannot be set to the full travel (its full scale reads too low), run the rotor fully clockwise, wait until it stops, then press the button. The app then multiplies the interface reading so full travel matches.").color(p.dim));
            ui.horizontal_wrapped(|ui| {
                let can = self.cal_ok && self.heading.is_some_and(|h| h > 50.0);
                if ui.add_enabled(can, primary_button("Set scale from clockwise stop", p)).clicked()
                    && let Some(h) = self.heading
                {
                    let raw = h / self.cfg.span_scale;
                    self.cfg.span_scale = (self.cfg.max_travel as f64 / raw * 10000.0).round() / 10000.0;
                }
                if ui.button("Reset scale").clicked() {
                    self.cfg.span_scale = 1.0;
                }
                ui.label(RichText::new(format!("Scale {:.4}", self.cfg.span_scale)).font(theme::bold(14.0)));
            });
        });
        card(ui, p, "Interface mode", |ui| {
            ui.label(RichText::new("Set the rotation range first (a G-800DXA turns 450°), then redo the two steps above. These only change interface settings; the rotor does not move. Commands P36, P45 and Z come from the GS-232B command set; check the Log for the interface's reply.").color(p.dim));
            let on = self.connected.is_some();
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(on, egui::Button::new("450° range (P45)")).clicked() {
                    let _ = self.tx.send(Cmd::Calibrate(Cal::Range450));
                }
                if ui.add_enabled(on, egui::Button::new("360° range (P36)")).clicked() {
                    let _ = self.tx.send(Cmd::Calibrate(Cal::Range360));
                }
                if ui.add_enabled(on, egui::Button::new("Toggle north/south start (Z)")).on_hover_text("Switches the interface between north-center and south-center; press again to switch back").clicked() {
                    let _ = self.tx.send(Cmd::Calibrate(Cal::ToggleStart));
                }
            });
        });
    }

    /// Remember where the main window is in the current mode (not while a mode switch is settling).
    fn track_window_pos(&mut self, ctx: &egui::Context) {
        if self.fit_until.is_some() || ctx.input(|i| i.pointer.any_down()) {
            return;
        }
        let Some(r) = ctx.input(|i| i.viewport().outer_rect) else { return };
        let pos = [r.min.x.round(), r.min.y.round()];
        let slot = if self.cfg.compact { &mut self.cfg.pos_compact } else { &mut self.cfg.pos_full };
        if *slot != Some(pos) {
            *slot = Some(pos);
            self.cfg.save();
        }
    }

    fn set_compact(&mut self, ctx: &egui::Context, compact: bool) {
        self.cfg.compact = compact;
        self.cfg.save();
        apply_window_mode(ctx, compact);
        // Go back to where this mode's window was last time.
        if let Some([x, y]) = if compact { self.cfg.pos_compact } else { self.cfg.pos_full } {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(Pos2::new(x, y)));
        }
        self.fit_until = Some(Instant::now() + Duration::from_millis(1500));
    }
}

fn apply_window_mode(ctx: &egui::Context, compact: bool) {
    let (size, min) = layout_sizes(compact);
    ctx.send_viewport_cmd(egui::ViewportCommand::Resizable(!compact));
    ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(min.into()));
    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size.into()));
}

// ---------------------------------------------------------------------- small widgets

#[derive(Clone, Copy)]
enum StatusKind {
    Ok,
    Demo,
    Busy,
    Off,
}

impl StatusKind {
    fn color(self, p: &theme::Pal) -> Color32 {
        match self {
            StatusKind::Ok => p.green,
            StatusKind::Demo => p.accent,
            StatusKind::Busy => p.amber,
            StatusKind::Off => p.red,
        }
    }
}

fn status_pill(ui: &mut egui::Ui, p: &theme::Pal, text: &str, kind: StatusKind) {
    egui::Frame::new().fill(p.card).stroke(Stroke::new(1.0, p.stroke)).corner_radius(99).inner_margin(egui::Margin::symmetric(12, 6)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 7.0;
            let (r, _) = ui.allocate_exact_size(vec2(10.0, 14.0), Sense::hover());
            ui.painter().circle_filled(r.center(), 4.5, kind.color(p));
            ui.label(RichText::new(text).size(13.0));
        });
    });
}

#[derive(Clone, Copy)]
enum Icon {
    Gear,
    Shrink,
    Expand,
}

/// Small square icon button; the icon is drawn with the painter so it looks the same everywhere.
fn icon_button(ui: &mut egui::Ui, icon: Icon, tip: &str) -> egui::Response {
    let p = theme::pal(ui.visuals());
    let (rect, resp) = ui.allocate_exact_size(vec2(32.0, 26.0), Sense::click());
    let fill = if resp.is_pointer_button_down_on() {
        p.accent.gamma_multiply(0.5)
    } else if resp.hovered() {
        p.card_hi.lerp_to_gamma(p.accent, 0.2)
    } else {
        p.card_hi
    };
    ui.painter().rect_filled(rect, 8.0, fill);
    ui.painter().rect_stroke(rect, 8.0, Stroke::new(1.0, if resp.hovered() { p.accent.gamma_multiply(0.7) } else { p.stroke }), egui::StrokeKind::Inside);
    let c = rect.center();
    let st = Stroke::new(1.6, p.text);
    match icon {
        Icon::Gear => {
            for k in 0..8 {
                let a = (k as f32) * std::f32::consts::FRAC_PI_4;
                let d = vec2(a.cos(), a.sin());
                ui.painter().line_segment([c + d * 5.2, c + d * 8.0], Stroke::new(2.6, p.text));
            }
            ui.painter().circle_stroke(c, 5.2, Stroke::new(2.0, p.text));
            ui.painter().circle_filled(c, 1.8, p.text);
        }
        Icon::Shrink => {
            ui.painter().rect_stroke(egui::Rect::from_center_size(c, vec2(15.0, 11.0)), 2.0, st, egui::StrokeKind::Middle);
            ui.painter().rect_filled(egui::Rect::from_min_size(c + vec2(0.0, -0.5), vec2(7.5, 5.5)), 1.0, p.text);
        }
        Icon::Expand => {
            ui.painter().rect_stroke(egui::Rect::from_center_size(c, vec2(15.0, 11.0)), 2.0, st, egui::StrokeKind::Middle);
            ui.painter().line_segment([c + vec2(-7.5, -2.5), c + vec2(7.5, -2.5)], st);
        }
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.on_hover_text(tip)
}

fn primary_button<'a>(text: impl Into<String>, p: &theme::Pal) -> egui::Button<'a> {
    egui::Button::new(RichText::new(text.into()).strong().color(Color32::WHITE)).fill(p.accent).stroke(Stroke::NONE)
}

fn pulse_value(ctx: &egui::Context) -> f32 {
    ctx.request_repaint_after(Duration::from_millis(33));
    ((ctx.input(|i| i.time) as f32 * 5.5).sin() + 1.0) / 2.0
}

/// Big press-and-hold button. `pulse` (0..1) animates it towards `hot` while the rotor moves.
#[derive(Clone, Copy)]
enum Dir {
    Left,
    Right,
    Up,
    Down,
}

#[allow(clippy::too_many_arguments)]
fn jog_button(ui: &mut egui::Ui, size: egui::Vec2, text: &str, dir: Option<Dir>, base: Color32, hot: Color32, pulse: Option<f32>, p: &theme::Pal) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let held = resp.is_pointer_button_down_on();
    let mut fill = base;
    if resp.hovered() {
        fill = fill.lerp_to_gamma(p.accent, 0.18);
    }
    let danger = hot == p.red;
    let mut text_col = if danger { Color32::WHITE } else { p.text };
    if let Some(k) = pulse {
        fill = fill.lerp_to_gamma(hot, 0.35 + 0.65 * k);
        text_col = if p.dark { Color32::BLACK } else { Color32::WHITE };
    }
    if held {
        fill = fill.lerp_to_gamma(Color32::BLACK, 0.15);
    }
    ui.painter().rect_filled(rect, 10.0, fill);
    ui.painter().rect_stroke(rect, 10.0, Stroke::new(1.0, p.stroke), egui::StrokeKind::Inside);
    let galley = ui.painter().layout_no_wrap(text.to_string(), theme::bold((size.y * 0.36).clamp(13.0, 17.0)), text_col);
    let s = (size.y * 0.17).clamp(5.0, 8.0);
    let gap = 8.0;
    let tw = galley.size().x;
    let arrow_w = if dir.is_some() { s * 1.6 } else { 0.0 };
    let (inline, total) = match dir {
        Some(Dir::Left | Dir::Right) if !text.is_empty() => (true, tw + gap + arrow_w),
        Some(_) => (false, arrow_w),
        None => (false, tw),
    };
    let mut x = rect.center().x - total / 2.0;
    let cy = rect.center().y;
    let tri = |painter: &egui::Painter, cx: f32, d: Dir| {
        let pts = match d {
            Dir::Left => vec![egui::pos2(cx - s * 0.8, cy), egui::pos2(cx + s * 0.8, cy - s), egui::pos2(cx + s * 0.8, cy + s)],
            Dir::Right => vec![egui::pos2(cx + s * 0.8, cy), egui::pos2(cx - s * 0.8, cy - s), egui::pos2(cx - s * 0.8, cy + s)],
            Dir::Up => vec![egui::pos2(cx, cy - s * 0.9), egui::pos2(cx - s, cy + s * 0.7), egui::pos2(cx + s, cy + s * 0.7)],
            Dir::Down => vec![egui::pos2(cx, cy + s * 0.9), egui::pos2(cx - s, cy - s * 0.7), egui::pos2(cx + s, cy - s * 0.7)],
        };
        painter.add(egui::Shape::convex_polygon(pts, text_col, Stroke::NONE));
    };
    match dir {
        Some(d @ Dir::Left) if inline => {
            tri(ui.painter(), x + arrow_w / 2.0, d);
            x += arrow_w + gap;
            ui.painter().galley(egui::pos2(x, cy - galley.size().y / 2.0), galley, text_col);
        }
        Some(d @ Dir::Right) if inline => {
            ui.painter().galley(egui::pos2(x, cy - galley.size().y / 2.0), galley, text_col);
            x += tw + gap;
            tri(ui.painter(), x + arrow_w / 2.0, d);
        }
        Some(d) => tri(ui.painter(), rect.center().x, d),
        None => {
            ui.painter().galley(egui::pos2(x, cy - galley.size().y / 2.0), galley, text_col);
        }
    }
    resp
}

#[allow(clippy::too_many_arguments)]
fn preset_button(ui: &mut egui::Ui, size: egui::Vec2, label: &str, sub: Option<String>, enabled: bool, active: bool, empty: bool, p: &theme::Pal) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let hovered = resp.hovered();
    let fill = if active {
        p.accent.gamma_multiply(if p.dark { 0.30 } else { 0.18 })
    } else if hovered && enabled {
        p.card_hi.lerp_to_gamma(p.accent, 0.15)
    } else {
        p.card
    };
    let stroke = if active { p.accent } else if hovered { p.accent.gamma_multiply(0.6) } else { p.stroke };
    ui.painter().rect_filled(rect, 10.0, fill);
    ui.painter().rect_stroke(rect, 10.0, Stroke::new(1.0, stroke), egui::StrokeKind::Inside);
    let col = if !enabled || empty { p.dim } else { p.text };
    let max_w = rect.width() - 8.0;
    let big = size.y < 40.0;
    let base = if big { 12.5 } else { 14.5 };
    // Shrink the label until it fits.
    let mut fs = base;
    let galley = loop {
        let g = ui.painter().layout_no_wrap(label.to_string(), theme::bold(fs), col);
        if g.size().x <= max_w || fs <= 8.0 {
            break g;
        }
        fs -= 0.5;
    };
    match sub {
        Some(s) => {
            let y = rect.center().y - 8.0;
            ui.painter().galley(egui::pos2(rect.center().x - galley.size().x / 2.0, y - galley.size().y / 2.0), galley, col);
            let sc = if enabled { p.dim } else { p.dim.gamma_multiply(0.7) };
            let mut sf = 11.5;
            let sg = loop {
                let g = ui.painter().layout_no_wrap(s.clone(), FontId::proportional(sf), sc);
                if g.size().x <= max_w || sf <= 8.0 {
                    break g;
                }
                sf -= 0.5;
            };
            ui.painter().galley(egui::pos2(rect.center().x - sg.size().x / 2.0, rect.center().y + 9.0 - sg.size().y / 2.0), sg, sc);
        }
        None => {
            ui.painter().galley(rect.center() - galley.size() / 2.0, galley, col);
        }
    }
    if hovered && enabled {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// iOS-style switch.
fn toggle(ui: &mut egui::Ui, on: &mut bool, p: &theme::Pal) -> egui::Response {
    let (rect, mut resp) = ui.allocate_exact_size(vec2(38.0, 22.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_responsive(resp.id, *on);
    let off = if p.dark { Color32::from_rgb(58, 65, 82) } else { Color32::from_rgb(190, 198, 212) };
    let fill = off.lerp_to_gamma(p.accent, t);
    ui.painter().rect_filled(rect, 11.0, fill);
    let x = egui::lerp(rect.left() + 11.0..=rect.right() - 11.0, t);
    ui.painter().circle_filled(egui::pos2(x, rect.center().y), 8.0, Color32::WHITE);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Pill-shaped choice between a few values.
fn segmented<T: PartialEq + Copy>(ui: &mut egui::Ui, p: &theme::Pal, value: &mut T, options: &[(T, &str)]) {
    egui::Frame::new().fill(p.well).corner_radius(10).inner_margin(3).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            for (v, label) in options {
                let sel = *value == *v;
                let text = RichText::new(*label).color(if sel { Color32::WHITE } else { p.text });
                let b = egui::Button::new(if sel { text.strong() } else { text }).fill(if sel { p.accent } else { Color32::TRANSPARENT }).stroke(Stroke::NONE).corner_radius(8);
                if ui.add(b).clicked() {
                    *value = *v;
                }
            }
        });
    });
}

fn card(ui: &mut egui::Ui, p: &theme::Pal, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new().fill(p.card).stroke(Stroke::new(1.0, p.stroke)).corner_radius(14).inner_margin(16).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(title).font(theme::bold(15.5)));
        ui.add_space(4.0);
        add(ui);
    });
    ui.add_space(4.0);
}

fn note(ui: &mut egui::Ui, p: &theme::Pal, color: Color32, text: &str) {
    egui::Frame::new().fill(color.gamma_multiply(if p.dark { 0.14 } else { 0.12 })).stroke(Stroke::new(1.0, color.gamma_multiply(0.5))).corner_radius(10).inner_margin(12).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(text));
    });
    ui.add_space(4.0);
}

/// Two-column label / control form.
fn form(ui: &mut egui::Ui, id: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::Grid::new(id).num_columns(2).min_col_width(140.0).spacing([20.0, 14.0]).show(ui, add);
}

fn field(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.label(RichText::new(label).font(theme::bold(14.0)));
    ui.horizontal_wrapped(add);
    ui.end_row();
}

// ---------------------------------------------------------------------- the window

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll();
        self.poll_update();
        self.poll_qrz();
        self.track_window_pos(&ctx);
        self.sync_server(&ctx);
        self.auto_reconnect(&ctx);
        if self.first_frame {
            self.first_frame = false;
            apply_window_mode(&ctx, self.cfg.compact);
            // `--demo`: start with the built-in simulated rotor (no hardware needed).
            if std::env::args().any(|a| a == "--demo") {
                self.want_link = false;
                let _ = self.tx.send(Cmd::Connect { transport: Transport::Demo, drv: self.driver_cfg() });
            }
        }
        // Only send window commands when they change: a title update on every position report
        // made Windows drop the mouse press, which cut jogging short.
        if self.sent_on_top != Some(self.cfg.always_on_top) {
            self.sent_on_top = Some(self.cfg.always_on_top);
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(if self.cfg.always_on_top {
                egui::viewport::WindowLevel::AlwaysOnTop
            } else {
                egui::viewport::WindowLevel::Normal
            }));
        }
        let title = self.title();
        if self.sent_title != title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.sent_title = title;
        }
        if self.applied_dark != Some(self.cfg.dark_mode) {
            crate::theme::apply(&ctx, self.cfg.dark_mode);
            self.applied_dark = Some(self.cfg.dark_mode);
        }
        let before = serde_json::to_string(&self.cfg).unwrap_or_default();
        if self.show_settings && !self.was_settings {
            // Just opened: put it beside the main window (the clamp below keeps it on the monitor).
            self.settings_pos = ctx.input(|i| i.viewport().outer_rect).map(|r| Pos2::new(r.right() + 12.0, r.top()));
            self.fit_until = Some(Instant::now() + Duration::from_millis(1500));
        }
        self.was_settings = self.show_settings;
        if let Some(t) = self.fit_until {
            if Instant::now() < t {
                crate::winfit::fit_foreground();
                ctx.request_repaint_after(Duration::from_millis(100));
            } else {
                self.fit_until = None;
            }
        }
        self.settings_window(&ctx);
        self.preset_window(&ctx);
        let compact = self.cfg.compact;
        let p = theme::pal(&ctx.global_style().visuals);
        let margin = if compact { egui::Margin::symmetric(8, 6) } else { egui::Margin::symmetric(20, 14) };
        egui::CentralPanel::default().frame(egui::Frame::new().fill(p.bg).inner_margin(margin)).show(ui, |ui| {
            if compact {
                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                let top_h = ui.available_height() - 30.0 - 4.0;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let size = top_h.min(ui.available_width() - 96.0);
                    self.dial(ui, size);
                    self.compact_side(ui, &ctx);
                });
                self.presets(ui, true);
            } else {
                self.top_bar(ui, &ctx);
                ui.add_space(6.0);
                let avail = ui.available_height();
                let size = (ui.available_width() - 40.0).min(avail - 270.0).clamp(220.0, 360.0);
                ui.vertical_centered(|ui| self.dial(ui, size));
                if self.cfg.has_elevation() {
                    self.elevation_bar(ui);
                }
                ui.add_space(2.0);
                self.controls_row(ui, 48.0, false);
                self.manual_row(ui);
                ui.add_space(4.0);
                if grid::to_latlon(&self.cfg.my_grid).is_none() {
                    let mut open = false;
                    egui::Frame::new().fill(p.amber.gamma_multiply(0.14)).stroke(Stroke::new(1.0, p.amber.gamma_multiply(0.5))).corner_radius(10).inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Set your grid square so the presets know where to point.").size(13.0));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                open = ui.button("Set grid…").clicked();
                            });
                        });
                    });
                    if open {
                        self.settings_tab = SettingsTab::Location;
                        self.show_settings = true;
                    }
                    ui.add_space(2.0);
                }
                ui.label(RichText::new("PRESETS").size(11.5).color(p.dim));
                self.presets(ui, false);
            }
        });
        if serde_json::to_string(&self.cfg).unwrap_or_default() != before {
            self.cfg.save();
        }
    }
}

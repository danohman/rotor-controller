//! Rotor I/O worker thread: owns the transport (serial port, TCP socket or the built-in
//! simulator), polls the position through the selected protocol driver and executes commands
//! sent from the UI.

use crate::drivers::{self, Cal, Driver, DriverCfg, Jog, Pos, Proto, Reply};
use crossbeam_channel::{Receiver, Sender, unbounded};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

pub enum Transport {
    Serial { port: String, baud: u32 },
    Tcp { addr: String },
    Demo,
}

pub enum Cmd {
    Connect { transport: Transport, drv: DriverCfg },
    Disconnect,
    GoTo { az: f64, el: Option<f64> },
    Stop,
    Jog(Jog),
    Calibrate(Cal),
}

pub enum Event {
    Position(Pos),
    Connected(Option<String>),
    Log(String),
}

trait Port: Send {
    fn write(&mut self, data: &[u8]) -> std::io::Result<()>;
    fn read(&mut self) -> std::io::Result<Vec<u8>>;
}

struct Serial(Box<dyn serialport::SerialPort>);

impl Port for Serial {
    fn write(&mut self, data: &[u8]) -> std::io::Result<()> {
        self.0.write_all(data)?;
        self.0.flush()
    }
    fn read(&mut self) -> std::io::Result<Vec<u8>> {
        let n = self.0.bytes_to_read().map_err(std::io::Error::other)? as usize;
        let mut buf = vec![0; n];
        if n > 0 {
            self.0.read_exact(&mut buf)?;
        }
        Ok(buf)
    }
}

struct Tcp(TcpStream);

impl Port for Tcp {
    fn write(&mut self, data: &[u8]) -> std::io::Result<()> {
        self.0.write_all(data)
    }
    fn read(&mut self) -> std::io::Result<Vec<u8>> {
        let mut buf = [0u8; 512];
        match self.0.read(&mut buf) {
            Ok(0) => Err(std::io::Error::new(std::io::ErrorKind::ConnectionAborted, "connection closed")),
            Ok(n) => Ok(buf[..n].to_vec()),
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }
}

/// Minimal GS-232 rotor simulator (3 degrees per second, azimuth and elevation).
struct Sim {
    az: f64,
    el: f64,
    target: Option<(f64, f64)>,
    dir: (f64, f64),
    last: Instant,
    rx: Vec<u8>,
    tx: Vec<u8>,
}

impl Sim {
    fn new() -> Self {
        Self { az: 180.0, el: 0.0, target: None, dir: (0.0, 0.0), last: Instant::now(), rx: vec![], tx: vec![] }
    }

    fn step(&mut self) {
        let d = 3.0 * self.last.elapsed().as_secs_f64();
        self.last = Instant::now();
        let approach = |cur: &mut f64, t: f64| {
            if (t - *cur).abs() <= d {
                *cur = t;
            } else {
                *cur += d * (t - *cur).signum();
            }
        };
        if let Some((ta, te)) = self.target {
            approach(&mut self.az, ta);
            approach(&mut self.el, te);
            if self.az == ta && self.el == te {
                self.target = None;
            }
        } else {
            self.az = (self.az + self.dir.0 * d).clamp(0.0, 450.0);
            self.el = (self.el + self.dir.1 * d).clamp(0.0, 180.0);
        }
    }
}

impl Port for Sim {
    fn write(&mut self, data: &[u8]) -> std::io::Result<()> {
        self.rx.extend_from_slice(data);
        while let Some(i) = self.rx.iter().position(|&b| b == b'\r') {
            let line: Vec<u8> = self.rx.drain(..=i).collect();
            let s = String::from_utf8_lossy(&line[..line.len() - 1]).to_string();
            self.step();
            let nums: Vec<f64> = s.get(1..).unwrap_or("").split_whitespace().filter_map(|t| t.parse().ok()).collect();
            match s.chars().next() {
                Some('C') if s.starts_with("C2") => {
                    self.tx.extend(format!("+0{:03}+0{:03}\r\n", self.az.round() as u16, self.el.round() as u16).bytes())
                }
                Some('C') => self.tx.extend(format!("+0{:03}\r\n", self.az.round() as u16).bytes()),
                Some('M') => {
                    self.target = nums.first().map(|&a| (a, self.el));
                    self.dir = (0.0, 0.0);
                }
                Some('W') if nums.len() == 2 => {
                    self.target = Some((nums[0], nums[1]));
                    self.dir = (0.0, 0.0);
                }
                Some('R') => (self.target, self.dir) = (None, (1.0, 0.0)),
                Some('L') => (self.target, self.dir) = (None, (-1.0, 0.0)),
                Some('U') => (self.target, self.dir) = (None, (0.0, 1.0)),
                Some('D') => (self.target, self.dir) = (None, (0.0, -1.0)),
                Some('O') | Some('F') => {}
                Some('S') | Some('A') | Some('E') => (self.target, self.dir) = (None, (0.0, 0.0)),
                _ => self.tx.extend(b"?>\r\n"),
            }
        }
        Ok(())
    }

    fn read(&mut self) -> std::io::Result<Vec<u8>> {
        self.step();
        Ok(std::mem::take(&mut self.tx))
    }
}

/// List serial ports available on this machine.
pub fn list_ports() -> Vec<String> {
    let mut v: Vec<String> =
        serialport::available_ports().map(|p| p.into_iter().map(|i| i.port_name).collect()).unwrap_or_default();
    v.sort();
    v
}

pub fn spawn(ctx: eframe::egui::Context) -> (Sender<Cmd>, Receiver<Event>) {
    let (cmd_tx, cmd_rx) = unbounded::<Cmd>();
    let (ev_tx, ev_rx) = unbounded::<Event>();
    std::thread::spawn(move || worker(cmd_rx, ev_tx, ctx));
    (cmd_tx, ev_rx)
}

fn open(t: Transport) -> Result<(Box<dyn Port>, String), String> {
    match t {
        Transport::Serial { port, baud } => serialport::new(&port, baud)
            .timeout(Duration::from_millis(50))
            .open()
            .map(|p| (Box::new(Serial(p)) as Box<dyn Port>, format!("{port} at {baud} baud")))
            .map_err(|e| format!("Cannot open {port}: {e}")),
        Transport::Tcp { addr } => {
            let sa = addr
                .to_socket_addrs()
                .map_err(|e| format!("Cannot resolve {addr}: {e}"))?
                .next()
                .ok_or_else(|| format!("No address for {addr}"))?;
            let s = TcpStream::connect_timeout(&sa, Duration::from_secs(3)).map_err(|e| format!("Cannot connect to {addr}: {e}"))?;
            s.set_read_timeout(Some(Duration::from_millis(30))).ok();
            s.set_nodelay(true).ok();
            Ok((Box::new(Tcp(s)), addr))
        }
        Transport::Demo => Ok((Box::new(Sim::new()), "Demo rotor".into())),
    }
}

fn worker(cmds: Receiver<Cmd>, events: Sender<Event>, ctx: eframe::egui::Context) {
    let send = |e: Event| {
        let _ = events.send(e);
        ctx.request_repaint();
    };
    let mut link: Link = None;
    // While a jog button is held the jog command is repeated instead of polling, in case the
    // interface drops the movement when another command (the position query) arrives.
    let mut jogging: Option<Jog> = None;
    let mut last_poll = Instant::now() - Duration::from_secs(1);
    loop {
        match cmds.recv_timeout(Duration::from_millis(40)) {
            Ok(cmd) => {
                let mut out: Option<Vec<u8>> = None;
                match cmd {
                    Cmd::Connect { transport, drv } => {
                        let demo = matches!(transport, Transport::Demo);
                        // The demo rotor always speaks GS-232.
                        let drv = if demo { DriverCfg { proto: Proto::Gs232, ..drv } } else { drv };
                        match open(transport) {
                            Ok((p, name)) => {
                                link = Some((p, drivers::make(drv)));
                                send(Event::Connected(Some(name.clone())));
                                send(Event::Log(format!("Connected: {name} ({:?})", drv.proto)));
                            }
                            Err(e) => send(Event::Log(e)),
                        }
                    }
                    Cmd::Disconnect => {
                        link = None;
                        jogging = None;
                        send(Event::Connected(None));
                        send(Event::Log("Disconnected".into()));
                    }
                    Cmd::GoTo { az, el } => {
                        jogging = None;
                        out = link.as_mut().map(|(_, d)| d.goto(az, el))
                    }
                    Cmd::Stop => {
                        jogging = None;
                        out = link.as_mut().map(|(_, d)| d.stop())
                    }
                    Cmd::Jog(j) => {
                        jogging = Some(j);
                        out = link.as_mut().map(|(_, d)| d.jog(j))
                    }
                    Cmd::Calibrate(c) => {
                        out = link.as_mut().and_then(|(_, d)| d.calibrate(c));
                        if out.is_none() {
                            send(Event::Log("Calibration is not supported by this protocol".into()));
                        }
                    }
                }
                if let (Some((p, _)), Some(data)) = (link.as_mut(), out)
                    && !data.is_empty()
                {
                    send(Event::Log(format!("> {}", printable(&data))));
                    if let Err(e) = p.write(&data) {
                        fail(&mut link, &send, e);
                    }
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return,
            Err(_) => {}
        }
        let Some((p, d)) = link.as_mut() else {
            jogging = None; // never resume a jog on a new connection
            continue;
        };
        if last_poll.elapsed() >= Duration::from_millis(500) {
            last_poll = Instant::now();
            let bytes = match jogging {
                Some(j) => d.jog(j),
                None => d.poll(),
            };
            if let Err(e) = p.write(&bytes) {
                fail(&mut link, &send, e);
                continue;
            }
        }
        match p.read() {
            Ok(bytes) if !bytes.is_empty() => {
                for r in d.feed(&bytes) {
                    match r {
                        Reply::Pos(pos) => send(Event::Position(pos)),
                        Reply::Text(t) => send(Event::Log(format!("< {t}"))),
                    }
                }
            }
            Ok(_) => {}
            Err(e) => fail(&mut link, &send, e),
        }
    }
}

/// Render bytes for the log, escaping control characters.
fn printable(data: &[u8]) -> String {
    data.iter()
        .map(|&b| match b {
            b'\r' | b'\n' => String::new(),
            0x20..=0x7e => (b as char).to_string(),
            _ => format!("\\x{b:02x}"),
        })
        .collect()
}

type Link = Option<(Box<dyn Port>, Box<dyn Driver>)>;

fn fail(link: &mut Link, send: &impl Fn(Event), e: std::io::Error) {
    *link = None;
    send(Event::Log(format!("Link error: {e}")));
    send(Event::Connected(None));
}

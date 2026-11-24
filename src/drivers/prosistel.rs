//! Prosistel "D" rotator protocol: STX, axis id, command, CR.

use super::{Driver, DriverCfg, Jog, Lines, Pos, Reply, leading_num};

const STOP_ANGLE: u32 = 997;

pub struct Prosistel {
    cfg: DriverCfg,
    lines: Lines,
    az: Option<f64>,
    el: Option<f64>,
}

impl Prosistel {
    pub fn new(cfg: DriverCfg) -> Self {
        Self { cfg, lines: Lines::default(), az: None, el: None }
    }

    fn both(&self, f: impl Fn(char) -> String) -> Vec<u8> {
        let mut s = f('A');
        if self.cfg.elevation {
            s.push_str(&f('E'));
        }
        s.into_bytes()
    }
}

impl Driver for Prosistel {
    fn poll(&mut self) -> Vec<u8> {
        self.both(|id| format!("\u{2}{id}?\r"))
    }

    fn goto(&mut self, az: f64, el: Option<f64>) -> Vec<u8> {
        let el = el.unwrap_or(0.0);
        self.both(|id| format!("\u{2}{id}G{:.0}\r", if id == 'A' { az } else { el }))
    }

    fn stop(&mut self) -> Vec<u8> {
        self.both(|id| format!("\u{2}{id}G{STOP_ANGLE}\r"))
    }

    fn jog(&mut self, j: Jog) -> Vec<u8> {
        // No continuous-move command: run to the end of travel on the one axis.
        let (id, v) = match j {
            Jog::Cw => ('A', self.cfg.az_max),
            Jog::Ccw => ('A', 0.0),
            Jog::Up => ('E', self.cfg.el_max),
            Jog::Down => ('E', 0.0),
        };
        format!("\u{2}{id}G{v:.0}\r").into_bytes()
    }

    fn feed(&mut self, data: &[u8]) -> Vec<Reply> {
        let mut out = Vec::new();
        for line in self.lines.push(data) {
            // Position reply: STX A,?,290,R
            let body = line.trim_start_matches('\u{2}');
            let f: Vec<&str> = body.split(',').collect();
            if f.len() >= 3 && f[1] == "?" {
                let v = leading_num(f[2]);
                match (f[0], v) {
                    ("A", Some(v)) => self.az = Some(v),
                    ("E", Some(v)) => self.el = self.cfg.elevation.then_some(v),
                    _ => {
                        out.push(Reply::Text(line));
                        continue;
                    }
                }
                if let Some(az) = self.az {
                    out.push(Reply::Pos(Pos { az, el: self.el }));
                }
            } else if f.len() < 2 || f[1] != "G" {
                out.push(Reply::Text(line));
            }
        }
        out
    }
}

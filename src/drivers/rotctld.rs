//! Hamlib rotctld network protocol (default "short" text commands over TCP).

use super::{Driver, DriverCfg, Jog, Lines, Pos, Reply};

pub struct Rotctld {
    cfg: DriverCfg,
    lines: Lines,
    pending_az: Option<f64>,
}

impl Rotctld {
    pub fn new(cfg: DriverCfg) -> Self {
        Self { cfg, lines: Lines::default(), pending_az: None }
    }
}

impl Driver for Rotctld {
    fn poll(&mut self) -> Vec<u8> {
        b"p\n".to_vec()
    }

    fn goto(&mut self, az: f64, el: Option<f64>) -> Vec<u8> {
        format!("P {az:.2} {:.2}\n", el.unwrap_or(0.0)).into_bytes()
    }

    fn stop(&mut self) -> Vec<u8> {
        b"S\n".to_vec()
    }

    fn jog(&mut self, j: Jog) -> Vec<u8> {
        // Hamlib move directions: up 2, down 4, left 8, right 16.
        let dir = match j {
            Jog::Up => 2,
            Jog::Down => 4,
            Jog::Ccw => 8,
            Jog::Cw => 16,
        };
        format!("M {dir} 50\n").into_bytes()
    }

    fn feed(&mut self, data: &[u8]) -> Vec<Reply> {
        let mut out = Vec::new();
        for line in self.lines.push(data) {
            if let Some(code) = line.strip_prefix("RPRT") {
                self.pending_az = None;
                if code.trim() != "0" {
                    out.push(Reply::Text(format!("rotctld: {line}")));
                }
            } else if let Ok(v) = line.parse::<f64>() {
                match self.pending_az.take() {
                    None => self.pending_az = Some(v),
                    Some(az) => out.push(Reply::Pos(Pos { az, el: self.cfg.elevation.then_some(v) })),
                }
            } else {
                out.push(Reply::Text(line));
            }
        }
        out
    }
}

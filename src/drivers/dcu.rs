//! Hy-Gain DCU-1 / Rotor-EZ style azimuth protocol: `AP1nnn;AM1;` to move, `AI1;` to read.

use super::{Driver, DriverCfg, Jog, Pos, Reply};

pub struct Dcu1 {
    cfg: DriverCfg,
    buf: String,
}

impl Dcu1 {
    pub fn new(cfg: DriverCfg) -> Self {
        Self { cfg, buf: String::new() }
    }
}

impl Driver for Dcu1 {
    fn poll(&mut self) -> Vec<u8> {
        b"AI1;".to_vec()
    }

    fn goto(&mut self, az: f64, _el: Option<f64>) -> Vec<u8> {
        format!("AP1{:03.0};AM1;", az.clamp(0.0, 999.0)).into_bytes()
    }

    fn stop(&mut self) -> Vec<u8> {
        b"AS1;".to_vec()
    }

    fn jog(&mut self, j: Jog) -> Vec<u8> {
        // No continuous-move command: run to the end of travel.
        match j {
            Jog::Cw => self.goto(self.cfg.az_max, None),
            Jog::Ccw => self.goto(0.0, None),
            Jog::Up | Jog::Down => Vec::new(),
        }
    }

    /// Replies look like `;123` (optionally with a decimal and trailing CR).
    fn feed(&mut self, data: &[u8]) -> Vec<Reply> {
        self.buf.push_str(&String::from_utf8_lossy(data));
        let mut out = Vec::new();
        loop {
            let Some(i) = self.buf.find(';') else {
                self.buf.clear();
                break;
            };
            self.buf.drain(..i);
            let num: String = self.buf[1..].chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
            let complete = self.buf.len() > 1 + num.len() || num.len() >= 3;
            if num.len() < 3 && !complete {
                break; // wait for more digits
            }
            if num.len() < 3 {
                self.buf.drain(..1); // garbage after ';'
                continue;
            }
            self.buf.drain(..1 + num.len());
            if let Ok(az) = num.parse::<f64>() {
                out.push(Reply::Pos(Pos { az, el: None }));
            }
        }
        out
    }
}

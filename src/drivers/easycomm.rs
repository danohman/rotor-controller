//! Easycomm II rotator protocol.

use super::{Driver, DriverCfg, Jog, Lines, Pos, Reply, leading_num};

pub struct Easycomm {
    cfg: DriverCfg,
    lines: Lines,
    az: Option<f64>,
    el: Option<f64>,
}

impl Easycomm {
    pub fn new(cfg: DriverCfg) -> Self {
        Self { cfg, lines: Lines::default(), az: None, el: None }
    }
}

impl Driver for Easycomm {
    fn poll(&mut self) -> Vec<u8> {
        if self.cfg.elevation { b"AZ\nEL\n".to_vec() } else { b"AZ\n".to_vec() }
    }

    fn goto(&mut self, az: f64, el: Option<f64>) -> Vec<u8> {
        format!("AZ{az:.1} EL{:.1}\n", el.unwrap_or(0.0)).into_bytes()
    }

    fn stop(&mut self) -> Vec<u8> {
        b"SA SE \n".to_vec()
    }

    fn jog(&mut self, j: Jog) -> Vec<u8> {
        match j {
            Jog::Cw => b"MR\n".to_vec(),
            Jog::Ccw => b"ML\n".to_vec(),
            Jog::Up => b"MU\n".to_vec(),
            Jog::Down => b"MD\n".to_vec(),
        }
    }

    fn feed(&mut self, data: &[u8]) -> Vec<Reply> {
        let mut out = Vec::new();
        for line in self.lines.push(data) {
            let mut got = false;
            for tok in line.split_whitespace() {
                if let Some(v) = tok.strip_prefix("AZ").and_then(leading_num) {
                    self.az = Some(v);
                    got = true;
                } else if let Some(v) = tok.strip_prefix("EL").and_then(leading_num) {
                    self.el = self.cfg.elevation.then_some(v);
                    got = true;
                }
            }
            match (got, self.az) {
                (true, Some(az)) => out.push(Reply::Pos(Pos { az, el: self.el })),
                _ => out.push(Reply::Text(line)),
            }
        }
        out
    }
}

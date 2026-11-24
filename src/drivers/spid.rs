//! SPID Rot2Prog (and MD-01/02 in Rot2Prog mode): 13-byte commands, 12-byte replies.

use super::{Driver, DriverCfg, Jog, Pos, Reply};

pub struct Rot2Prog {
    cfg: DriverCfg,
    buf: Vec<u8>,
    /// Pulses per degree, learned from every status reply.
    ph: u8,
    pv: u8,
    last_az: f64,
    last_el: f64,
}

impl Rot2Prog {
    pub fn new(cfg: DriverCfg) -> Self {
        Self { cfg, buf: Vec::new(), ph: 10, pv: 10, last_az: 0.0, last_el: 0.0 }
    }

    fn command(&self, k: u8) -> Vec<u8> {
        let mut c = vec![0u8; 13];
        c[0] = 0x57;
        c[11] = k;
        c[12] = 0x20;
        c
    }
}

/// Write `v` as four ASCII digits.
fn digits(v: f64, out: &mut [u8]) {
    let n = v.round().max(0.0) as u32;
    for (i, d) in out.iter_mut().enumerate() {
        *d = b'0' + ((n / 10u32.pow((3 - i) as u32)) % 10) as u8;
    }
}

impl Driver for Rot2Prog {
    fn poll(&mut self) -> Vec<u8> {
        self.command(0x1F)
    }

    fn goto(&mut self, az: f64, el: Option<f64>) -> Vec<u8> {
        let el = el.unwrap_or(self.last_el);
        let mut c = self.command(0x2F);
        digits(self.ph as f64 * (360.0 + az), &mut c[1..5]);
        c[5] = self.ph;
        digits(self.pv as f64 * (360.0 + el), &mut c[6..10]);
        c[10] = self.pv;
        c
    }

    fn stop(&mut self) -> Vec<u8> {
        self.command(0x0F)
    }

    fn jog(&mut self, j: Jog) -> Vec<u8> {
        // No documented continuous-move command: run to the end of travel instead.
        match j {
            Jog::Cw => self.goto(self.cfg.az_max, None),
            Jog::Ccw => self.goto(0.0, None),
            Jog::Up => self.goto(self.last_az, Some(self.cfg.el_max)),
            Jog::Down => self.goto(self.last_az, Some(0.0)),
        }
    }

    fn feed(&mut self, data: &[u8]) -> Vec<Reply> {
        self.buf.extend_from_slice(data);
        let mut out = Vec::new();
        loop {
            match self.buf.iter().position(|&b| b == 0x57) {
                None => {
                    self.buf.clear();
                    break;
                }
                Some(i) if i > 0 => {
                    self.buf.drain(..i);
                }
                _ => {}
            }
            if self.buf.len() < 12 {
                break;
            }
            if self.buf[11] != 0x20 {
                self.buf.remove(0);
                continue;
            }
            let f: Vec<u8> = self.buf.drain(..12).collect();
            let v = |a: usize| f[a] as f64 * 100.0 + f[a + 1] as f64 * 10.0 + f[a + 2] as f64 + f[a + 3] as f64 / 10.0;
            if f[5] != 0 {
                self.ph = f[5];
            }
            if f[10] != 0 {
                self.pv = f[10];
            }
            let az = v(1) - 360.0;
            let el = v(6) - 360.0;
            self.last_az = az;
            self.last_el = el;
            out.push(Reply::Pos(Pos { az, el: self.cfg.elevation.then_some(el) }));
        }
        out
    }
}

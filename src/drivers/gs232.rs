//! Yaesu GS-232A/B rotator protocol (also G-5500 az/el).

use super::{Cal, Driver, DriverCfg, Jog, Lines, Pos, Reply, leading_num};

pub struct Gs232 {
    cfg: DriverCfg,
    lines: Lines,
}

impl Gs232 {
    pub fn new(cfg: DriverCfg) -> Self {
        Self { cfg, lines: Lines::default() }
    }
}

/// Parse `+0aaa`, `+0aaa+0eee`, or `AZ=aaa EL=eee`.
fn parse_line(line: &str) -> Option<Pos> {
    let s = line.trim();
    let up = s.to_ascii_uppercase();
    let (az, el) = if let Some(i) = up.find("AZ=") {
        let el = up.find("EL=").and_then(|j| leading_num(&up[j + 3..]));
        (leading_num(&up[i + 3..])?, el)
    } else if s.starts_with('+') {
        let mut it = s.split('+').filter(|t| !t.is_empty()).map(leading_num);
        (it.next()??, it.next().flatten())
    } else {
        return None;
    };
    (az <= 450.0 && el.is_none_or(|e| e <= 180.0)).then_some(Pos { az, el })
}

impl Driver for Gs232 {
    fn poll(&mut self) -> Vec<u8> {
        if self.cfg.elevation { b"C2\r".to_vec() } else { b"C\r".to_vec() }
    }

    fn goto(&mut self, az: f64, el: Option<f64>) -> Vec<u8> {
        let az = az.round().clamp(0.0, 450.0) as u16;
        if self.cfg.elevation {
            let el = el.unwrap_or(0.0).round().clamp(0.0, 180.0) as u16;
            format!("W{az:03} {el:03}\r").into_bytes()
        } else {
            format!("M{az:03}\r").into_bytes()
        }
    }

    fn stop(&mut self) -> Vec<u8> {
        b"S\r".to_vec()
    }

    fn jog(&mut self, j: Jog) -> Vec<u8> {
        match j {
            Jog::Cw => b"R\r".to_vec(),
            Jog::Ccw => b"L\r".to_vec(),
            Jog::Up => b"U\r".to_vec(),
            Jog::Down => b"D\r".to_vec(),
        }
    }

    fn calibrate(&mut self, c: Cal) -> Option<Vec<u8>> {
        Some(match c {
            Cal::Zero => b"O\x0d".to_vec(),
            Cal::Full => b"F\x0d".to_vec(),
            Cal::Range360 => b"P36\x0d".to_vec(),
            Cal::Range450 => b"P45\x0d".to_vec(),
            Cal::ToggleStart => b"Z\x0d".to_vec(),
            Cal::Speed(n) => format!("X{}\r", n.clamp(1, 4)).into_bytes(),
        })
    }

    fn feed(&mut self, data: &[u8]) -> Vec<Reply> {
        self.lines.push(data).into_iter().map(|l| parse_line(&l).map_or(Reply::Text(l), Reply::Pos)).collect()
    }
}

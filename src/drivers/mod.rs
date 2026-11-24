//! Rotor protocol drivers. Pure byte builders and stream parsers, no I/O.

use serde::{Deserialize, Serialize};

mod dcu;
mod easycomm;
mod gs232;
mod prosistel;
mod rotctld;
mod spid;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Proto {
    #[default]
    Gs232,
    Easycomm,
    Rot2Prog,
    Prosistel,
    Dcu1,
    Rotctld,
}

impl Proto {
    pub const ALL: [Proto; 6] = [Proto::Gs232, Proto::Easycomm, Proto::Rot2Prog, Proto::Prosistel, Proto::Dcu1, Proto::Rotctld];

    pub fn label(self) -> &'static str {
        match self {
            Proto::Gs232 => "Yaesu GS-232A/B (G-450/650/800/1000/2800/5500)",
            Proto::Easycomm => "Easycomm II",
            Proto::Rot2Prog => "SPID Rot2Prog / MD-01/02",
            Proto::Prosistel => "Prosistel D",
            Proto::Dcu1 => "Hy-Gain DCU-1 / Rotor-EZ",
            Proto::Rotctld => "rotctld (Hamlib network)",
        }
    }

    pub fn default_baud(self) -> u32 {
        match self {
            Proto::Gs232 | Proto::Prosistel => 9600,
            Proto::Easycomm => 19200,
            Proto::Rot2Prog => 600,
            Proto::Dcu1 => 4800,
            Proto::Rotctld => 0,
        }
    }

    /// Whether the protocol can carry an elevation axis.
    pub fn elevation_capable(self) -> bool {
        !matches!(self, Proto::Dcu1)
    }

    pub fn is_network(self) -> bool {
        self == Proto::Rotctld
    }

    /// Only the Yaesu GS-232 driver is aimed at the author's own hardware; the others are built
    /// from the published protocol descriptions and have not been tried on a real rotor.
    pub fn verified(self) -> bool {
        self == Proto::Gs232
    }
}

/// Rotor position in the rotor's own scale (before the heading offset is removed).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Pos {
    pub az: f64,
    pub el: Option<f64>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Jog {
    Cw,
    Ccw,
    Up,
    Down,
}

/// Interface calibration steps (GS-232B: `O` zero point, `F` full scale) and interface mode
/// settings (`P36`/`P45` rotation range, `Z` north/south start toggle).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cal {
    Zero,
    Full,
    Range360,
    Range450,
    ToggleStart,
}

pub enum Reply {
    Pos(Pos),
    Text(String),
}

#[derive(Clone, Copy)]
pub struct DriverCfg {
    pub proto: Proto,
    pub elevation: bool,
    pub az_max: f64,
    pub el_max: f64,
}

pub trait Driver: Send {
    /// Bytes that ask the rotor for its position.
    fn poll(&mut self) -> Vec<u8>;
    /// Bytes that turn the rotor to `az` (and `el`, if the rotor has elevation).
    fn goto(&mut self, az: f64, el: Option<f64>) -> Vec<u8>;
    fn stop(&mut self) -> Vec<u8>;
    /// Start continuous movement (drivers without a jog command run to the end of travel).
    fn jog(&mut self, j: Jog) -> Vec<u8>;
    /// Store a calibration point, if the protocol has one.
    fn calibrate(&mut self, _c: Cal) -> Option<Vec<u8>> {
        None
    }
    /// Feed received bytes; returns decoded replies.
    fn feed(&mut self, data: &[u8]) -> Vec<Reply>;
}

pub fn make(cfg: DriverCfg) -> Box<dyn Driver> {
    match cfg.proto {
        Proto::Gs232 => Box::new(gs232::Gs232::new(cfg)),
        Proto::Easycomm => Box::new(easycomm::Easycomm::new(cfg)),
        Proto::Rot2Prog => Box::new(spid::Rot2Prog::new(cfg)),
        Proto::Prosistel => Box::new(prosistel::Prosistel::new(cfg)),
        Proto::Dcu1 => Box::new(dcu::Dcu1::new(cfg)),
        Proto::Rotctld => Box::new(rotctld::Rotctld::new(cfg)),
    }
}

/// Choose the azimuth command value for a compass `bearing` given the rotor's `max` travel
/// (360 or 450) and the current heading: the reachable candidate nearest to `current`.
pub fn pick_target(bearing: f64, current: f64, max: u16) -> f64 {
    let b = bearing.rem_euclid(360.0);
    let mut best = b;
    for cand in [b, b + 360.0] {
        if cand <= max as f64 && (cand - current).abs() < (best - current).abs() {
            best = cand;
        }
    }
    best.round()
}

/// Splits a byte stream into text lines (ending in any of `\r`, `\n`).
#[derive(Default)]
pub(crate) struct Lines(String);

impl Lines {
    pub fn push(&mut self, data: &[u8]) -> Vec<String> {
        self.0.push_str(&String::from_utf8_lossy(data));
        let mut out = Vec::new();
        while let Some(i) = self.0.find(['\r', '\n']) {
            let line: String = self.0.drain(..=i).collect();
            let line = line.trim();
            if !line.is_empty() {
                out.push(line.to_string());
            }
        }
        if self.0.len() > 512 {
            self.0.clear();
        }
        out
    }
}

/// Parse the number at the start of `s` (digits and a decimal point).
pub(crate) fn leading_num(s: &str) -> Option<f64> {
    let n: String = s.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    n.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(proto: Proto, elevation: bool) -> DriverCfg {
        DriverCfg { proto, elevation, az_max: 450.0, el_max: 180.0 }
    }

    fn pos(r: Vec<Reply>) -> Vec<Pos> {
        r.into_iter().filter_map(|r| if let Reply::Pos(p) = r { Some(p) } else { None }).collect()
    }

    #[test]
    fn overlap_picks_nearest() {
        assert_eq!(pick_target(30.0, 400.0, 450), 390.0);
        assert_eq!(pick_target(30.0, 100.0, 450), 30.0);
        assert_eq!(pick_target(30.0, 400.0, 360), 30.0);
        assert_eq!(pick_target(200.0, 300.0, 450), 200.0);
    }

    #[test]
    fn gs232_commands_and_replies() {
        let mut d = make(cfg(Proto::Gs232, false));
        assert_eq!(d.goto(5.0, None), b"M005\r");
        assert_eq!(d.poll(), b"C\r");
        assert_eq!(d.stop(), b"S\r");
        assert_eq!(pos(d.feed(b"+0123\r\n")), vec![Pos { az: 123.0, el: None }]);
        let mut e = make(cfg(Proto::Gs232, true));
        assert_eq!(e.goto(270.0, Some(45.0)), b"W270 045\r");
        assert_eq!(e.poll(), b"C2\r");
        assert_eq!(e.jog(Jog::Up), b"U\r");
        assert_eq!(pos(e.feed(b"+0123+0045\r\n")), vec![Pos { az: 123.0, el: Some(45.0) }]);
        assert_eq!(pos(e.feed(b"AZ=270 EL=010\r")), vec![Pos { az: 270.0, el: Some(10.0) }]);
        assert!(pos(e.feed(b"?>\r")).is_empty());
        assert!(pos(e.feed(b"+0451\r")).is_empty());
    }

    #[test]
    fn gs232_handles_split_reads() {
        let mut d = make(cfg(Proto::Gs232, false));
        assert!(pos(d.feed(b"+01")).is_empty());
        assert_eq!(pos(d.feed(b"23\r\n")), vec![Pos { az: 123.0, el: None }]);
    }

    #[test]
    fn easycomm() {
        let mut d = make(cfg(Proto::Easycomm, true));
        assert_eq!(d.goto(123.4, Some(45.0)), b"AZ123.4 EL45.0\n");
        assert_eq!(d.poll(), b"AZ\nEL\n");
        assert_eq!(d.stop(), b"SA SE \n");
        assert_eq!(pos(d.feed(b"AZ100.5\nEL20.0\n")), vec![Pos { az: 100.5, el: None }, Pos { az: 100.5, el: Some(20.0) }]);
    }

    #[test]
    fn rot2prog_frames() {
        let mut d = make(cfg(Proto::Rot2Prog, true));
        // 'W', azimuth digits 5,6,0,0 (= 560.0 raw, i.e. 200.0), PH=10, elevation 3,6,5,0 (= 365.0 raw, i.e. 5.0), PV=10, ' '
        let frame = [0x57, 5, 6, 0, 0, 10, 3, 6, 5, 0, 10, 0x20];
        let r = pos(d.feed(&frame));
        assert_eq!(r, vec![Pos { az: 200.0, el: Some(5.0) }]);
        let set = d.goto(200.0, Some(5.0));
        assert_eq!(set.len(), 13);
        assert_eq!(&set[1..5], b"5600");
        assert_eq!(set[5], 10);
        assert_eq!(&set[6..10], b"3650");
        assert_eq!(set[11], 0x2F);
        assert_eq!(d.poll()[11], 0x1F);
        assert_eq!(d.stop()[11], 0x0F);
        // resync after garbage
        let mut junk = vec![1, 2, 3];
        junk.extend_from_slice(&frame);
        assert_eq!(pos(d.feed(&junk)).len(), 1);
        // elevation jog keeps the azimuth
        let up = d.jog(Jog::Up);
        assert_eq!(&up[1..5], b"5600");
    }

    #[test]
    fn prosistel() {
        let mut d = make(cfg(Proto::Prosistel, true));
        assert_eq!(d.goto(290.0, Some(10.0)), b"\x02AG290\r\x02EG10\r");
        assert_eq!(d.poll(), b"\x02A?\r\x02E?\r");
        assert_eq!(d.stop(), b"\x02AG997\r\x02EG997\r");
        assert_eq!(pos(d.feed(b"\x02A,?,290,R\r")), vec![Pos { az: 290.0, el: None }]);
        assert_eq!(pos(d.feed(b"\x02E,?,12,R\r")), vec![Pos { az: 290.0, el: Some(12.0) }]);
    }

    #[test]
    fn dcu1() {
        let mut d = make(cfg(Proto::Dcu1, false));
        assert_eq!(d.goto(45.0, None), b"AP1045;AM1;");
        assert_eq!(d.poll(), b"AI1;");
        assert_eq!(d.stop(), b"AS1;");
        assert!(pos(d.feed(b";0")).is_empty());
        assert_eq!(pos(d.feed(b"93\r")), vec![Pos { az: 93.0, el: None }]);
        assert_eq!(pos(d.feed(b";270")), vec![Pos { az: 270.0, el: None }]);
    }

    #[test]
    fn rotctld() {
        let mut d = make(cfg(Proto::Rotctld, true));
        assert_eq!(d.goto(10.0, Some(20.0)), b"P 10.00 20.00\n");
        assert_eq!(d.poll(), b"p\n");
        assert_eq!(d.jog(Jog::Cw), b"M 16 50\n");
        assert_eq!(pos(d.feed(b"123.5\n45.0\nRPRT 0\n")), vec![Pos { az: 123.5, el: Some(45.0) }]);
        let mut z = make(cfg(Proto::Rotctld, false));
        assert_eq!(pos(z.feed(b"90\n0\n")), vec![Pos { az: 90.0, el: None }]);
    }
}

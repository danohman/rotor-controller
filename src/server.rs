//! Optional rotctld-compatible TCP server so other programs (loggers, trackers) can steer the
//! rotor through this app. Supports the common short commands: `p`, `P`, `S`, `M`, `_`, `q` and
//! `\dump_state`, plus their long names.

use crate::drivers::Jog;
use crossbeam_channel::Sender;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A request from a network client, executed by the UI thread so offset and overlap rules apply.
pub enum Remote {
    Goto { az: f64, el: Option<f64> },
    Stop,
    Move(Jog),
}

/// Last known compass position (azimuth, elevation) published by the UI.
pub type Shared = Arc<Mutex<Option<(f64, f64)>>>;

pub struct Server {
    stop: Arc<AtomicBool>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

pub fn start(port: u16, shared: Shared, tx: Sender<Remote>, ctx: eframe::egui::Context, elevation: bool, max_az: u16) -> std::io::Result<Server> {
    let listener = TcpListener::bind(("0.0.0.0", port))?;
    listener.set_nonblocking(true)?;
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    std::thread::spawn(move || {
        while !flag.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((s, _)) => {
                    let (shared, tx, ctx, flag) = (shared.clone(), tx.clone(), ctx.clone(), flag.clone());
                    std::thread::spawn(move || client(s, shared, tx, ctx, flag, elevation, max_az));
                }
                Err(_) => std::thread::sleep(Duration::from_millis(100)),
            }
        }
    });
    Ok(Server { stop })
}

fn client(s: TcpStream, shared: Shared, tx: Sender<Remote>, ctx: eframe::egui::Context, stop: Arc<AtomicBool>, elevation: bool, max_az: u16) {
    s.set_nonblocking(false).ok();
    s.set_read_timeout(Some(Duration::from_millis(200))).ok();
    let Ok(mut w) = s.try_clone() else { return };
    let mut r = BufReader::new(s);
    let mut line = String::new();
    while !stop.load(Ordering::Relaxed) {
        match r.read_line(&mut line) {
            Ok(0) => return,
            Ok(_) => {}
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => continue,
            Err(_) => return,
        }
        let cmd = std::mem::take(&mut line);
        let Some(reply) = handle(cmd.trim(), &shared, &tx, elevation, max_az) else { return };
        ctx.request_repaint();
        if w.write_all(reply.as_bytes()).is_err() {
            return;
        }
    }
}

/// Returns the reply text, or None to close the connection.
fn handle(cmd: &str, shared: &Shared, tx: &Sender<Remote>, elevation: bool, max_az: u16) -> Option<String> {
    let mut it = cmd.split_whitespace();
    let name = it.next().unwrap_or("");
    let nums: Vec<f64> = it.filter_map(|t| t.parse().ok()).collect();
    let ok = "RPRT 0\n".to_string();
    Some(match name {
        "p" | "\\get_pos" => {
            let (az, el) = shared.lock().ok().and_then(|g| *g).unwrap_or((0.0, 0.0));
            format!("{az:.6}\n{el:.6}\n")
        }
        "P" | "\\set_pos" if !nums.is_empty() => {
            let el = if elevation { nums.get(1).copied() } else { None };
            let _ = tx.send(Remote::Goto { az: nums[0], el });
            ok
        }
        "S" | "\\stop" => {
            let _ = tx.send(Remote::Stop);
            ok
        }
        "M" | "\\move" if !nums.is_empty() => {
            let j = match nums[0] as i32 {
                2 => Jog::Up,
                4 => Jog::Down,
                8 => Jog::Ccw,
                16 => Jog::Cw,
                _ => return Some("RPRT -1\n".into()),
            };
            let _ = tx.send(Remote::Move(j));
            ok
        }
        "_" | "\\get_info" => "Rotor Controller\n".into(),
        "\\dump_state" => format!(
            "1\n1\nmin_az=0\nmax_az={max_az}\nmin_el=0\nmax_el=180\nsouth_zero=0\nrot_type={}\ndone\n",
            if elevation { "AzEl" } else { "Az" }
        ),
        "q" | "Q" | "\\quit" => return None,
        _ => "RPRT -1\n".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::unbounded;

    #[test]
    fn short_commands() {
        let (tx, rx) = unbounded();
        let shared: Shared = Arc::new(Mutex::new(Some((123.0, 45.0))));
        assert_eq!(handle("p", &shared, &tx, true, 450).unwrap(), "123.000000\n45.000000\n");
        assert_eq!(handle("P 90 10", &shared, &tx, true, 450).unwrap(), "RPRT 0\n");
        assert!(matches!(rx.try_recv(), Ok(Remote::Goto { az, el: Some(e) }) if az == 90.0 && e == 10.0));
        assert_eq!(handle("S", &shared, &tx, false, 450).unwrap(), "RPRT 0\n");
        assert!(matches!(rx.try_recv(), Ok(Remote::Stop)));
        assert_eq!(handle("bogus", &shared, &tx, false, 450).unwrap(), "RPRT -1\n");
        assert!(handle("q", &shared, &tx, false, 450).is_none());
        assert!(handle("\\dump_state", &shared, &tx, false, 360).unwrap().contains("max_az=360"));
    }
}

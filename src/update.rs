//! Check GitHub releases for a newer version of the program, and (on Windows) install it.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

const LATEST_URL: &str = "https://api.github.com/repos/danohman/rotor-controller/releases/latest";
/// Downloads are only accepted from this project's own releases.
const DOWNLOAD_PREFIX: &str = "https://github.com/danohman/rotor-controller/releases/download/";
/// Release asset that replaces the running exe (the stand-alone Windows build).
const WINDOWS_ASSET: &str = "rotor-controller.exe";

/// A downloadable release file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub url: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum UpdateStatus {
    #[default]
    Idle,
    Checking,
    UpToDate,
    /// `asset` is the file that can be installed in place (Windows only), if the release has one.
    Available { version: String, url: String, asset: Option<Asset> },
    Failed(String),
}

/// Split "v1.2.3-beta+meta" into numeric components and an optional pre-release tag.
fn parse_version(s: &str) -> (Vec<u64>, Option<String>) {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let s = s.split('+').next().unwrap_or(s);
    let (core, pre) = match s.split_once('-') {
        Some((c, p)) => (c, Some(p.to_string())),
        None => (s, None),
    };
    (core.split('.').map_while(|p| p.parse::<u64>().ok()).collect(), pre)
}

/// True when `latest` is a strictly newer version than `current`.
/// A final release is newer than a pre-release of the same number (1.0.0 > 1.0.0-beta).
pub fn is_newer(latest: &str, current: &str) -> bool {
    let ((a, pa), (b, pb)) = (parse_version(latest), parse_version(current));
    let n = a.len().max(b.len());
    for i in 0..n {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    match (pa, pb) {
        (None, Some(_)) => true,
        (Some(x), Some(y)) => x > y,
        _ => false,
    }
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(timeout)).build().into()
}

const USER_AGENT: &str = concat!("rotor-controller/", env!("CARGO_PKG_VERSION"));

fn fetch() -> Result<UpdateStatus, String> {
    let body = agent(Duration::from_secs(10))
        .get(LATEST_URL)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| match e {
            // No release published yet.
            ureq::Error::StatusCode(404) => "no releases published yet".to_string(),
            e => e.to_string(),
        })?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let tag = v["tag_name"].as_str().ok_or("unexpected reply from GitHub")?;
    let url = v["html_url"].as_str().unwrap_or("https://github.com/danohman/rotor-controller/releases").to_string();
    if !is_newer(tag, env!("CARGO_PKG_VERSION")) {
        return Ok(UpdateStatus::UpToDate);
    }
    let asset = v["assets"].as_array().and_then(|list| {
        list.iter().find(|a| a["name"].as_str() == Some(WINDOWS_ASSET)).and_then(|a| {
            Some(Asset { url: a["browser_download_url"].as_str()?.to_string(), size: a["size"].as_u64().unwrap_or(0) })
        })
    });
    Ok(UpdateStatus::Available { version: tag.trim_start_matches(['v', 'V']).to_string(), url, asset })
}

/// Start a background check; the result arrives on the returned channel.
pub fn spawn_check(ctx: eframe::egui::Context) -> Receiver<UpdateStatus> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let st = fetch().unwrap_or_else(UpdateStatus::Failed);
        let _ = tx.send(st);
        ctx.request_repaint();
    });
    rx
}

fn sibling(exe: &std::path::Path, suffix: &str) -> PathBuf {
    let mut s = exe.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// Remove the previous exe left behind by an earlier in-place update.
pub fn cleanup_old() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(sibling(&exe, ".old"));
    }
}

/// Download `asset` and swap it in for the running exe. A running Windows exe can be renamed but not
/// overwritten, so the old file is moved aside (and deleted on the next start). Returns the path of
/// the exe to launch for the restart.
pub fn install(asset: &Asset) -> Result<PathBuf, String> {
    if !asset.url.starts_with(DOWNLOAD_PREFIX) {
        return Err("unexpected download address".into());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let (new, old) = (sibling(&exe, ".new"), sibling(&exe, ".old"));
    let result = (|| -> Result<(), String> {
        let mut resp = agent(Duration::from_secs(180))
            .get(&asset.url)
            .header("User-Agent", USER_AGENT)
            .call()
            .map_err(|e| e.to_string())?;
        let mut data = Vec::new();
        resp.body_mut().as_reader().read_to_end(&mut data).map_err(|e| e.to_string())?;
        if asset.size > 0 && data.len() as u64 != asset.size {
            return Err(format!("download incomplete ({} of {} bytes)", data.len(), asset.size));
        }
        if !data.starts_with(b"MZ") {
            return Err("downloaded file is not a Windows program".into());
        }
        let mut f = std::fs::File::create(&new).map_err(|e| format!("cannot write next to the program: {e}"))?;
        f.write_all(&data).and_then(|_| f.sync_all()).map_err(|e| e.to_string())?;
        drop(f);
        let _ = std::fs::remove_file(&old);
        std::fs::rename(&exe, &old).map_err(|e| e.to_string())?;
        if let Err(e) = std::fs::rename(&new, &exe) {
            let _ = std::fs::rename(&old, &exe);
            return Err(e.to_string());
        }
        Ok(())
    })();
    match result {
        Ok(()) => Ok(exe),
        Err(e) => {
            let _ = std::fs::remove_file(&new);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compare() {
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(is_newer("0.1.1", "0.1"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("0.0.9", "0.1.0"));
        // A final release supersedes its own beta, but not the other way round.
        assert!(is_newer("v1.0.0", "1.0.0-beta"));
        assert!(!is_newer("1.0.0-beta", "1.0.0"));
        assert!(!is_newer("v1.0.0-beta", "1.0.0-beta"));
        assert!(is_newer("1.0.0-beta.2", "1.0.0-beta.1"));
        assert!(is_newer("1.0.1-beta", "1.0.0"));
    }

    #[test]
    fn refuses_foreign_download_urls() {
        let a = Asset { url: "https://example.com/rotor-controller.exe".into(), size: 0 };
        assert!(install(&a).is_err());
    }
}

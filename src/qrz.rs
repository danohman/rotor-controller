//! Callsign lookup through the QRZ.com XML data service (needs the user's own QRZ.com login; the
//! location fields are only returned to accounts with an XML data subscription).

use std::sync::Mutex;
use std::time::Duration;

const URL: &str = "https://xmldata.qrz.com/xml/current/";
const AGENT: &str = concat!("rotor-controller-", env!("CARGO_PKG_VERSION"));

/// Where a looked-up station is.
#[derive(Clone, Debug)]
pub struct Station {
    pub call: String,
    pub name: String,
    pub lat: f64,
    pub lon: f64,
}

/// Session key of the last successful login, with the user it belongs to.
static SESSION: Mutex<Option<(String, String)>> = Mutex::new(None);

/// True when `s` could be an amateur callsign (letters, digits and `/`, with at least one of each).
pub fn looks_like_callsign(s: &str) -> bool {
    let n = s.chars().count();
    (3..=12).contains(&n)
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '/')
        && s.chars().any(|c| c.is_ascii_digit())
        && s.chars().any(|c| c.is_ascii_alphabetic())
}

/// Text between `<tag>` and `</tag>`.
fn tag<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&format!("</{name}>"))? + start;
    Some(xml[start..end].trim())
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

fn fetch(params: &[(&str, &str)]) -> Result<String, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(10))).build().into();
    let mut req = agent.get(URL).query("agent", AGENT);
    for (k, v) in params {
        req = req.query(*k, *v);
    }
    req.call().map_err(|e| format!("QRZ.com: {e}"))?.body_mut().read_to_string().map_err(|e| format!("QRZ.com: {e}"))
}

fn login(user: &str, pass: &str) -> Result<String, String> {
    if user.trim().is_empty() || pass.is_empty() {
        return Err("Enter your QRZ.com username and password in Settings".into());
    }
    let xml = fetch(&[("username", user.trim()), ("password", pass)])?;
    if let Some(key) = tag(&xml, "Key") {
        return Ok(key.to_string());
    }
    Err(unescape(tag(&xml, "Error").unwrap_or("unexpected reply from QRZ.com")))
}

/// Check the login.
pub fn test(user: &str, pass: &str) -> Result<(), String> {
    let key = login(user, pass)?;
    *SESSION.lock().unwrap() = Some((user.trim().to_string(), key));
    Ok(())
}

pub fn lookup(user: &str, pass: &str, call: &str) -> Result<Station, String> {
    let call = call.trim().to_uppercase();
    for attempt in 0..2 {
        let cached = SESSION.lock().unwrap().clone().filter(|(u, _)| u == user.trim()).map(|(_, k)| k);
        let key = match cached {
            Some(k) => k,
            None => {
                let k = login(user, pass)?;
                *SESSION.lock().unwrap() = Some((user.trim().to_string(), k.clone()));
                k
            }
        };
        let xml = fetch(&[("s", &key), ("callsign", &call)])?;
        if let Some(err) = tag(&xml, "Error") {
            let err = unescape(err);
            // Expired session: log in again once.
            if attempt == 0 && (err.contains("Session Timeout") || err.contains("Invalid session")) {
                *SESSION.lock().unwrap() = None;
                continue;
            }
            return Err(err);
        }
        return parse_station(&xml, &call);
    }
    Err("QRZ.com session could not be renewed".into())
}

fn parse_station(xml: &str, call: &str) -> Result<Station, String> {
    let rec = tag(xml, "Callsign").ok_or("unexpected reply from QRZ.com")?;
    let num = |n: &str| tag(rec, n).and_then(|v| v.parse::<f64>().ok());
    let (lat, lon) = match (num("lat"), num("lon")) {
        (Some(a), Some(b)) => (a, b),
        _ => tag(rec, "grid")
            .and_then(crate::grid::to_latlon)
            .ok_or("QRZ.com returned no location for this callsign (the XML data subscription is needed)")?,
    };
    let name = [tag(rec, "fname"), tag(rec, "name")].into_iter().flatten().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
    Ok(Station { call: tag(rec, "call").unwrap_or(call).to_uppercase(), name: unescape(&name), lat, lon })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callsign_shapes() {
        assert!(looks_like_callsign("WD4DAN"));
        assert!(looks_like_callsign("K1AB"));
        assert!(looks_like_callsign("DL1ABC/P"));
        assert!(!looks_like_callsign("270"));
        assert!(!looks_like_callsign("WHAT"));
        assert!(!looks_like_callsign("A1"));
    }

    #[test]
    fn parses_a_record() {
        let xml = "<QRZDatabase><Callsign><call>W1AW</call><fname>Hiram</fname><name>Maxim &amp; Co</name><lat>41.714775</lat><lon>-72.727260</lon><grid>FN31pr</grid></Callsign><Session><Key>abc</Key></Session></QRZDatabase>";
        let s = parse_station(xml, "W1AW").unwrap();
        assert_eq!(s.call, "W1AW");
        assert_eq!(s.name, "Hiram Maxim & Co");
        assert!((s.lat - 41.714775).abs() < 1e-6 && (s.lon + 72.72726).abs() < 1e-6);
    }

    #[test]
    fn falls_back_to_grid_and_reports_missing_location() {
        let s = parse_station("<Callsign><call>W1AW</call><grid>FN31</grid></Callsign>", "W1AW").unwrap();
        assert!((s.lat - 41.5).abs() < 1e-9);
        assert!(parse_station("<Callsign><call>W1AW</call></Callsign>", "W1AW").is_err());
    }

    #[test]
    fn reads_session_key_and_error() {
        assert_eq!(tag("<Session><Key>k1</Key></Session>", "Key"), Some("k1"));
        assert_eq!(tag("<Session><Error>Not found: X</Error></Session>", "Error"), Some("Not found: X"));
    }
}

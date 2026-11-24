//! Maidenhead grid locators and great-circle bearing / distance.

const EARTH_RADIUS_KM: f64 = 6371.0;

/// Center of a 2-, 4-, 6- or 8-character locator as (latitude, longitude) in degrees.
pub fn to_latlon(grid: &str) -> Option<(f64, f64)> {
    let g: Vec<char> = grid.trim().chars().collect();
    if g.len() < 2 || g.len() > 8 || !g.len().is_multiple_of(2) {
        return None;
    }
    let letter = |c: char, max: u8| {
        let v = (c.to_ascii_uppercase() as u8).checked_sub(b'A')?;
        (v < max).then_some(v as f64)
    };
    let digit = |c: char| c.to_digit(10).map(|d| d as f64);
    let mut lon = -180.0 + letter(g[0], 18)? * 20.0;
    let mut lat = -90.0 + letter(g[1], 18)? * 10.0;
    let (mut w, mut h) = (20.0, 10.0);
    if g.len() >= 4 {
        lon += digit(g[2])? * 2.0;
        lat += digit(g[3])?;
        (w, h) = (2.0, 1.0);
    }
    if g.len() >= 6 {
        lon += letter(g[4], 24)? * (2.0 / 24.0);
        lat += letter(g[5], 24)? * (1.0 / 24.0);
        (w, h) = (2.0 / 24.0, 1.0 / 24.0);
    }
    if g.len() >= 8 {
        lon += digit(g[6])? * (2.0 / 240.0);
        lat += digit(g[7])? * (1.0 / 240.0);
        (w, h) = (2.0 / 240.0, 1.0 / 240.0);
    }
    Some((lat + h / 2.0, lon + w / 2.0))
}

/// Initial great-circle bearing (degrees, 0..360) and distance (km) from `a` to `b`.
pub fn bearing_distance(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    let (p1, p2) = (a.0.to_radians(), b.0.to_radians());
    let dl = (b.1 - a.1).to_radians();
    let y = dl.sin() * p2.cos();
    let x = p1.cos() * p2.sin() - p1.sin() * p2.cos() * dl.cos();
    let brg = y.atan2(x).to_degrees().rem_euclid(360.0);
    let h = ((p2 - p1) / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    let dist = 2.0 * EARTH_RADIUS_KM * h.sqrt().asin();
    (brg, dist)
}

/// Bearing and distance between two locators; `long_path` gives the opposite way round.
pub fn between(from: &str, to: &str, long_path: bool) -> Option<(f64, f64)> {
    let (brg, dist) = bearing_distance(to_latlon(from)?, to_latlon(to)?);
    Some(if long_path {
        ((brg + 180.0) % 360.0, 2.0 * std::f64::consts::PI * EARTH_RADIUS_KM - dist)
    } else {
        (brg, dist)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_of_known_squares() {
        let (lat, lon) = to_latlon("JN58td").unwrap();
        assert!((lat - 48.14).abs() < 0.02 && (lon - 11.60).abs() < 0.03, "{lat} {lon}");
        let (lat, lon) = to_latlon("FN31").unwrap();
        assert!((lat - 41.5).abs() < 1e-9 && (lon + 73.0).abs() < 1e-9);
    }

    #[test]
    fn rejects_bad_locators() {
        for g in ["", "J", "JN5", "ZZ11", "JN5x", "JN58tdaa1", "1234"] {
            assert!(to_latlon(g).is_none(), "{g}");
        }
    }

    #[test]
    fn bearing_due_east_and_north() {
        let (b, _) = bearing_distance((0.0, 0.0), (0.0, 10.0));
        assert!((b - 90.0).abs() < 1e-6);
        let (b, d) = bearing_distance((0.0, 0.0), (10.0, 0.0));
        assert!(b.abs() < 1e-6 && (d - 1111.9).abs() < 1.0);
    }

    #[test]
    fn long_path_is_opposite() {
        let (sp, sd) = between("FN31", "JN58", false).unwrap();
        let (lp, ld) = between("FN31", "JN58", true).unwrap();
        assert!(((lp - sp).rem_euclid(360.0) - 180.0).abs() < 1e-9);
        assert!((sd + ld - 40030.0).abs() < 50.0);
    }
}

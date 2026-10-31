use super::xml::Element;

const EARTH_KM: f64 = 6371.0088;

pub(super) struct Location {
    pub(super) lat: f64,
    pub(super) lon: f64,
    pub(super) radius_km: f64,
    pub(super) weather_feed: Option<String>,
    pub(super) advisory_country: String,
}

pub(super) fn distance(lat: f64, lon: f64, other_lat: f64, other_lon: f64) -> f64 {
    let (a, b) = (lat.to_radians(), other_lat.to_radians());
    let half = ((b - a) / 2.0).sin().powi(2) + a.cos() * b.cos() * ((other_lon - lon).to_radians() / 2.0).sin().powi(2);
    EARTH_KM * 2.0 * half.clamp(0.0, 1.0).sqrt().asin()
}

pub(super) fn nearby(location: &Location, lat: f64, lon: f64, radius: f64) -> Result<bool, String> {
    let finite = [lat, lon, radius].iter().all(|value| value.is_finite());
    if !finite || !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) || radius < 0.0 {
        return Err("invalid feed geometry".into());
    }
    Ok(distance(location.lat, location.lon, lat, lon) <= location.radius_km + radius)
}

fn number(text: &str) -> Result<f64, String> {
    text.trim().parse().map_err(|_| format!("invalid number {text:?}"))
}

fn bearing(from: (f64, f64), to: (f64, f64)) -> f64 {
    let (x, y, delta) = (from.0.to_radians(), to.0.to_radians(), (to.1 - from.1).to_radians());
    (delta.sin() * y.cos()).atan2(x.cos() * y.sin() - x.sin() * y.cos() * delta.cos())
}

fn near_segment(location: &Location, a: (f64, f64), b: (f64, f64)) -> Result<bool, String> {
    let segment = distance(a.0, a.1, b.0, b.1) / EARTH_KM;
    if segment == 0.0 {
        return nearby(location, a.0, a.1, 0.0);
    }
    let start = distance(a.0, a.1, location.lat, location.lon) / EARTH_KM;
    let angle = bearing(a, (location.lat, location.lon)) - bearing(a, b);
    let along = (start.sin() * angle.cos()).atan2(start.cos());
    let cross = (start.sin() * angle.sin()).clamp(-1.0, 1.0).asin();
    let closest = if (0.0..=segment).contains(&along) {
        cross.abs() * EARTH_KM
    } else {
        distance(location.lat, location.lon, a.0, a.1).min(distance(location.lat, location.lon, b.0, b.1))
    };
    Ok(closest <= location.radius_km)
}

pub(super) fn polygon_nearby(location: &Location, value: &str) -> Result<bool, String> {
    let mut points = Vec::new();
    for pair in value.split_whitespace() {
        let (lat, lon) = pair.split_once(',').ok_or("invalid feed polygon")?;
        points.push((number(lat)?, number(lon)?));
    }
    if points.len() < 4 || points.first() != points.last() {
        return Err("invalid feed polygon".into());
    }
    let relative: Vec<(f64, f64)> = points.iter().map(|(x, y)| ((y - location.lon + 180.0).rem_euclid(360.0) - 180.0, x - location.lat)).collect();
    let mut inside = false;
    for pair in relative.windows(2) {
        let ((ax, ay), (bx, by)) = (pair[0], pair[1]);
        if (ay > 0.0) != (by > 0.0) && 0.0 < ax + (bx - ax) * -ay / (by - ay) {
            inside = !inside;
        }
    }
    if inside {
        return Ok(true);
    }
    for pair in points.windows(2) {
        if near_segment(location, pair[0], pair[1])? {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn weather_nearby(entry: &Element, location: &Location) -> Result<bool, String> {
    for node in entry.walk() {
        let value = node.text.as_str();
        if value.trim().is_empty() {
            continue;
        }
        let near = match node.local() {
            "polygon" => polygon_nearby(location, value)?,
            "circle" => {
                let parts: Vec<&str> = value.split_whitespace().collect();
                let [center, radius] = parts[..] else { return Err("invalid feed circle".into()) };
                let (lat, lon) = center.split_once(',').ok_or("invalid feed circle")?;
                nearby(location, number(lat)?, number(lon)?, number(radius)?)?
            }
            "point" => {
                let parts: Vec<&str> = value.split_whitespace().collect();
                let [lat, lon] = parts[..] else { return Err("invalid feed point".into()) };
                nearby(location, number(lat)?, number(lon)?, 0.0)?
            }
            _ => false,
        };
        if near {
            return Ok(true);
        }
    }
    Ok(false)
}

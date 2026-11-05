//! The caveat printed under the flow rows: whether the landing watch is alive.
use super::host::Places;

pub fn watch_silent(places: &Places, now: f64, limit: f64) -> Option<Option<f64>> {
    let beat = std::fs::read_to_string(places.state.join("north/watch/heartbeat")).ok().and_then(|text| text.trim().parse::<f64>().ok());
    match beat {
        Some(at) if now - at <= limit => None,
        Some(at) => Some(Some(now - at)),
        None => Some(None),
    }
}

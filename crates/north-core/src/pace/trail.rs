use super::limits;
use super::window::Window;

pub(super) fn extended(baseline: Option<&Window>, window: &Window, at: f64, fallback_at: f64) -> Vec<[f64; 2]> {
    let mut trail = match baseline.filter(|old| old.resets_at == window.resets_at) {
        Some(old) if !old.trail.is_empty() => old.trail.clone(),
        Some(old) => vec![old.anchor.as_ref().map_or([fallback_at, old.used_pct], |anchor| [anchor.at, anchor.used])],
        None => Vec::new(),
    };
    trail.push([at, window.used_pct]);
    let horizon = limits::burn_window_seconds();
    let start = trail.iter().rposition(|point| at - point[0] >= horizon).unwrap_or(0);
    trail.split_off(start)
}

pub(super) fn burn(trail: &[[f64; 2]], used: f64, at: f64) -> Option<f64> {
    let [first_at, first_used] = *trail.first()?;
    (at - first_at >= limits::burn_window_seconds()).then(|| (used - first_used).max(0.0) * 3600.0 / (at - first_at))
}

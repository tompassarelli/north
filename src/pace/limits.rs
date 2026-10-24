use std::sync::LazyLock;

use toml_edit::DocumentMut;

static DEFAULTS: LazyLock<DocumentMut> = LazyLock::new(|| {
    include_str!("../../harness/defaults.toml")
        .parse()
        .expect("harness/defaults.toml parses")
});

pub(crate) fn default(name: &str) -> f64 {
    let value = &DEFAULTS[name]["value"];
    value
        .as_float()
        .or_else(|| value.as_integer().map(|whole| whole as f64))
        .unwrap_or_else(|| panic!("harness/defaults.toml has no number for {name}"))
}

pub(super) fn plan_stop() -> f64 {
    default("plan_stop")
}

pub(super) fn claude_target_left() -> f64 {
    default("pace_target_left_claude")
}

pub(super) fn five_hour_burn_cap() -> f64 {
    default("five_hour_burn_cap_pct_h")
}

pub(super) fn week_average_hours() -> f64 {
    default("week_projection_average_h")
}

pub(super) fn state_dwell_seconds() -> f64 {
    default("pace_state_dwell_min") * 60.0
}

pub(super) fn burn_window_seconds() -> f64 {
    default("pace_burn_window_min") * 60.0
}

pub(crate) fn reservation_ttl_seconds() -> f64 {
    default("spawn_reservation_ttl_s")
}
pub(crate) fn grant_hold_seconds() -> f64 {
    default("grant_reservation_hold_s")
}
pub(crate) fn placement_drain_margin() -> f64 {
    default("placement_drain_margin")
}

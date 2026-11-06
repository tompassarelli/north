//! The numbers replay judges a change against, read from harness/defaults.toml.
use toml_edit::DocumentMut;

pub struct Bounds {
    pub five_hour_floor_points: f64,
    pub week_floor_points: f64,
    pub landing_p90_seconds: f64,
    pub shift_ratio: f64,
    pub shift_min_cases: usize,
    pub budget_seconds: f64,
}

impl Bounds {
    pub fn from_defaults() -> Self {
        let defaults: DocumentMut = north_base::platform::harness::table(north_base::platform::harness::DEFAULTS).parse().expect("harness/defaults.toml parses");
        let number = |name: &str| {
            let value = &defaults[name]["value"];
            value.as_float().or_else(|| value.as_integer().map(|whole| whole as f64)).unwrap_or_else(|| panic!("harness/defaults.toml has no number for {name}"))
        };
        Self {
            five_hour_floor_points: number("replay_five_hour_floor_points"),
            week_floor_points: number("replay_week_floor_points"),
            landing_p90_seconds: number("replay_landing_p90_seconds"),
            shift_ratio: number("replay_outcome_shift_ratio"),
            shift_min_cases: number("replay_outcome_shift_min_cases") as usize,
            budget_seconds: number("replay_budget_seconds"),
        }
    }
}

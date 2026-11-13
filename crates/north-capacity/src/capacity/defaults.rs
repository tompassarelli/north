//! The numbers capacity reads from `harness/defaults.toml`, through one reader so every subject parses
//! a default the same way.

use std::sync::LazyLock;

use toml_edit::DocumentMut;

static DEFAULTS: LazyLock<&DocumentMut> = LazyLock::new(|| north_base::platform::harness::document(north_base::platform::harness::DEFAULTS));

pub fn default_mib(name: &str) -> f64 {
    let value = &DEFAULTS[name]["value"];
    value
        .as_float()
        .or_else(|| value.as_integer().map(|whole| whole as f64))
        .unwrap_or_else(|| panic!("harness/defaults.toml has no number for {name}"))
}

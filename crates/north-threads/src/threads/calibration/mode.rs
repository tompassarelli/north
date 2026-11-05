//! The recommendation mode the operator's orchestration.toml names.
use std::path::{Path, PathBuf};

use toml_edit::DocumentMut;

/// The providers whose tiers a recommendation may name, and whether that is every provider.
pub struct Mode {
    pub name: String,
    pub allowed: Vec<String>,
}

pub fn configuration(home: &Path) -> PathBuf {
    if let Some(path) = std::env::var_os("AGENTS_ORCHESTRATION").filter(|path| !path.is_empty()) {
        return PathBuf::from(path);
    }
    let operator = std::env::var_os("AGENTS_OPERATOR_DIR").filter(|path| !path.is_empty()).map_or_else(|| home.join("code/south/main/agents"), PathBuf::from);
    operator.join("orchestration.toml")
}

/// The mode and the tier keys it allows, or why the configuration cannot say.
pub fn resolve(path: &Path) -> Result<Mode, String> {
    let refused = |why: String| format!("cannot resolve recommendation mode from {}: {why}", path.display());
    let text = std::fs::read_to_string(path).map_err(|error| refused(error.to_string()))?;
    let document: DocumentMut = text.parse().map_err(|error: toml_edit::TomlError| refused(error.to_string()))?;
    let keys: Vec<String> = document.get("tiers").and_then(|tiers| tiers.as_table_like()).map(|tiers| tiers.iter().map(|(key, _)| key.to_owned()).collect()).unwrap_or_default();
    let mut providers: Vec<&str> = Vec::new();
    for key in &keys {
        let provider = key.split(':').next().unwrap_or(key);
        if !providers.contains(&provider) {
            providers.push(provider);
        }
    }
    let name = document.get("mode").and_then(|mode| mode.as_str()).unwrap_or("split").to_owned();
    let wanted: Vec<&str> = match name.strip_suffix("-only") {
        _ if name == "split" => providers.clone(),
        Some(only) if providers.contains(&only) => vec![only],
        _ => {
            let only: Vec<String> = providers.iter().map(|provider| format!("{provider}-only")).collect();
            return Err(refused(format!("unknown mode '{name}'; use split, {}", only.join(", "))));
        }
    };
    let allowed: Vec<String> = keys.iter().filter(|key| wanted.contains(&key.split(':').next().unwrap_or(key))).cloned().collect();
    if name == "codex-only" {
        default_band(&document, &keys, &allowed).ok_or_else(|| format!("no mode-allowed band for default tier {}", super::default_tier()))?;
    }
    Ok(Mode { name, allowed })
}

/// The allowed tier whose difficulty band holds the start of the default tier's band, None when there is none.
fn default_band(document: &DocumentMut, keys: &[String], allowed: &[String]) -> Option<String> {
    let starts: Vec<f64> = keys.iter().map(|key| document["tiers"][key.as_str()]["start"].as_float().or_else(|| document["tiers"][key.as_str()]["start"].as_integer().map(|whole| whole as f64)).unwrap_or(0.0)).collect();
    let mut floor = 0.0_f64;
    let mut lows: Vec<f64> = Vec::new();
    for (index, start) in starts.iter().enumerate() {
        floor = if index == 0 { 0.0 } else { floor.max(*start) };
        lows.push(floor);
    }
    let high = |index: usize| lows.get(index + 1).copied().unwrap_or(100.0);
    let default_start = lows[keys.iter().position(|key| *key == super::default_tier())?];
    let mut resolved: Vec<(f64, f64, &String)> = Vec::new();
    let mut pending: Option<f64> = None;
    for (index, key) in keys.iter().enumerate() {
        if allowed.contains(key) {
            resolved.push((pending.take().unwrap_or(lows[index]), high(index), key));
        } else if pending.is_none() {
            pending = Some(lows[index]);
        }
    }
    if pending.is_some()
        && let Some(last) = resolved.last_mut()
    {
        last.1 = 100.0;
    }
    resolved.into_iter().find(|(lo, hi, _)| hi > lo && *lo <= default_start && default_start < *hi).map(|(_, _, key)| key.clone())
}

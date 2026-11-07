use toml_edit::DocumentMut;

#[derive(Clone)]
pub(super) struct Tier {
    pub key: String,
    pub model: String,
    pub effort: String,
    pub start: f64,
    pub agent: Option<String>,
    pub role: Option<String>,
}

pub(super) struct Band {
    pub lo: f64,
    pub hi: f64,
    pub key: String,
}

pub(super) struct Mode {
    pub name: String,
    pub providers: Vec<String>,
    pub wanted: Vec<String>,
}

pub(super) fn provider(key: &str) -> &str {
    key.split(':').next().unwrap_or(key)
}

impl Tier {
    pub(super) fn label(&self) -> String {
        match &self.agent {
            Some(agent) => format!("{} {} ({agent})", self.model, self.effort),
            None => format!("{} {}", self.model, self.effort),
        }
    }
}

pub(super) fn read(document: &DocumentMut) -> Result<Vec<Tier>, String> {
    let Some(table) = document.get("tiers").and_then(|tiers| tiers.as_table_like()) else {
        return Ok(Vec::new());
    };
    table
        .iter()
        .map(|(key, item)| {
            let text = |field: &str| item.get(field).and_then(|value| value.as_str()).map(str::to_owned);
            let start = item.get("start").and_then(|value| {
                value.as_float().or_else(|| value.as_integer().map(|number| number as f64))
            });
            match (text("model"), text("effort"), start) {
                (Some(model), Some(effort), Some(start))
                    if key.contains(':') && (0.0..=100.0).contains(&start) =>
                {
                    Ok(Tier { key: key.to_owned(), model, effort, start, agent: text("agent"), role: text("role") })
                }
                _ => Err(format!(
                    "tier '{key}' needs provider:name, model, effort and start = difficulty percentile (0-100)"
                )),
            }
        })
        .collect()
}

pub(super) fn resolve_mode(tiers: &[Tier], configured: Option<&str>, requested: &str) -> Result<Mode, String> {
    let mut providers: Vec<String> = Vec::new();
    for tier in tiers {
        if !providers.iter().any(|known| known == provider(&tier.key)) {
            providers.push(provider(&tier.key).to_owned());
        }
    }
    let name = if requested.is_empty() { configured.unwrap_or("split") } else { requested };
    let wanted = match name.strip_suffix("-only") {
        _ if name == "split" => providers.clone(),
        Some(only) if providers.iter().any(|known| known == only) => vec![only.to_owned()],
        _ => {
            let only: Vec<String> = providers.iter().map(|known| format!("{known}-only")).collect();
            return Err(format!("unknown mode '{name}'; use split, {}", only.join(", ")));
        }
    };
    Ok(Mode { name: name.to_owned(), providers, wanted })
}

pub(super) fn bands_of(tiers: &[Tier]) -> Vec<Band> {
    let mut floor: f64 = 0.0;
    let mut bands: Vec<Band> = tiers
        .iter()
        .enumerate()
        .map(|(index, tier)| {
            let lo = if index == 0 { 0.0 } else { floor.max(tier.start) };
            floor = lo;
            Band { lo, hi: 100.0, key: tier.key.clone() }
        })
        .collect();
    for index in 0..bands.len().saturating_sub(1) {
        bands[index].hi = bands[index + 1].lo;
    }
    bands
}

pub(super) fn resolve_bands(full: &[Band], available: &[String]) -> Vec<Band> {
    if available.is_empty() {
        return Vec::new();
    }
    let mut resolved: Vec<Band> = Vec::new();
    let mut pending: Option<f64> = None;
    for band in full {
        if available.contains(&band.key) {
            resolved.push(Band { lo: pending.take().unwrap_or(band.lo), hi: band.hi, key: band.key.clone() });
        } else if pending.is_none() {
            pending = Some(band.lo);
        }
    }
    if pending.is_some()
        && let Some(last) = resolved.last_mut()
    {
        last.hi = 100.0;
    }
    resolved.retain(|band| band.hi > band.lo);
    resolved
}

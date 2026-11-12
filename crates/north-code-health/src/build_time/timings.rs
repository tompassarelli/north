use serde::Deserialize;

#[derive(Deserialize)]
struct Unit {
    i: usize,
    name: String,
    #[serde(default)]
    target: String,
    start: f64,
    duration: f64,
    #[serde(default, alias = "unlocked_units")]
    unblocked_units: Vec<usize>,
    #[serde(default, alias = "unlocked_rmeta_units")]
    unblocked_rmeta_units: Vec<usize>,
}

pub(super) fn critical_path(html: &str) -> Result<String, String> {
    let units = units(html)?;
    let last = units.iter().max_by(|a, b| (a.start + a.duration).total_cmp(&(b.start + b.duration))).ok_or("cargo timings: no units")?;
    let mut chain = vec![last];
    while let Some(previous) = unlocker(&units, chain[chain.len() - 1].i) {
        if chain.len() > units.len() {
            break;
        }
        chain.push(previous);
    }
    let steps: Vec<String> = chain.iter().rev().filter(|unit| unit.duration >= 1.0 || unit.i == last.i).map(|unit| format!("{}{} {:.1}s", unit.name, label(&unit.target), unit.duration)).collect();
    Ok(format!("{:.1}s: {}", last.start + last.duration, steps.join(" > ")))
}

fn label(target: &str) -> String {
    let target = target.trim();
    if target.is_empty() { String::new() } else { format!(" ({target})") }
}

fn unlocker(units: &[Unit], index: usize) -> Option<&Unit> {
    let started = units.iter().find(|unit| unit.i == index).map_or(f64::INFINITY, |unit| unit.start);
    let unlock_time = |unit: &Unit| {
        if unit.unblocked_units.contains(&index) {
            Some(unit.start + unit.duration)
        } else if unit.unblocked_rmeta_units.contains(&index) {
            Some((unit.start + unit.duration).min(started))
        } else {
            None
        }
    };
    units.iter().filter_map(|unit| unlock_time(unit).map(|time| (unit, time))).max_by(|a, b| a.1.total_cmp(&b.1)).map(|(unit, _)| unit)
}

fn units(html: &str) -> Result<Vec<Unit>, String> {
    const MARK: &str = "UNIT_DATA = ";
    let start = html.find(MARK).ok_or("cargo timings: no UNIT_DATA in the report")? + MARK.len();
    let mut stream = serde_json::Deserializer::from_str(&html[start..]).into_iter::<Vec<Unit>>();
    stream.next().ok_or("cargo timings: empty UNIT_DATA")?.map_err(|error| format!("cargo timings: {error}"))
}

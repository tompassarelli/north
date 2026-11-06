use std::path::Path;

use north_base::foundation::clock::Timestamp;
use serde_json::Value;

use crate::capacity::provider_capacity::Outcome;
use super::report::{Inputs, report};
use super::sampler::{ROTATED_FILE, SAMPLES_FILE};
use super::snapshot::read;

const DEFAULT_WINDOWS: [f64; 2] = [5.0, 30.0];
const KEPT_SAMPLES: usize = 200;

fn lines(path: &Path) -> Vec<String> {
    read(path).unwrap_or_default().lines().filter(|line| !line.is_empty()).map(str::to_owned).collect()
}

pub(super) fn capacity_flag(hits: Result<Vec<Outcome>, String>) -> Option<String> {
    let hits = match hits {
        Err(error) => return Some(format!("provider-capacity check unavailable: {error}")),
        Ok(hits) if hits.is_empty() => return None,
        Ok(hits) => hits,
    };
    let mut models: Vec<&str> = Vec::new();
    for hit in &hits {
        let model = hit.model.as_deref().unwrap_or_default();
        if !models.contains(&model) {
            models.push(model);
        }
    }
    Some(format!("provider-capacity codex: {} turn(s) of {} at capacity, last {}", hits.len(), models.join(", "), hits[hits.len() - 1].at))
}

pub(super) fn run(arguments: &[String]) -> Result<(), String> {
    let mut minutes = Vec::new();
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--minutes" => minutes.push(words.next().and_then(|text| text.parse::<f64>().ok()).ok_or(super::USAGE)?),
            _ => return Err(super::USAGE.to_owned()),
        }
    }
    let dir = super::directory();
    let samples: Vec<Value> = {
        let all: Vec<String> = lines(&dir.join(ROTATED_FILE)).into_iter().chain(lines(&dir.join(SAMPLES_FILE))).collect();
        all[all.len().saturating_sub(KEPT_SAMPLES)..].iter().filter_map(|line| serde_json::from_str(line).ok()).collect()
    };
    let incidents: Vec<String> = lines(&dir.join(super::INCIDENTS_FILE)).into_iter().filter(|line| line.starts_with("- ")).collect();
    let windows = if minutes.is_empty() { DEFAULT_WINDOWS.to_vec() } else { minutes };
    let now = Timestamp::now().unix_millis();
    for window in windows {
        let inputs = Inputs { samples: &samples, incidents: &incidents, now };
        let mut result = report(&inputs, window);
        result["flags"].as_array_mut().expect("a report has flags").extend(capacity_flag(crate::capacity::provider_capacity::capacity_events(window)).map(Value::String));
        println!("{result}");
    }
    Ok(())
}

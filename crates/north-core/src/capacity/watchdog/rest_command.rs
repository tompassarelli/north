use std::path::PathBuf;

use super::rest::{Expected, Observed, check};
use super::rest_live::observe;
use super::snapshot::read;

const DEFAULT_SECONDS: f64 = 30.0;

pub(super) fn run(arguments: &[String]) -> Result<u8, String> {
    let option = |name: &str| arguments.iter().position(|word| word == name).and_then(|at| arguments.get(at + 1));
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| "/tmp".into()));
    let expected_path = option("--expected").map(PathBuf::from).or_else(|| std::env::var_os("CAPACITY_WATCHDOG_REST").map(PathBuf::from)).unwrap_or_else(|| {
        std::env::var_os("XDG_CONFIG_HOME").map_or_else(|| home.join(".config"), PathBuf::from).join("capacity-watchdog/rest.json")
    });
    let Some(text) = read(&expected_path) else {
        eprintln!("no expected-at-rest set at {}", expected_path.display());
        return Ok(2);
    };
    let expected: Expected = serde_json::from_str(&text).map_err(|error| format!("{}: {error}", expected_path.display()))?;
    let observed: Observed = match option("--observed") {
        Some(path) => serde_json::from_str(&read(path).ok_or_else(|| format!("{path}: unreadable"))?).map_err(|error| format!("{path}: {error}"))?,
        None => observe(option("--seconds").and_then(|text| text.parse().ok()).unwrap_or(DEFAULT_SECONDS)),
    };
    if let Some(path) = option("--save") {
        let body = serde_json::to_string(&observed).map_err(|error| error.to_string())?;
        std::fs::write(path, body + "\n").map_err(|error| format!("{path}: {error}"))?;
    }
    let verdict = check(&expected, &observed);
    for line in verdict.lines() {
        println!("{line}");
    }
    Ok(u8::from(!verdict.ok()))
}

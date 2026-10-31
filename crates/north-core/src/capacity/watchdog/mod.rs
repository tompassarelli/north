use std::collections::BTreeMap;
use std::io::Read;
use std::path::PathBuf;

use serde_json::{Value, json};

mod act;
mod detect;
mod owners;
#[cfg(test)]
mod tests;

use detect::{Action, Owner, Sample, Settings, State};

const USAGE: &str = "usage: north capacity watchdog step < SAMPLE.json | replay SAMPLES.jsonl [--owners OWNERS.json] [--cpus N]";
const STATE_FILE: &str = "overload.json";
const INCIDENTS_FILE: &str = "incidents.md";

pub(super) fn run(arguments: &[String]) -> u8 {
    let outcome = match arguments.first().map(String::as_str) {
        Some("step") if arguments.len() == 1 => step(),
        Some("replay") if arguments.len() >= 2 => replay(&arguments[1..]),
        _ => Err(USAGE.to_owned()),
    };
    match outcome {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("north capacity watchdog: {message}");
            2
        }
    }
}

fn directory() -> PathBuf {
    std::env::var_os("CAPACITY_WATCHDOG_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state/agents/watchdog"))
}

fn cpus() -> f64 {
    std::thread::available_parallelism().map_or(1.0, |count| count.get() as f64)
}

fn step() -> Result<(), String> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).map_err(|error| error.to_string())?;
    let value: Value = serde_json::from_str(&input).map_err(|error| format!("sample: {error}"))?;
    let sample = Sample::parse(&value).ok_or("sample lacks at, load, cpuPsi60 or cgroups")?;
    let dir = directory();
    let state_path = dir.join(STATE_FILE);
    let state: State = std::fs::read_to_string(&state_path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let north = std::env::var_os("NORTH_BIN").map(PathBuf::from).or_else(|| std::env::current_exe().ok()).unwrap_or_else(|| "north".into());
    let mut live = owners::Live::new(home, north.clone());
    let (state, actions) = detect::step(&Settings::bundled(cpus()), &mut live, state, &sample);
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let staged = state_path.with_extension("json.tmp");
    std::fs::write(&staged, serde_json::to_vec(&state).map_err(|error| error.to_string())?).and_then(|()| std::fs::rename(&staged, &state_path)).map_err(|error| format!("{}: {error}", state_path.display()))?;
    act::perform(&actions, &dir.join(INCIDENTS_FILE), &north);
    Ok(())
}

fn replay(arguments: &[String]) -> Result<(), String> {
    let mut samples = None;
    let mut owners: BTreeMap<String, Owner> = BTreeMap::new();
    let mut count = cpus();
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--owners" => {
                let path = words.next().ok_or(USAGE)?;
                owners = serde_json::from_str(&std::fs::read_to_string(path).map_err(|error| format!("{path}: {error}"))?).map_err(|error| format!("{path}: {error}"))?;
            }
            "--cpus" => count = words.next().and_then(|text| text.parse().ok()).ok_or(USAGE)?,
            path if samples.is_none() => samples = Some(path.to_owned()),
            _ => return Err(USAGE.to_owned()),
        }
    }
    let path = samples.ok_or(USAGE)?;
    let text = std::fs::read_to_string(&path).map_err(|error| format!("{path}: {error}"))?;
    for action in replayed(&text, &mut owners, Settings::bundled(count)) {
        println!("{}", describe(&action));
    }
    Ok(())
}

fn replayed(text: &str, owners: &mut BTreeMap<String, Owner>, settings: Settings) -> Vec<Action> {
    let mut state = State::default();
    let mut all = Vec::new();
    for sample in text.lines().filter_map(|line| Sample::parse(&serde_json::from_str(line).ok()?)) {
        let (next, actions) = detect::step(&settings, owners, state, &sample);
        state = next;
        all.extend(actions);
    }
    all
}

fn describe(action: &Action) -> Value {
    match action {
        Action::Incident(incident) => json!({ "incident": incident }),
        Action::Cap(cap) => json!({ "cap": cap }),
        Action::Release(cap) => json!({ "release": cap }),
        Action::Send { to, text } => json!({ "send": to, "text": text }),
    }
}

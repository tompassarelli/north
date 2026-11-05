use std::collections::BTreeMap;
use std::io::Read;
use std::path::PathBuf;

use serde_json::{Value, json};

mod act;
mod detect;
mod owners;
mod report;
mod report_command;
mod rest;
mod rest_command;
mod rest_live;
mod sampler;
mod sampling;
mod snapshot;
#[cfg(test)]
mod sampler_tests;
#[cfg(test)]
mod tests;

use detect::{Action, Owner, Sample, Settings, State};

pub const USAGE: &str = "usage: north capacity watchdog sample [--once] [--interval SECONDS]
       north capacity watchdog report [--minutes N]... [--since DATE]
       north capacity watchdog replay SAMPLES.jsonl [--owners OWNERS.json] [--cpus N] | replay --snapshots SNAPSHOTS.jsonl
       north capacity watchdog rest-check [--seconds N] [--expected FILE] [--observed FILE] [--save FILE]
       north capacity watchdog step < SAMPLE.json

sample      every 30 s append load, CPU/memory PSI, per-cgroup CPU, leases, clients and the top processes outside
            capacity scopes to samples.jsonl (rotated daily); log an incident for an unleased process tree over 4 cores
            for 2 minutes and SIGTERM git maintenance/gc/repack outside capacity scopes and the nightly timer. Each
            sample also goes through step, which on sustained overload names the top process trees by fleet session,
            caps the offending session scope and sends the owner and coordinators.
report      summarize the last N minutes (default 5 and 30) of samples and incidents with flags. Claude workers over
            the handoff context with no handoff fire flag in that window since the hook fix. A fire without HANDOFF or
            a Done:/Not done: report in its next three assistant turns flags as handoff ignored. --since DATE replays
            ledger runs from DATE, including pre-fix runs (samples keep their minute window). A Codex turn in the
            window that failed with \"Selected model is at capacity\" flags provider-capacity codex.
replay      run recorded overload samples through the overload decision core and print its actions; with --snapshots,
            run recorded process snapshots through the sampling core and print its incidents and terminations.
rest-check  compare running system and user units, active timers and listening sockets with the host's
            expected-at-rest set and measure each unit's CPU and context switches over N s (default 30); sockets read
            \"PROTO ADDR:PORT UNIT\", UNIT \"-\" when its creating cgroup is gone (kernel WireGuard); print each extra
            unit, socket and over-budget unit, list agent sessions separately, and exit 1 on any failure. --observed
            checks a saved observation, --save writes the live one. Expected set: --expected, $CAPACITY_WATCHDOG_REST
            or ~/.config/capacity-watchdog/rest.json, declared per host in firn.
step        decide on one sample read from stdin.
State: $CAPACITY_WATCHDOG_DIR (default ~/.local/state/agents/watchdog).
Ledger: $THREADS_DB (default ~/.local/state/threads/threads.db), read only.";
const STATE_FILE: &str = "overload.json";
const INCIDENTS_FILE: &str = "incidents.md";

pub(super) fn run(arguments: &[String]) -> u8 {
    let outcome = match arguments.first().map(String::as_str) {
        Some("--help" | "-h") => {
            println!("{USAGE}");
            Ok(0)
        }
        Some("step") if arguments.len() == 1 => step().map(|()| 0),
        Some("sample") => sampler::run(&arguments[1..]).map(|()| 0),
        Some("report") => report_command::run(&arguments[1..]).map(|()| 0),
        Some("rest-check") => rest_command::run(&arguments[1..]),
        Some("replay") if arguments.len() >= 2 => replay(&arguments[1..]).map(|()| 0),
        _ => Err(USAGE.to_owned()),
    };
    match outcome {
        Ok(status) => status,
        Err(message) => {
            eprintln!("north capacity watchdog: {message}");
            2
        }
    }
}

fn directory() -> PathBuf {
    std::env::var_os("CAPACITY_WATCHDOG_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state/agents/watchdog"))
}

fn north_binary() -> PathBuf {
    north_base::platform::north_binary::north_binary()
}

fn cpus() -> f64 {
    super::host::host_cores()
}

fn step() -> Result<(), String> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).map_err(|error| error.to_string())?;
    step_sample(&serde_json::from_str(&input).map_err(|error| format!("sample: {error}"))?)
}

fn step_sample(value: &Value) -> Result<(), String> {
    let sample = Sample::parse(value).ok_or("sample lacks at, load, cpuPsi60 or cgroups")?;
    let dir = directory();
    let state_path = dir.join(STATE_FILE);
    let state: State = std::fs::read_to_string(&state_path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let north = north_binary();
    let mut live = owners::Live::new(home, north.clone());
    let (state, actions) = detect::step(&Settings::bundled(cpus()), &mut live, state, &sample);
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let staged = state_path.with_extension("json.tmp");
    std::fs::write(&staged, serde_json::to_vec(&state).map_err(|error| error.to_string())?).and_then(|()| std::fs::rename(&staged, &state_path)).map_err(|error| format!("{}: {error}", state_path.display()))?;
    act::perform(&actions, &dir.join(INCIDENTS_FILE), &north);
    Ok(())
}

fn replay_snapshots(path: &str) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|error| format!("{path}: {error}"))?;
    let mut state = sampling::State::default();
    for line in text.lines().filter(|line| !line.is_empty()) {
        let stepped = sampling::step(state, serde_json::from_str(line).map_err(|error| format!("{path}: {error}"))?);
        state = stepped.state;
        for incident in &stepped.incidents {
            println!("{}", serde_json::to_string(incident).map_err(|error| error.to_string())?);
        }
        for term in &stepped.terms {
            println!("{}", json!({ "term": term.pid }));
        }
    }
    Ok(())
}

fn replay(arguments: &[String]) -> Result<(), String> {
    if let [flag, path] = arguments
        && flag == "--snapshots"
    {
        return replay_snapshots(path);
    }
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

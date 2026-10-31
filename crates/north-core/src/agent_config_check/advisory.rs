use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::platform::rolling_issue::{Gh, Tracker, settle};

const FAILING_RUNS: u32 = 2;

#[derive(Default, Deserialize, Serialize)]
struct State {
    invocation: String,
    streak: u32,
}

pub(super) fn title(host: &str) -> String {
    format!("Agent configuration check failing on {host}")
}

fn body(host: &str, failures: &[String]) -> String {
    let lines: String = failures.iter().map(|failure| format!("- {failure}\n")).collect();
    format!(
        "Category: bug-known-cause\nSpec: measured\nScope: cross-module\nSurface: rust\nVerify: local-test\nNeeds: local-state\n\n\
         The one rolling issue `north agent-config-check --local` keeps for {host}. It opens when the check fails twice in a row, edits itself as the failures change and closes itself when a run passes.\n\n\
         Failing now:\n\n{lines}\n## Done when\n- [ ] (headless run) `north agent-config-check --local` exits 0 on {host}; this issue then closes itself.\n"
    )
}

fn streak(state_path: &Path, invocation: &str, passed: bool) -> Result<u32, String> {
    let mut state: State = fs::read_to_string(state_path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
    if state.invocation != invocation {
        state.streak = if passed { 0 } else { state.streak + 1 };
        state.invocation = invocation.to_owned();
        if let Some(parent) = state_path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        fs::write(state_path, serde_json::to_string(&state).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    }
    Ok(state.streak)
}

pub(super) fn record(tracker: &mut dyn Tracker, state_path: &Path, host: &str, invocation: &str, failures: &[String]) -> Result<(), String> {
    let streak = streak(state_path, invocation, failures.is_empty())?;
    let body = (streak >= FAILING_RUNS).then(|| body(host, failures));
    settle(tracker, &title(host), body.as_deref(), &format!("`north agent-config-check --local` passed on {host}; it closed this issue."))
}

pub(super) fn record_local(home: &Path, failures: &[String]) {
    let invocation = format!("{}-{}", std::process::id(), crate::foundation::clock::Timestamp::now().unix_nanos());
    let state_path = home.join(".local/state/north/agent-config-check.json");
    if let Err(error) = record(&mut Gh, &state_path, &crate::platform::host::name(), &invocation, failures) {
        eprintln!("agent-config-check: issue: {error}");
    }
}

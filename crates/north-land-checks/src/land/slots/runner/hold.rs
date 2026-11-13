use std::path::Path;
use std::process::Stdio;

use north_base::foundation::carry;
use north_base::foundation::offload::RUN_SECONDS_CEILING;
use north_base::platform::harness;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, Command};
use tokio::sync::watch;

use super::{Arguments, steps, warn};
use crate::land::telemetry::epoch;
use crate::land::trace::check_log::Mark;

const SETTING: &str = "prequeue_one_slot";
const BUILT_ONCE: &str = "prequeue_tests_built_once";
const HOLD_SECONDS: u64 = 2 * RUN_SECONDS_CEILING;

/// One build-host slot held for a run's checks before queueing: `value` is the `HOST SLOT TOKEN` each of its offloaded steps joins it by, and the host keeps the slot while `input` stays open.
pub(super) struct Held {
    pub(super) value: String,
    _input: ChildStdin,
    _holder: Child,
}

/// The slot a hold takes on the run's build host before the run lists its tests, so the listing and every check wait in one slot line, once; nothing when `prequeue_one_slot` is off, the run is placed on no build host, or the host gives it no slot, so each step takes a slot of its own as before.
pub(super) async fn taken(root: &Path, arguments: &Arguments, north: &str, cancel: watch::Receiver<bool>) -> Option<Held> {
    if !on(SETTING) || arguments.after || arguments.runner_host.is_empty() {
        return None;
    }
    let held = hold(root, arguments, north, cancel).await;
    if held.is_none() {
        warn(&format!("safe-push: {} held no slot for the checks before queueing, so each takes a slot of its own", arguments.runner_host));
    }
    held
}

async fn hold(root: &Path, arguments: &Arguments, north: &str, mut cancel: watch::Receiver<bool>) -> Option<Held> {
    let seconds = HOLD_SECONDS.to_string();
    let words = ["capacity", "offload", "hold", "--host", &arguments.runner_host, "--base", &arguments.base, "--head", arguments.tree(), "--timeout", &seconds];
    let mut holder = Command::new(north).args(words).current_dir(root).env(carry::LANDING, "1").envs(arguments.line_budget().map(|left| (carry::LINE_WITHIN, left.to_string()))).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true).spawn().ok()?;
    let (input, said) = (holder.stdin.take()?, holder.stdout.take()?);
    if let Some(stderr) = holder.stderr.take() {
        tokio::spawn(relayed(stderr));
    }
    let mut lines = BufReader::new(said).lines();
    let value = tokio::select! {
        line = lines.next_line() => line.ok().flatten(),
        Ok(_) = cancel.wait_for(|stopped| *stopped) => None,
    };
    let value = value.filter(|value| value.split(' ').filter(|word| !word.is_empty()).count() == 3)?;
    Some(Held { value, _input: input, _holder: holder })
}

async fn relayed(stderr: ChildStderr) {
    let mut lines = BufReader::new(stderr).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        steps::waited(&[Mark { at: epoch(), line: line.clone() }]);
        eprintln!("{line}");
    }
}

/// Whether a held run builds its test binaries in the held slot before it lists them, so the listing and the Rust test check compile nothing again; off, the listing takes a slot of its own with the listing's own packages, as before.
pub(super) fn builds_tests_once() -> bool {
    on(BUILT_ONCE)
}

fn on(setting: &str) -> bool {
    harness::document(harness::DEFAULTS).get(setting).and_then(|entry| entry.get("value")).and_then(|value| value.as_bool()).unwrap_or(true)
}

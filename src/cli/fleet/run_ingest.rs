use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::{Value, json};

use crate::foundation::clock::Timestamp;
use crate::fleet::run_recording::{RetiredRun, RunRecording};
use crate::providers::claude::runs::stop;
use crate::platform::process::status_within;

/// How long a host waits for the ledger to take its conversation's run before it exits and leaves the run to the timer.
const RECORDING_LIMIT: Duration = Duration::from_secs(45);

pub(in crate::cli) struct IngestRuns;

/// The runs row of a retired session: its agent, item, box, end and outcome from the record, its stop from the closing line of its report.
pub(in crate::cli) fn retired_row(run: &RetiredRun) -> Value {
    let at = |ms: u64| Timestamp::from_unix_seconds_f64(ms as f64 / 1000.0);
    let id = run.conversation.as_deref().unwrap_or(&run.name);
    let report: String = run.report.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(600).collect();
    json!({
        "id": id, "session": id, "fleet_name": run.name, "status": "finished", "role": "worker", "provider": run.provider,
        "account": run.account, "model": run.model, "tier": run.model.as_deref().unwrap_or("unknown"),
        "item": run.item, "box": run.box_name, "started": at(run.started_ms).iso(), "ended": at(run.ended_ms).iso(),
        "actual_min": run.ended_ms.saturating_sub(run.started_ms) / 60_000, "outcome": run.outcome,
        "report": report, "stop": stop::line(&run.report),
    })
}

impl RunRecording for IngestRuns {
    fn retired(&self, run: &RetiredRun) {
        let row = (!crate::providers::opencode::runs::PROVIDERS.contains(&run.provider.as_str())).then(|| format!("{}\n", retired_row(run)));
        let _ = row.map(|row| crate::threads::client::ingest(row.as_bytes()).inspect_err(|error| eprintln!("north fleet: the run row for retired {} went unrecorded: {error}", run.name)));
    }

    fn ended(&self, conversation: &str) {
        let recorded = std::env::current_exe().and_then(|north| status_within(Command::new(north).args(["ledger", "worker", "--session", conversation]).stdin(Stdio::null()).stdout(Stdio::null()), RECORDING_LIMIT));
        match recorded {
            Ok(Some(_)) => return,
            Ok(None) => {}
            Err(error) => return eprintln!("north session-host: the run of {conversation} was not recorded: {error}"),
        }
        eprintln!("north session-host: the run of {conversation} was not recorded within {} s; the worker-ledger timer records it", RECORDING_LIMIT.as_secs());
    }
}

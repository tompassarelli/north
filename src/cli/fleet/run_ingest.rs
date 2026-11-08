use std::process::{Command, Stdio};
use std::time::Duration;

use crate::fleet::run_recording::RunRecording;
use crate::platform::process::status_within;

/// How long a host waits for the ledger to take its conversation's run before it exits and leaves the run to the timer.
const RECORDING_LIMIT: Duration = Duration::from_secs(45);

pub(in crate::cli) struct IngestRuns;

impl RunRecording for IngestRuns {
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

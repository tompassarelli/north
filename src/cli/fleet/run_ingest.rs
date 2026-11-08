use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::fleet::run_recording::RunRecording;

/// How long a host waits for the ledger to take its conversation's run before it exits and leaves the run to the timer.
const RECORDING_LIMIT: Duration = Duration::from_secs(45);

pub(in crate::cli) struct IngestRuns;

impl RunRecording for IngestRuns {
    fn ended(&self, conversation: &str) {
        let spawned = std::env::current_exe().and_then(|north| Command::new(north).args(["ledger", "worker", "--session", conversation]).stdin(Stdio::null()).stdout(Stdio::null()).spawn());
        let mut child = match spawned {
            Ok(child) => child,
            Err(error) => return eprintln!("north session-host: the run of {conversation} was not recorded: {error}"),
        };
        let start = Instant::now();
        while start.elapsed() < RECORDING_LIMIT {
            match child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => std::thread::sleep(Duration::from_millis(100)),
                Err(_) => break,
            }
        }
        let _ = child.kill();
        let _ = child.wait();
        eprintln!("north session-host: the run of {conversation} was not recorded within {} s; the worker-ledger timer records it", RECORDING_LIMIT.as_secs());
    }
}

//! The ledger host's pull of every other `[hosts.NAME]` with an ssh destination: a host that cannot reach
//! the ledger prints its unrecorded rows against the ledger's ids, and they join this run's batch.
use std::time::Duration;

use super::{Ids, Options};
use crate::ops::pacing::placement::{self, gate};
use crate::platform::process::run_with_input_within;

const LIMIT: Duration = Duration::from_secs(180);

pub(super) fn hosts(options: &Options, ids: &Ids, rows: &mut Vec<u8>) {
    let Ok(cfg) = placement::config() else { return };
    let Ok(input) = serde_json::to_vec(ids) else { return };
    let here = crate::platform::host::name();
    for machine in gate::machines(&cfg).into_iter().filter(|machine| machine.name != here) {
        match pulled(&machine.ssh, options, &input) {
            Ok(batch) => {
                rows.extend_from_slice(&batch);
                if !batch.is_empty() && !batch.ends_with(b"\n") {
                    rows.push(b'\n');
                }
            }
            Err(why) => eprintln!("north worker-ledger: {} runs unread: {why}", machine.name),
        }
    }
}

fn pulled(destination: &str, options: &Options, input: &[u8]) -> Result<Vec<u8>, String> {
    let mut words = vec!["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", destination, "north", "worker-ledger", "--emit"];
    if !options.since.is_empty() {
        words.extend(["--since", options.since.as_str()]);
    }
    if !options.session.is_empty() {
        words.extend(["--session", options.session.as_str()]);
    }
    if options.backfill {
        words.push("--backfill");
    }
    let output = run_with_input_within("ssh", &words, Some(input), LIMIT).ok_or("timed out")?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).lines().last().unwrap_or("failed").to_owned());
    }
    Ok(output.stdout)
}

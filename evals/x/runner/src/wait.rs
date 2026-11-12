use std::fs;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use crate::cli::Settings;
use crate::error::{Result, RunError};
use crate::gate::{self, Headroom, Verdict};

const LOAD_POLL: Duration = Duration::from_secs(15);
const HEADROOM_POLL: Duration = Duration::from_secs(60);
const REPORT_TIMEOUT_S: &str = "90";

pub struct LoadReading {
    pub loadavg: String,
    pub waited_s: u64,
}

pub fn for_load(max_load: f64) -> Result<LoadReading> {
    let started = Instant::now();
    loop {
        let text = fs::read_to_string("/proc/loadavg")?;
        let loadavg = text
            .split_whitespace()
            .take(3)
            .collect::<Vec<_>>()
            .join(" ");
        if gate::one_minute_load(&text).is_some_and(|load| load < max_load) {
            return Ok(LoadReading {
                loadavg,
                waited_s: started.elapsed().as_secs(),
            });
        }
        eprintln!("eval-x: load {loadavg} is over {max_load}; waiting");
        thread::sleep(LOAD_POLL);
    }
}

pub fn for_headroom(settings: &Settings, account: &str) -> Result<(Headroom, u64)> {
    let started = Instant::now();
    loop {
        let verdict = match read_report(account) {
            Ok(snapshot) => gate::decide(&snapshot, gate::now(), settings),
            Err(error) => Verdict::Wait(format!("the account report is unreadable: {error}")),
        };
        match verdict {
            Verdict::Go(headroom) => return Ok((headroom, started.elapsed().as_secs())),
            Verdict::Wait(reason) => {
                eprintln!("eval-x: account {account}: {reason}; waiting for the reset");
                thread::sleep(HEADROOM_POLL);
            }
        }
    }
}

fn read_report(account: &str) -> Result<gate::Snapshot> {
    let output = Command::new("timeout")
        .args([REPORT_TIMEOUT_S, "north", "accounts", "--json"])
        .output()?;
    if !output.status.success() {
        return Err(RunError::Failed(format!(
            "north accounts exited {}",
            output.status
        )));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    gate::snapshot(&text, account)
}

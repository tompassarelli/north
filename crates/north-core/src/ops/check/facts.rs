use super::expectations::Expectation;
use super::{Facts, Notify};
use crate::ledger;
use crate::ops::dispatch::launch_records;
use crate::platform::process::run_within;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

const REMOTE_WAIT: Duration = Duration::from_secs(30);
const HEARTBEAT: &str = "ops/heartbeat.json";

pub struct Machine {
    pub directory: PathBuf,
    pub host: String,
    pub at: f64,
}

fn local_heartbeat(directory: &Path) -> Result<Value, String> {
    let path = directory.join(HEARTBEAT);
    let bytes = std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("{}: {error}", path.display()))
}

fn remote_heartbeat(host: &str) -> Result<Value, String> {
    let command = format!("cat ~/.local/state/agents/{HEARTBEAT}");
    let output = run_within("ssh", &["-o", "BatchMode=yes", "-o", "ConnectTimeout=10", host, &command], REMOTE_WAIT).ok_or_else(|| format!("ssh {host} did not return within {} s", REMOTE_WAIT.as_secs()))?;
    if !output.status.success() {
        return Err(format!("ssh {host}: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    serde_json::from_slice(&output.stdout).map_err(|error| format!("ssh {host} heartbeat: {error}"))
}

impl Machine {
    fn heartbeat_age_minutes(&self, expectation: &Expectation) -> Result<f64, String> {
        let beat = match expectation.host.as_deref() {
            Some(host) if host != self.host => remote_heartbeat(host)?,
            _ => local_heartbeat(&self.directory)?,
        };
        let at = beat["at"].as_f64().ok_or("heartbeat has no time")?;
        Ok(((self.now() - at) / 60.0).max(0.0))
    }

    fn launches(&self, expectation: &Expectation, counted: impl Fn(&crate::ops::dispatch::LaunchRecord) -> bool) -> f64 {
        let since = self.now() - expectation.window_seconds;
        launch_records(&self.directory.join("dispatch")).iter().filter(|record| record.at >= since && counted(record)).count() as f64
    }
}

impl Facts for Machine {
    fn now(&self) -> f64 {
        self.at
    }

    fn measure(&self, expectation: &Expectation) -> Result<f64, String> {
        match expectation.measure.as_str() {
            "cloud_launches" => Ok(self.launches(expectation, |record| record.launched && record.cloud)),
            "launch_failures" => Ok(self.launches(expectation, |record| !record.launched)),
            "closed_boxes" => ledger::closed_report(&expectation.window).map(|report| report.closed as f64).map_err(|error| error.user_message()),
            "heartbeat_age_minutes" => self.heartbeat_age_minutes(expectation),
            other => Err(format!("no measure named {other}")),
        }
    }
}

pub fn write_heartbeat(directory: &Path, host: &str, at: f64) -> Result<(), String> {
    let path = directory.join(HEARTBEAT);
    write_json(&path, &json!({"at": at, "host": host}))
}

pub fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    let parent = path.parent().ok_or("state file has no parent")?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let mut pending = tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
    std::io::Write::write_all(&mut pending, &serde_json::to_vec(value).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    pending.as_file().sync_all().map_err(|error| error.to_string())?;
    pending.persist(path).map(|_| ()).map_err(|error| error.to_string())
}

pub struct Phone;

impl Notify for Phone {
    fn phone(&mut self, title: &str, text: &str) -> Result<(), String> {
        let output = run_within("nexus-alert", &["normal", text, title], REMOTE_WAIT).ok_or("nexus-alert did not return in time")?;
        output.status.success().then_some(()).ok_or_else(|| format!("nexus-alert: {}", String::from_utf8_lossy(&output.stderr).trim()))
    }

    fn proxy(&mut self, session: &str, text: &str) -> Result<(), String> {
        let north = std::env::current_exe().map_err(|error| error.to_string())?;
        let output = run_within(&north.to_string_lossy(), &["fleet", "send", session, text], REMOTE_WAIT).ok_or("north fleet send did not return in time")?;
        output.status.success().then_some(()).ok_or_else(|| format!("north fleet send {session}: {}", String::from_utf8_lossy(&output.stderr).trim()))
    }
}

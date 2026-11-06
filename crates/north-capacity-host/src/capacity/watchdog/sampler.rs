use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use rustix::process::{Pid, Signal, kill_process};
use serde_json::Value;

use super::sampling::{Incident, Reading, State, Term, step};
use super::snapshot::{read, snapshot};
use crate::platform::process::status_within;

const ROTATE_MS: i64 = 24 * 3600 * 1000;
const DEFAULT_INTERVAL_SECONDS: u64 = 30;
const NOTIFY_WAIT: Duration = Duration::from_secs(15);
const STAT_START_FIELD: usize = 19;
pub(super) const SAMPLES_FILE: &str = "samples.jsonl";
pub(super) const ROTATED_FILE: &str = "samples.1.jsonl";

struct Samples {
    dir: PathBuf,
    file_start: Option<i64>,
}

impl Samples {
    fn append(&mut self, reading: &Reading) -> std::io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let path = self.dir.join(SAMPLES_FILE);
        let now = now_ms();
        if self.file_start.is_none() && path.exists() {
            self.file_start = Some(first_sample_time(&path).unwrap_or(now));
        }
        if self.file_start.is_some_and(|start| now - start >= ROTATE_MS) {
            fs::rename(&path, self.dir.join(ROTATED_FILE))?;
            self.file_start = None;
        }
        self.file_start.get_or_insert(now);
        let line = serde_json::to_string(reading).map_err(std::io::Error::other)?;
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        writeln!(file, "{line}")
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |age| age.as_millis() as i64)
}

fn first_sample_time(path: &Path) -> Option<i64> {
    let head: String = read(path)?.chars().take(200).collect();
    let stamp = head.split("\"at\":\"").nth(1)?.split('"').next()?;
    north_base::foundation::clock::Timestamp::parse_rfc3339(stamp).map(|moment| moment.unix_millis())
}

pub(super) fn terminate(terms: &[Term]) {
    for term in terms {
        let Some(stat) = read(format!("/proc/{}/stat", term.pid)) else { continue };
        let start = stat.rfind(')').and_then(|close| stat.get(close + 2..)).and_then(|rest| rest.split(' ').nth(STAT_START_FIELD));
        if start == Some(term.start.as_str())
            && let Some(pid) = i32::try_from(term.pid).ok().and_then(Pid::from_raw)
        {
            let _ = kill_process(pid, Signal::Term);
        }
    }
}

pub(super) fn entry(incident: &Incident) -> String {
    let kind = serde_json::to_value(&incident.kind).ok().and_then(|value| value.as_str().map(str::to_owned)).unwrap_or_default();
    let pids = incident.pids.iter().map(i64::to_string).collect::<Vec<_>>().join(" ");
    let chain = incident.chain.iter().map(|link| format!("  < {link}")).collect::<Vec<_>>().join("\n");
    format!("- {} {kind}: {}\n  cgroup: {}\n  cwd: {}\n  pids: {pids}\n{chain}\n", incident.at, incident.summary, incident.cgroup, incident.cwd.as_deref().unwrap_or("?"))
}

fn notify(program: &str, incident: &Incident) {
    let kind = serde_json::to_value(&incident.kind).ok().and_then(|value| value.as_str().map(str::to_owned)).unwrap_or_default();
    let mut command = Command::new(program);
    command.args(["-u", "critical", "-a", "capacity-watchdog", &format!("watchdog: {kind}"), &incident.summary]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let _ = status_within(&mut command, NOTIFY_WAIT);
}

fn act(dir: &Path, incidents: &[Incident], terms: &[Term]) {
    terminate(terms);
    if incidents.is_empty() {
        return;
    }
    let _ = fs::create_dir_all(dir);
    let program = std::env::var("CAPACITY_WATCHDOG_NOTIFY").unwrap_or_else(|_| "notify-send".to_owned());
    for incident in incidents {
        let path = dir.join(super::INCIDENTS_FILE);
        if let Err(error) = OpenOptions::new().create(true).append(true).open(&path).and_then(|mut file| file.write_all(entry(incident).as_bytes())) {
            eprintln!("north capacity watchdog: {}: {error}", path.display());
        }
        if !program.is_empty() {
            notify(&program, incident);
        }
    }
}

pub(super) fn run(arguments: &[String]) -> Result<(), String> {
    let once = arguments.iter().any(|word| word == "--once");
    let interval = match arguments.iter().position(|word| word == "--interval") {
        Some(at) => arguments.get(at + 1).and_then(|text| text.parse().ok()).ok_or(super::USAGE)?,
        None => DEFAULT_INTERVAL_SECONDS,
    };
    let dir = super::directory();
    let north = super::north_binary();
    let mut samples = Samples { dir: dir.clone(), file_start: None };
    let mut state = State::default();
    loop {
        let stepped = step(state, snapshot(&north));
        state = stepped.state;
        let measured = state.has_previous() && stepped.reading.interval_seconds > 0.0;
        if measured {
            match samples.append(&stepped.reading) {
                Ok(()) => act_on_overload(&stepped.reading),
                Err(error) => eprintln!("north capacity watchdog: {}: {error}", dir.display()),
            }
        }
        act(&dir, &stepped.incidents, &stepped.terms);
        if once && measured {
            return Ok(());
        }
        if !once && north_base::platform::north_binary::superseded() {
            eprintln!("north capacity watchdog: the promoted runtime moved; exiting so the service starts it");
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs(if once { 1 } else { interval }));
    }
}

fn act_on_overload(reading: &Reading) {
    let sample: Value = serde_json::to_value(reading).unwrap_or(Value::Null);
    if let Err(message) = super::step_sample(&sample) {
        eprintln!("north capacity watchdog: step: {message}");
    }
}

use crate::foundation::clock::Timestamp;
use crate::guard::issue_labels::gh;
use crate::platform::{north_binary::north_binary, process};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

const MINUTES: &str = "break_glass_minutes";
const LEDGER: &str = "north/break-glass.jsonl";
const RECENT_LAUNCHES: usize = 2;
const PROBE_PROVIDER: &str = "claude";
const PROBE_LIMIT: Duration = Duration::from_secs(300);
const OWNER: &str = "tompassarelli";

pub const USAGE: &str = "Usage: north fleet break-glass --issue REPO#N REASON... (lifts the proxy's look budgets for at most 30 minutes, only while the last two launches failed or the launch probe exits non-zero; REPO#N is the issue that tracks the cause)";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Grant {
    pub at_ms: u64,
    pub until_ms: u64,
    pub who: String,
    pub reason: String,
    pub issue: String,
    pub evidence: String,
}

#[derive(Debug, PartialEq)]
pub enum Refusal {
    NoReason,
    WorkersCanLaunch(String),
    Unrecorded(String),
    BoxNotOpened(String),
}

impl Refusal {
    pub fn message(&self) -> String {
        match self {
            Self::NoReason => format!("break glass needs a reason. {USAGE}"),
            Self::WorkersCanLaunch(measured) => format!("break glass refused: workers can launch ({measured}), so the proxy's budgets stay; write the brief and staff a worker instead"),
            Self::Unrecorded(why) => format!("break glass refused: the ledger could not record it ({why}), so nothing was lifted"),
            Self::BoxNotOpened(why) => format!("break glass is lifted and recorded, but the box on the tracking issue was not opened ({why}); open it by hand"),
        }
    }
}

pub trait World {
    fn now_ms(&self) -> u64;
    fn who(&self) -> String;
    fn recent_launches(&self, count: usize) -> Vec<bool>;
    fn probe_exit(&self) -> Option<i32>;
    fn grants(&self) -> Vec<Grant>;
    fn record(&self, grant: &Grant) -> Result<(), String>;
    fn open_box(&self, issue: &str, text: &str) -> Result<(), String>;
}

pub fn minutes() -> u64 {
    north_base::platform::harness::default_number(MINUTES) as u64
}

pub fn measured_stall(world: &impl World) -> Result<String, String> {
    let launches = world.recent_launches(RECENT_LAUNCHES);
    if launches.len() == RECENT_LAUNCHES && launches.iter().all(|launched| !launched) {
        return Ok(format!("the last {RECENT_LAUNCHES} launches failed"));
    }
    match world.probe_exit() {
        Some(code) if code != 0 => Ok(format!("the launch probe exited {code}")),
        Some(_) => Err("the last launches did not both fail and the launch probe exited 0".into()),
        None => Err("the last launches did not both fail and the launch probe could not be run to say otherwise".into()),
    }
}

pub fn invoke(world: &impl World, reason: &str, issue: &str) -> Result<Grant, Refusal> {
    if reason.trim().is_empty() || issue.trim().is_empty() {
        return Err(Refusal::NoReason);
    }
    let evidence = measured_stall(world).map_err(Refusal::WorkersCanLaunch)?;
    let now = world.now_ms();
    let grant = Grant { at_ms: now, until_ms: now + minutes() * 60_000, who: world.who(), reason: reason.trim().to_owned(), issue: issue.to_owned(), evidence };
    world.record(&grant).map_err(Refusal::Unrecorded)?;
    match world.open_box(issue, &tracking_box(&grant)) {
        Ok(()) => Ok(grant),
        Err(why) => Err(Refusal::BoxNotOpened(why)),
    }
}

pub fn active(grants: &[Grant], now_ms: u64) -> Option<&Grant> {
    grants.iter().filter(|grant| grant.at_ms <= now_ms && now_ms < grant.until_ms).max_by_key(|grant| grant.until_ms)
}

pub fn tracking_box(grant: &Grant) -> String {
    format!(
        "- [ ] Find and fix why workers could not launch: break glass was used at {} by {} ({}): {}. Done when: `north runtime probe launch {PROBE_PROVIDER}` exits 0 and the last two entries of ~/.local/state/agents/dispatch/launches.jsonl have `\"launched\":true`.",
        clock(grant.at_ms),
        grant.who,
        grant.evidence,
        one_line(&grant.reason)
    )
}

pub fn digest_section(grants: &[Grant], since_ms: u64, now_ms: u64) -> String {
    let used: Vec<&Grant> = grants.iter().filter(|grant| grant.at_ms >= since_ms).collect();
    if used.is_empty() {
        return String::new();
    }
    let mut text = format!("Break glass used since you last looked ({}): the proxy's look budgets were lifted because workers could not start; each opened a box on the issue that tracks the cause.\n", used.len());
    for grant in used {
        let state = if now_ms < grant.until_ms { format!("lifted until {}", clock(grant.until_ms)) } else { format!("expired at {}", clock(grant.until_ms)) };
        text.push_str(&format!("- {} by {} ({state}): {}\n  Measured: {}\n  Cause tracked in: {}\n", clock(grant.at_ms), grant.who, one_line(&grant.reason), grant.evidence, grant.issue));
    }
    text
}

fn clock(ms: u64) -> String {
    Timestamp::from_unix_seconds_f64(ms as f64 / 1000.0).local_twelve_hour()
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub struct Live {
    state: PathBuf,
}

impl Live {
    pub fn here() -> Self {
        let state = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"));
        Self { state }
    }

    pub fn in_state(state: PathBuf) -> Self {
        Self { state }
    }

    fn ledger(&self) -> PathBuf {
        self.state.join(LEDGER)
    }

    fn launches_file(&self) -> PathBuf {
        self.state.join("agents/dispatch/launches.jsonl")
    }
}

pub fn now_ms() -> u64 {
    (Timestamp::now().unix_seconds_f64() * 1000.0) as u64
}

pub fn recorded() -> Vec<Grant> {
    Live::here().grants()
}

pub fn lifted_now() -> bool {
    active(&recorded(), now_ms()).is_some()
}

impl World for Live {
    fn now_ms(&self) -> u64 {
        now_ms()
    }

    fn who(&self) -> String {
        ["NORTH_FLEET_SESSION", "USER"].iter().find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty())).unwrap_or_else(|| "unknown".into())
    }

    fn recent_launches(&self, count: usize) -> Vec<bool> {
        let text = std::fs::read_to_string(self.launches_file()).unwrap_or_default();
        let launched: Vec<bool> = text.lines().filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok()).filter_map(|record| record["launched"].as_bool()).collect();
        launched[launched.len().saturating_sub(count)..].to_vec()
    }

    fn probe_exit(&self) -> Option<i32> {
        let mut command = Command::new(north_binary());
        command.args(["runtime", "probe", "launch", PROBE_PROVIDER]);
        let output = process::try_run_command_within(command, None, PROBE_LIMIT).ok()?;
        Some(output.map_or(-1, |output| output.status.code().unwrap_or(-1)))
    }

    fn grants(&self) -> Vec<Grant> {
        std::fs::read_to_string(self.ledger()).unwrap_or_default().lines().filter_map(|line| serde_json::from_str(line).ok()).collect()
    }

    fn record(&self, grant: &Grant) -> Result<(), String> {
        let path = self.ledger();
        std::fs::create_dir_all(path.parent().ok_or("no ledger directory")?).map_err(|error| error.to_string())?;
        let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&path).map_err(|error| error.to_string())?;
        let line = format!("{}\n", serde_json::to_string(grant).map_err(|error| error.to_string())?);
        file.write_all(line.as_bytes()).map_err(|error| error.to_string())
    }

    fn open_box(&self, issue: &str, text: &str) -> Result<(), String> {
        let (repo, number) = issue.rsplit_once('#').filter(|(_, number)| number.parse::<u64>().is_ok()).ok_or_else(|| format!("{issue} is not REPO#N"))?;
        let repo = if repo.contains('/') { repo.to_owned() } else { format!("{OWNER}/{repo}") };
        let body = gh(None, &["issue", "view", number, "-R", &repo, "--json", "body", "--jq", ".body"]).ok_or_else(|| format!("gh could not read {issue}"))?;
        let file = tempfile::NamedTempFile::new().map_err(|error| error.to_string())?;
        std::fs::write(file.path(), format!("{}\n{text}\n", body.trim_end())).map_err(|error| error.to_string())?;
        gh(None, &["issue", "edit", number, "-R", &repo, "--body-file", &file.path().to_string_lossy()]).map(drop).ok_or_else(|| format!("gh could not edit {issue}"))
    }
}

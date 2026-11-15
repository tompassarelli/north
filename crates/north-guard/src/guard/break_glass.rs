use north_foundation::foundation::clock::Timestamp;
use crate::guard::gh_cli::gh;
use north_base::platform::{north_binary::north_binary, process};
use north_fleet_store::landing_stall::{self, State};
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
const LANDING_REPO: &str = "north";
const INCIDENT_MINUTES: u64 = 30;
const WATCHDOG_STATE: &str = "overload.json";
const LANDER_SILENT: Duration = Duration::from_secs(30 * 60);

pub const USAGE: &str = "Usage: north fleet break-glass [--issue REPO#N] [--reason] REASON... (with --issue: lifts the coordinator's reach guard for at most 30 minutes, only while the harness measures an incident: the last two launches failed, the launch probe exits non-zero, the laptop load alarm is firing, or the coordinator host rebooted in the last 30 minutes, REPO#N being the issue that tracks the cause; without it: lifts the coordinator's reach guard for at most 30 minutes while `north ledger flow --stall-alarm` reports a fresh landing stall in north)";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Grant {
    pub at_ms: u64,
    pub until_ms: u64,
    pub who: String,
    pub reason: String,
    pub issue: Option<String>,
    pub evidence: String,
    #[serde(default)]
    pub landing: Option<Landing>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Landing {
    pub repo: String,
    pub opened_ms: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Incident {
    WorkersCannotLaunch(String),
    LoadAlarm,
    Rebooted(u64),
}

impl Incident {
    pub fn evidence(&self) -> String {
        match self {
            Self::WorkersCannotLaunch(measured) => measured.clone(),
            Self::LoadAlarm => "the laptop load alarm is firing".into(),
            Self::Rebooted(minutes) => format!("the coordinator host rebooted {minutes} minutes ago"),
        }
    }

    fn cause(&self) -> &'static str {
        match self {
            Self::WorkersCannotLaunch(_) => "workers could not launch",
            Self::LoadAlarm => "the laptop load alarm was firing",
            Self::Rebooted(_) => "the coordinator host had just rebooted",
        }
    }

    fn done_when(&self) -> &'static str {
        match self {
            Self::WorkersCannotLaunch(_) => "`north runtime probe launch claude` exits 0 and the last two entries of ~/.local/state/agents/dispatch/launches.jsonl have `\"launched\":true`",
            Self::LoadAlarm => "`north capacity watchdog report --minutes 5` no longer flags an overload",
            Self::Rebooted(_) => "`north fleet rebooted` lists no session this reboot stranded and `north fleet break-glass` is refused because the host has been up more than 30 minutes",
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum Refusal {
    NoReason,
    NoIncident(String),
    NoIssue(String),
    Unrecorded(String),
    BoxNotOpened(String),
}

impl Refusal {
    pub fn message(&self) -> String {
        match self {
            Self::NoReason => format!("break glass needs a reason. {USAGE}"),
            Self::NoIncident(why) => format!("break glass refused: {why}, so the coordinator's budgets stay; write the brief and staff a worker instead"),
            Self::NoIssue(measured) => format!("break glass refused: {measured}, but no --issue REPO#N names the issue that tracks it, so nothing was lifted"),
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
    fn load_alarm(&self) -> bool;
    fn booted_ms(&self) -> Option<u64>;
    fn landing_stalls(&self) -> State;
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

pub fn measured_incident(world: &impl World) -> Result<Incident, String> {
    if world.load_alarm() {
        return Ok(Incident::LoadAlarm);
    }
    if let Some(booted) = world.booted_ms() {
        let age = world.now_ms().saturating_sub(booted);
        if age < INCIDENT_MINUTES * 60_000 {
            return Ok(Incident::Rebooted(age / 60_000));
        }
    }
    if let Ok(measured) = measured_stall(world) {
        return Ok(Incident::WorkersCannotLaunch(measured));
    }
    Err(format!("workers can launch, the laptop load alarm is not firing and the coordinator host has not rebooted in the last {INCIDENT_MINUTES} minutes"))
}

pub fn invoke(world: &impl World, reason: &str, issue: Option<&str>) -> Result<Grant, Refusal> {
    let reason = reason.trim();
    if reason.is_empty() {
        return Err(Refusal::NoReason);
    }
    let now = world.now_ms();
    if let Some(episode) = world.landing_stalls().remove(LANDING_REPO) {
        let grant = Grant {
            at_ms: now,
            until_ms: now + minutes() * 60_000,
            who: world.who(),
            reason: reason.to_owned(),
            issue: None,
            evidence: format!("the lander stalled in {LANDING_REPO}"),
            landing: Some(Landing { repo: LANDING_REPO.to_owned(), opened_ms: episode.opened_ms }),
        };
        world.record(&grant).map_err(Refusal::Unrecorded)?;
        return Ok(grant);
    }
    let incident = measured_incident(world).map_err(Refusal::NoIncident)?;
    let evidence = incident.evidence();
    let issue = issue.map(str::trim).filter(|issue| !issue.is_empty()).ok_or_else(|| Refusal::NoIssue(evidence.clone()))?;
    let grant = Grant { at_ms: now, until_ms: now + minutes() * 60_000, who: world.who(), reason: reason.to_owned(), issue: Some(issue.to_owned()), evidence, landing: None };
    world.record(&grant).map_err(Refusal::Unrecorded)?;
    match world.open_box(issue, &tracking_box(&grant, &incident)) {
        Ok(()) => Ok(grant),
        Err(why) => Err(Refusal::BoxNotOpened(why)),
    }
}

pub fn active(grants: &[Grant], now_ms: u64) -> Option<&Grant> {
    grants.iter().filter(|grant| grant.landing.is_none() && grant.at_ms <= now_ms && now_ms < grant.until_ms).max_by_key(|grant| grant.until_ms)
}

pub fn lifted(grants: &[Grant], stalls: &State, now_ms: u64) -> bool {
    active(grants, now_ms).is_some()
        || grants.iter().any(|grant| {
            grant.at_ms <= now_ms
                && now_ms < grant.until_ms
                && grant.landing.as_ref().is_some_and(|landing| stalls.get(&landing.repo).is_some_and(|episode| episode.opened_ms == landing.opened_ms))
        })
}

pub fn tracking_box(grant: &Grant, incident: &Incident) -> String {
    format!(
        "- [ ] Find and fix the incident behind break glass: it was used at {} by {} because {} ({}): {}. Done when: {}.",
        clock(grant.at_ms),
        grant.who,
        incident.cause(),
        grant.evidence,
        one_line(&grant.reason),
        incident.done_when()
    )
}

pub fn digest_section(grants: &[Grant], since_ms: u64, now_ms: u64) -> String {
    let used: Vec<&Grant> = grants.iter().filter(|grant| grant.at_ms >= since_ms).collect();
    if used.is_empty() {
        return String::new();
    }
    let mut text = format!("Break glass used since you last looked ({}): the coordinator's reach guard was lifted while workers could not start or while the landing was stalled; each names what was measured and what tracks the cause.\n", used.len());
    for grant in used {
        let (state, tracked) = match &grant.landing {
            Some(landing) => {
                let state = if now_ms < grant.until_ms {
                    format!("lifted until {} or while the landing stall of {} stays fresh", clock(grant.until_ms), landing.repo)
                } else {
                    format!("expired at {}", clock(grant.until_ms))
                };
                (state, format!("the landing-is-stalled issue of {}", landing.repo))
            }
            None => {
                let state = if now_ms < grant.until_ms { format!("lifted until {}", clock(grant.until_ms)) } else { format!("expired at {}", clock(grant.until_ms)) };
                (state, grant.issue.clone().unwrap_or_default())
            }
        };
        text.push_str(&format!("- {} by {} ({state}): {}\n  Measured: {}\n  Cause tracked in: {tracked}\n", clock(grant.at_ms), grant.who, one_line(&grant.reason), grant.evidence));
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
    booted_ms: Option<u64>,
}

impl Live {
    pub fn here() -> Self {
        let state = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"));
        Self { state, booted_ms: None }
    }

    pub fn in_state(state: PathBuf) -> Self {
        Self { state, booted_ms: None }
    }

    pub fn booted_at(self, booted_ms: Option<u64>) -> Self {
        Self { booted_ms, ..self }
    }

    fn ledger(&self) -> PathBuf {
        self.state.join(LEDGER)
    }

    fn launches_file(&self) -> PathBuf {
        self.state.join("agents/dispatch/launches.jsonl")
    }

    fn landing_file(&self) -> PathBuf {
        landing_stall::path(&self.state.join("agents/flow"))
    }
}

pub fn now_ms() -> u64 {
    (Timestamp::now().unix_seconds_f64() * 1000.0) as u64
}

pub fn recorded() -> Vec<Grant> {
    Live::here().grants()
}

pub fn lifted_now() -> bool {
    let live = Live::here();
    lifted(&live.grants(), &live.landing_stalls(), now_ms())
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

    fn load_alarm(&self) -> bool {
        let dir = std::env::var_os("CAPACITY_WATCHDOG_DIR").map(PathBuf::from).unwrap_or_else(|| self.state.join("agents/watchdog"));
        let Ok(text) = std::fs::read_to_string(dir.join(WATCHDOG_STATE)) else { return false };
        let Ok(state) = serde_json::from_str::<serde_json::Value>(&text) else { return false };
        state["reported"].as_bool() == Some(true)
    }

    fn booted_ms(&self) -> Option<u64> {
        self.booted_ms
    }

    fn landing_stalls(&self) -> State {
        let path = self.landing_file();
        let fresh = std::fs::metadata(&path).and_then(|meta| meta.modified()).ok().and_then(|modified| modified.elapsed().ok()).is_some_and(|age| age <= LANDER_SILENT);
        if fresh { landing_stall::load(&path) } else { State::new() }
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

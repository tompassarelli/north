use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use north_base::lifecycle::{self, Effect};
use north_runtime_follow::Follow;

use crate::error::NorthResult;

use super::super::gates::GateRequest;
use super::super::host::{Host, alive};
use super::super::model::{Delivery, Role, SessionRecord, Status};
use super::super::receipts::{Aftermath, Courier, HostDeath, deliver_death, still_watched};
use super::super::records::write_record;
use super::super::runtime::{Runtime, binary, new_uuid, now_ms};
use super::super::sender::Sender;
use super::super::turns::Turns;
use super::super::unaccepted::Unaccepted;
use super::Fleet;
use super::relaunching::REVIVED_MID_TURN;

const WATCHDOG_POLL: Duration = Duration::from_secs(1);
/// The outbox kind of a fleet watchdog start (north#862).
const WATCHDOG: &str = "fleet-watchdog";
/// How long one watchdog start may take before another process takes it over.
const WATCHDOG_START: Duration = Duration::from_secs(60);

impl Fleet {
    pub fn guard(&self, name: &str) -> NorthResult<()> {
        let Some(host_pid) = self.record(name)?.host_pid else { return Ok(()) };
        self.guard_host(name, host_pid)
    }

    /// Watches `host_pid` while it serves `name`, then acts on its death as `guard` does.
    pub fn guard_host(&self, name: &str, host_pid: u32) -> NorthResult<()> {
        let mut follow = Some(Follow::current());
        while alive(host_pid) && still_watched(self.record(name).ok().as_ref(), host_pid) {
            if let Some(next) = follow.as_mut().and_then(Follow::replaced) {
                let error = follow.take().map_or_else(String::new, |follow| follow.reexec().to_string());
                eprintln!("north fleet watchdog: {name} stays on its runtime, since it cannot move onto {next}: {error}");
            }
            std::thread::sleep(WATCHDOG_POLL);
        }
        let Ok(child) = self.record(name) else { return Ok(()) };
        let runtime = self.runtime(&child)?;
        let lines = self.recent(runtime.as_ref(), &child)?.entries;
        let death = HostDeath { child: &child, host_pid, mid_task: mid_task(self, runtime.as_ref(), &child)?, lines: &lines };
        match deliver_death(self.ledger.as_ref(), &Inline(self), &death, now_ms())? {
            Aftermath::Revive => self.revive_mid_task(name, host_pid),
            Aftermath::Reported => Host::new(child.host_dir.clone()).terminate(),
            Aftermath::Quiet => self.resume_unaccepted(name, host_pid),
        }
    }

    fn revive_mid_task(&self, name: &str, dead_host: u32) -> NorthResult<()> {
        {
            let _start = self.start_lock(name)?;
            let Ok(mut child) = self.record(name) else { return Ok(()) };
            if !still_watched(Some(&child), dead_host) {
                return Ok(());
            }
            Host::new(child.host_dir.clone()).terminate()?;
            child.revived_mid_task_ms = Some(now_ms());
            write_record(&self.session_dir(name)?, &child)?;
            self.revive_held(name, None, None, None)?;
        }
        self.carry_on(name, REVIVED_MID_TURN)
    }

    fn resume_unaccepted(&self, name: &str, dead_host: u32) -> NorthResult<()> {
        let _start = self.start_lock(name)?;
        let Ok(child) = self.record(name) else { return Ok(()) };
        if still_watched(Some(&child), dead_host) && Unaccepted::awaits_a_host(&child.host_dir) {
            self.revive_held(name, None, None, None)?;
        }
        Ok(())
    }

    /// Whether a report to `name` reaches a live session; a proxy name whose holder died first moves to a live window, as a message to it would.
    pub fn listens(&self, name: &str) -> NorthResult<bool> {
        match self.record(name) {
            Ok(_) => {
                self.rebind_dead_proxy(name)?;
                Ok(self.status(&self.record(name)?) != Status::Exited)
            }
            Err(_) => self.known_elsewhere(name),
        }
    }

    pub fn role_of(&self, name: &str) -> Option<Role> {
        self.record(name).ok().map(|record| record.role())
    }

    pub fn tell(&self, parent: &str, text: &str, delivery: Delivery) {
        let told = match self.record(parent) {
            Ok(record) if record.ended() => Ok(()),
            _ => self.send_message(&Sender::fleet(), parent, text, delivery).map(drop),
        };
        if let Err(error) = told {
            eprintln!("north fleet: {parent} was not told: {}", error.user_message());
        }
    }

    /// The watchdog start a session's move to Live commits, for a session with a parent or a task to report to, keyed by
    /// its launch and host so one host never gets two.
    pub(super) fn watchdog(record: &SessionRecord) -> Vec<Effect> {
        let Some(host_pid) = record.host_pid.filter(|_| record.parent.is_some() || record.task_ref.is_some()) else { return Vec::new() };
        let key = format!("{WATCHDOG}/{}/{host_pid}", record.launch().instance());
        vec![Effect { kind: WATCHDOG.to_owned(), key, payload: serde_json::json!({ "name": record.name, "host_pid": host_pid }), delivery: lifecycle::Delivery::ReplaySafe }]
    }

    /// Starts every watchdog a move to Live committed that no process started yet, a killed launcher's included; one
    /// whose session moved to another host or whose host is gone settles without a start.
    pub fn start_watchdogs(&self) -> NorthResult<usize> {
        self.phases()?.deliver(WATCHDOG, WATCHDOG_START, |leased| {
            let name = leased.effect.payload["name"].as_str().unwrap_or_default();
            let host_pid = leased.effect.payload["host_pid"].as_u64().and_then(|pid| u32::try_from(pid).ok());
            if host_pid.is_none_or(|pid| !alive(pid) || self.record(name).ok().and_then(|record| record.host_pid) != Some(pid)) {
                return Ok(());
            }
            detach(&format!("watchdog-{}", unit_word(name)), &["fleet", "watchdog", name]).map_err(|error| error.user_message())
        })
    }
}

impl Courier for Fleet {
    fn notify(&self, parent: &str, text: String, delivery: Delivery) {
        self.tell(parent, &text, delivery);
    }

    fn hold(&self, parent: &str, text: String) {
        crate::fleet::held::hold(&self.root, parent, &Sender::fleet().to_string(), &text, now_ms());
    }

    fn ask(&self, request: GateRequest) {
        let fleet = self.clone();
        std::thread::spawn(move || raise(&fleet, &request));
    }

    fn answer(&self, host_dir: &Path, frame: String) {
        let host_dir = host_dir.to_path_buf();
        std::thread::spawn(move || reply(&host_dir, frame));
    }
}

struct Inline<'a>(&'a Fleet);

impl Courier for Inline<'_> {
    fn notify(&self, parent: &str, text: String, delivery: Delivery) {
        self.0.tell(parent, &text, delivery);
    }

    fn hold(&self, parent: &str, text: String) {
        Courier::hold(self.0, parent, text);
    }

    fn ask(&self, request: GateRequest) {
        raise(self.0, &request);
    }

    fn answer(&self, host_dir: &Path, frame: String) {
        reply(host_dir, frame);
    }
}

pub(super) fn await_default(gate: i64) -> NorthResult<()> {
    detach(&format!("gate-await-{gate}"), &["gate", "await", &gate.to_string()])
}

fn detach(unit: &str, arguments: &[&str]) -> NorthResult<()> {
    let mut child = Command::new(binary("setsid"))
        .arg(binary("systemd-run"))
        .args(["--user", "--scope", "--quiet", "--collect", "--slice=agent.slice", "--unit"])
        .arg(format!("{unit}-{}.scope", new_uuid()?))
        .arg("--")
        .arg(std::env::current_exe()?)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

fn unit_word(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect()
}

fn raise(fleet: &Fleet, request: &GateRequest) {
    if let Err(error) = fleet.open_gate(request) {
        eprintln!("north fleet: question from {} not raised: {}", request.session, error.user_message());
    }
}

fn reply(host_dir: &Path, frame: String) {
    if let Err(error) = Host::new(host_dir.to_path_buf()).send(&[frame]) {
        eprintln!("north fleet: question reply not sent: {}", error.user_message());
    }
}

fn mid_task(fleet: &Fleet, runtime: &dyn Runtime, record: &SessionRecord) -> NorthResult<bool> {
    let Some(conversation) = record.conversation.as_deref() else { return Ok(false) };
    let mut turns = Turns::default();
    for frame in fleet.recent_frames(record)? {
        for signal in runtime.signals(&frame) {
            turns.observe(signal, 0);
        }
    }
    Ok(turns.busy(conversation))
}

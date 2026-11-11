pub mod fleet;
pub mod health;
pub mod hosts;
pub mod landing;
pub mod rollout;

use std::collections::BTreeMap;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::snapshot::{Builds, Fleet, Health, Host, HostState, Lander, Landing, Reading, Rollout, Snapshot};

pub enum Update {
    Fleet(Result<Fleet, String>),
    Landing(Result<Landing, String>),
    Landers(Result<Vec<Lander>, String>),
    Host(String, Result<HostState, String>),
    Lag(Result<BTreeMap<String, BTreeMap<String, u32>>, String>),
    Rollout(Result<Rollout, String>),
    Health(Result<Health, String>),
    Builds(Result<Builds, String>),
    Ladder(Result<u8, String>),
}

pub struct HostTarget {
    pub name: String,
    pub ssh: Option<String>,
}

pub fn default_hosts() -> Vec<HostTarget> {
    [("whiterabbit", None), ("gex45", Some("tom@gex45")), ("nexus", Some("nexus")), ("cpx62", Some("tom@cpx62")), ("cpx62b", Some("tom@cpx62b"))]
        .into_iter()
        .map(|(name, ssh)| HostTarget { name: name.into(), ssh: ssh.map(str::to_owned) })
        .collect()
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

pub fn state_dir() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .unwrap_or_else(|| PathBuf::from(".local/state"))
        .join("north")
}

pub fn run(program: &str, arguments: &[&str], timeout: Duration) -> Result<String, String> {
    let (success, text, complaint) = output(program, arguments, timeout)?;
    if success {
        Ok(text)
    } else {
        let first = complaint.lines().find(|line| !line.trim().is_empty()).unwrap_or("").to_owned();
        Err(format!("{program} failed: {first}"))
    }
}

pub fn output(program: &str, arguments: &[&str], timeout: Duration) -> Result<(bool, String, String), String> {
    let mut child = Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("{program}: {error}"))?;
    let mut stdout = child.stdout.take().ok_or("no stdout")?;
    let mut stderr = child.stderr.take().ok_or("no stderr")?;
    let reader = thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.read_to_string(&mut text);
        text
    });
    let errors = thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("{program} timed out after {}s", timeout.as_secs()));
        }
        thread::sleep(Duration::from_millis(50));
    };
    Ok((status.success(), reader.join().unwrap_or_default(), errors.join().unwrap_or_default()))
}

fn every(period: Duration, updates: Sender<Update>, mut read: impl FnMut() -> Update + Send + 'static) {
    thread::spawn(move || {
        loop {
            if updates.send(read()).is_err() {
                return;
            }
            thread::sleep(period);
        }
    });
}

pub fn spawn(hosts: Vec<HostTarget>) -> Receiver<Update> {
    let (updates, receiver) = channel();
    let state = state_dir();
    every(Duration::from_secs(3), updates.clone(), || Update::Fleet(fleet::live()));
    let queue = state.join("land-queue");
    let mut cache = landing::Cache::default();
    every(Duration::from_secs(4), updates.clone(), move || Update::Landing(landing::read(&queue, now(), &mut cache)));
    let queue = state.join("land-queue");
    every(Duration::from_secs(4), updates.clone(), move || Update::Landers(landing::landers(&queue, now())));
    for host in hosts {
        let period = Duration::from_secs(if host.ssh.is_some() { 30 } else { 5 });
        every(period, updates.clone(), move || Update::Host(host.name.clone(), hosts::read(&host)));
    }
    every(Duration::from_secs(30), updates.clone(), || Update::Lag(hosts::lag()));
    let pins = state.clone();
    every(Duration::from_secs(5), updates.clone(), move || Update::Rollout(rollout::read(&pins, now())));
    let checks = state.clone();
    every(Duration::from_secs(5), updates.clone(), move || Update::Health(health::checks(&checks)));
    let spans = state.clone();
    every(Duration::from_secs(15), updates.clone(), move || Update::Builds(health::builds_tail(&spans)));
    every(Duration::from_secs(2), updates, move || Update::Ladder(health::ladder(&state)));
    receiver
}

pub fn apply(snapshot: &mut Snapshot, update: Update, at: u64) {
    match update {
        Update::Fleet(result) => snapshot.fleet.update(at, result),
        Update::Landing(result) => snapshot.landing.update(at, result),
        Update::Landers(result) => snapshot.landers.update(at, result),
        Update::Host(name, result) => match snapshot.hosts.iter_mut().find(|host| host.name == name) {
            Some(host) => host.state.update(at, result),
            None => {
                let mut state = Reading::default();
                state.update(at, result);
                snapshot.hosts.push(Host { name, state });
            }
        },
        Update::Lag(result) => snapshot.lag.update(at, result),
        Update::Rollout(result) => snapshot.rollout.update(at, result),
        Update::Health(result) => snapshot.health.update(at, result),
        Update::Builds(result) => snapshot.builds.update(at, result),
        Update::Ladder(result) => snapshot.ladder.update(at, result),
    }
}

pub fn seed(hosts: &[HostTarget]) -> Snapshot {
    Snapshot { hosts: hosts.iter().map(|host| Host { name: host.name.clone(), state: Reading::default() }).collect(), ..Snapshot::default() }
}

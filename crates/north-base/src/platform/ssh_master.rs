use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::ssh::{argv, OWN_CONNECTION};

pub const SLOW: Duration = Duration::from_secs(3);
const DIRECT_LIMIT: Duration = Duration::from_secs(15);
const READY_LIMIT: Duration = Duration::from_secs(15);
const UNIT_LIMIT: Duration = Duration::from_secs(30);

const USAGE: &str = "usage: north ssh-master probe HOST...
  time `ssh HOST true` through each host's shared master; a master slower than 3 s twice while a direct
  connection answers is killed, its socket removed and its ssh-master-HOST user unit restarted";

pub trait Master {
    fn stop(&mut self, host: &str) -> Result<(), String>;
    fn start(&mut self, host: &str) -> Result<(), String>;
}

#[derive(Debug)]
pub enum Health {
    Answered(Duration),
    Unreachable,
    Recycled { direct: Duration, after: Option<Duration> },
    RecycleFailed { direct: Duration, why: String },
}

impl Health {
    pub fn line(&self, host: &str) -> String {
        let seconds = |took: &Duration| format!("{:.2} s", took.as_secs_f64());
        match self {
            Self::Answered(took) => format!("{host}: {} through the shared master", seconds(took)),
            Self::Unreachable => format!("{host}: no answer through the master within 3 s nor directly within 15 s; left the master alone"),
            Self::Recycled { direct, after } => format!(
                "{host}: the master took over 3 s while a direct connection answered in {}; recycled it, now {}",
                seconds(direct),
                after.as_ref().map_or_else(|| "no answer within 3 s".to_owned(), seconds),
            ),
            Self::RecycleFailed { direct, why } => format!("{host}: the master took over 3 s while a direct connection answered in {}; recycling failed: {why}", seconds(direct)),
        }
    }
}

pub fn probe(program: &[OsString], env: &BTreeMap<String, String>, host: &str, master: &mut dyn Master) -> Health {
    let through = argv(program, env, &["-o", "BatchMode=yes", host, "true"]);
    if let Some(took) = round_trip(&through, SLOW) {
        return Health::Answered(took);
    }
    let direct = argv(program, env, &["-o", "BatchMode=yes", "-o", "ConnectTimeout=10", "-o", OWN_CONNECTION, host, "true"]);
    let Some(direct) = round_trip(&direct, DIRECT_LIMIT) else {
        return Health::Unreachable;
    };
    if let Some(took) = round_trip(&through, SLOW) {
        return Health::Answered(took);
    }
    match recycle(program, env, host, master) {
        Ok(()) => Health::Recycled { direct, after: round_trip(&through, SLOW) },
        Err(why) => Health::RecycleFailed { direct, why },
    }
}

fn recycle(program: &[OsString], env: &BTreeMap<String, String>, host: &str, master: &mut dyn Master) -> Result<(), String> {
    let socket = socket(program, env, host)?;
    master.stop(host)?;
    match std::fs::remove_file(&socket) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(format!("removing {}: {error}", socket.display())),
        _ => {}
    }
    master.start(host)?;
    let check = argv(program, env, &["-o", "BatchMode=yes", "-O", "check", host]);
    let started = Instant::now();
    while round_trip(&check, SLOW).is_none() {
        if started.elapsed() > READY_LIMIT {
            return Err(format!("no master answered on {} within {} s of the restart", socket.display(), READY_LIMIT.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

fn socket(program: &[OsString], env: &BTreeMap<String, String>, host: &str) -> Result<PathBuf, String> {
    let words = argv(program, env, &["-G", host]);
    let output = Command::new(&words[0]).args(&words[1..]).stdin(Stdio::null()).stderr(Stdio::null()).output().map_err(|error| format!("ssh -G {host}: {error}"))?;
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("controlpath "))
        .filter(|path| *path != "none")
        .map(PathBuf::from)
        .ok_or_else(|| format!("ssh -G {host} names no control path"))
}

fn round_trip(words: &[OsString], limit: Duration) -> Option<Duration> {
    let started = Instant::now();
    let mut child = Command::new(&words[0]).args(&words[1..]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().ok()?;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success().then(|| started.elapsed()),
            Ok(None) if started.elapsed() < limit => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

struct Unit;

impl Unit {
    fn systemctl(verb: &[&str], host: &str) -> Result<(), String> {
        let unit = format!("ssh-master-{host}.service");
        let mut words = vec!["--user"];
        words.extend(verb);
        words.push(&unit);
        let output = super::process::run_within("systemctl", &words, UNIT_LIMIT).ok_or_else(|| format!("systemctl {} {unit} did not finish within {} s", verb.join(" "), UNIT_LIMIT.as_secs()))?;
        output.status.success().then_some(()).ok_or_else(|| format!("systemctl {} {unit}: {}", verb.join(" "), String::from_utf8_lossy(&output.stderr).trim()))
    }
}

impl Master for Unit {
    fn stop(&mut self, host: &str) -> Result<(), String> {
        Self::systemctl(&["kill", "--signal=SIGKILL"], host)
    }

    fn start(&mut self, host: &str) -> Result<(), String> {
        Self::systemctl(&["restart"], host)
    }
}

pub fn run(arguments: &[String]) -> u8 {
    let Some(("probe", hosts)) = arguments.split_first().map(|(verb, hosts)| (verb.as_str(), hosts)) else {
        eprintln!("{USAGE}");
        return 64;
    };
    if hosts.is_empty() || hosts.iter().any(|host| host.starts_with('-')) {
        eprintln!("{USAGE}");
        return 64;
    }
    let env: BTreeMap<String, String> = std::env::vars().collect();
    let program = [OsString::from("ssh")];
    let healths: Vec<Health> = std::thread::scope(|scope| {
        let probes: Vec<_> = hosts.iter().map(|host| scope.spawn(|| probe(&program, &env, host, &mut Unit))).collect();
        probes.into_iter().map(|probe| probe.join().unwrap_or(Health::Unreachable)).collect()
    });
    let mut code = 0;
    for (host, health) in hosts.iter().zip(&healths) {
        println!("{}", health.line(host));
        if matches!(health, Health::RecycleFailed { .. }) {
            code = 1;
        }
    }
    code
}

#[cfg(test)]
#[path = "ssh_master_tests.rs"]
mod tests;

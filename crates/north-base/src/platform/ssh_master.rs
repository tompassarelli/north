use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::ssh::OWN_CONNECTION;

pub const SLOW: Duration = Duration::from_secs(3);
const DIRECT_LIMIT: Duration = Duration::from_secs(15);
const READY_LIMIT: Duration = Duration::from_secs(15);
const UNIT_LIMIT: Duration = Duration::from_secs(30);

const USAGE: &str = "usage: north ssh-master probe HOST...
  time `ssh HOST true` through each host's shared master; a master slower than 3 s twice while a direct
  connection answers in under 1.5 s is killed, its socket removed and its ssh-master-HOST user unit restarted";

pub trait Master {
    fn stop(&mut self, host: &str) -> Result<(), String>;
    fn start(&mut self, host: &str) -> Result<(), String>;
}

trait Dial {
    fn round_trip(&mut self, words: &[OsString], limit: Duration) -> Option<Duration>;
}

struct Process;

impl Dial for Process {
    fn round_trip(&mut self, words: &[OsString], limit: Duration) -> Option<Duration> {
        let started = Instant::now();
        let mut child = Command::new(&words[0]).args(&words[1..]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().ok()?;
        let status = super::process::child_within(&mut child, limit).ok().flatten()?;
        status.success().then(|| started.elapsed())
    }
}

#[derive(Debug)]
pub enum Health {
    Answered(Duration),
    Unreachable,
    Loaded { direct: Duration },
    Recycled { direct: Duration, after: Option<Duration> },
    RecycleFailed { direct: Duration, why: String },
}

impl Health {
    pub fn line(&self, host: &str) -> String {
        let seconds = |took: &Duration| format!("{:.2} s", took.as_secs_f64());
        match self {
            Self::Answered(took) => format!("{host}: {} through the shared master", seconds(took)),
            Self::Unreachable => format!("{host}: no answer through the master within 3 s nor directly within 15 s; left the master alone"),
            Self::Loaded { direct } => format!("{host}: the master took over 3 s but a direct connection took {} too, so the host or this machine is loaded; left the master and its sessions alone", seconds(direct)),
            Self::Recycled { direct, after } => format!(
                "{host}: the master took over 3 s while a direct connection answered in {}; recycled it, now {}",
                seconds(direct),
                after.as_ref().map_or_else(|| "no answer within 3 s".to_owned(), seconds),
            ),
            Self::RecycleFailed { direct, why } => format!("{host}: the master took over 3 s while a direct connection answered in {}; recycling failed: {why}", seconds(direct)),
        }
    }
}

fn argv(program: &[OsString], env: &BTreeMap<String, String>, arguments: &[&str]) -> Vec<OsString> {
    let mut words = program.to_vec();
    if let Some(config) = env.get("NORTH_SSH_CONFIG") {
        words.extend(["-F".into(), config.into()]);
    }
    words.extend(arguments.iter().map(OsString::from));
    words
}

pub fn probe(program: &[OsString], env: &BTreeMap<String, String>, host: &str, master: &mut dyn Master) -> Health {
    probe_with(&mut Process, program, env, host, master)
}

fn probe_with(dial: &mut dyn Dial, program: &[OsString], env: &BTreeMap<String, String>, host: &str, master: &mut dyn Master) -> Health {
    let through = argv(program, env, &["-o", "BatchMode=yes", host, "true"]);
    if let Some(took) = dial.round_trip(&through, SLOW) {
        return Health::Answered(took);
    }
    let direct = argv(program, env, &["-o", "BatchMode=yes", "-o", "ConnectTimeout=10", "-o", OWN_CONNECTION, host, "true"]);
    let Some(direct) = dial.round_trip(&direct, DIRECT_LIMIT) else {
        return Health::Unreachable;
    };
    if direct >= SLOW / 2 {
        return Health::Loaded { direct };
    }
    if let Some(took) = dial.round_trip(&through, SLOW) {
        return Health::Answered(took);
    }
    match recycle(dial, program, env, host, master) {
        Ok(()) => Health::Recycled { direct, after: dial.round_trip(&through, SLOW) },
        Err(why) => Health::RecycleFailed { direct, why },
    }
}

fn recycle(dial: &mut dyn Dial, program: &[OsString], env: &BTreeMap<String, String>, host: &str, master: &mut dyn Master) -> Result<(), String> {
    let socket = socket(program, env, host)?;
    master.stop(host)?;
    match std::fs::remove_file(&socket) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(format!("removing {}: {error}", socket.display())),
        _ => {}
    }
    master.start(host)?;
    let check = argv(program, env, &["-o", "BatchMode=yes", "-O", "check", host]);
    let started = Instant::now();
    while dial.round_trip(&check, SLOW).is_none() {
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


pub fn reopen_if_dead(program: &[OsString], env: &BTreeMap<String, String>, host: &str) -> Result<bool, String> {
    reopen_with(program, env, host, &mut Unit)
}

fn reopen_with(program: &[OsString], env: &BTreeMap<String, String>, host: &str, master: &mut dyn Master) -> Result<bool, String> {
    let Ok(socket) = socket(program, env, host) else { return Ok(false) };
    match std::os::unix::net::UnixStream::connect(&socket) {
        Err(error) if error.kind() == std::io::ErrorKind::ConnectionRefused => {}
        _ => return Ok(false),
    }
    std::fs::remove_file(&socket).map_err(|error| format!("removing {}: {error}", socket.display()))?;
    master.start(host)?;
    Ok(true)
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

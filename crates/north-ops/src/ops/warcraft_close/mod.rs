#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::Duration;

use rustix::process::{Pid, Signal, kill_process};

const DEFAULT_APPID: &str = "3775098022";
const DEFAULT_COMPAT: &str = ".local/share/Steam/steamapps/compatdata/3516115571";

pub struct Launch {
    pub proc: PathBuf,
    pub compat: String,
    pub appid: String,
}

pub struct Timing {
    pub poll: Duration,
    pub polls: u32,
    pub settle: Duration,
}

impl Launch {
    fn from_env() -> Self {
        let set = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
        Self {
            proc: set("TOM_WARCRAFT_PROC").map_or_else(|| "/proc".into(), PathBuf::from),
            compat: set("TOM_WARCRAFT_COMPAT").unwrap_or_else(|| format!("{}/{DEFAULT_COMPAT}", std::env::var("HOME").unwrap_or_default())),
            appid: set("TOM_WARCRAFT_APPID").unwrap_or_else(|| DEFAULT_APPID.to_owned()),
        }
    }

    pub fn members(&self) -> BTreeSet<u32> {
        let owner = rustix::process::geteuid().as_raw();
        let wanted_env = format!("STEAM_COMPAT_DATA_PATH={}", self.compat);
        let wanted_arg = format!("AppId={}", self.appid);
        let mut parents = BTreeMap::new();
        let mut found = BTreeSet::new();
        for entry in fs::read_dir(&self.proc).into_iter().flatten().flatten() {
            let Some(pid) = entry.file_name().to_str().and_then(|name| name.parse::<u32>().ok()) else { continue };
            let dir = entry.path();
            if fs::metadata(&dir).map_or(true, |meta| meta.uid() != owner) {
                continue;
            }
            let Some((state, parent)) = fs::read(dir.join("stat")).ok().and_then(|stat| parse_stat(&stat)) else { continue };
            if state == 'Z' {
                continue;
            }
            parents.insert(pid, parent);
            if holds(&dir.join("environ"), &wanted_env) || holds(&dir.join("cmdline"), &wanted_arg) {
                found.insert(pid);
            }
        }
        loop {
            let grown: Vec<u32> = parents.iter().filter(|(pid, parent)| !found.contains(pid) && found.contains(parent)).map(|(pid, _)| *pid).collect();
            if grown.is_empty() {
                return found;
            }
            found.extend(grown);
        }
    }

    fn describe(&self, pids: &BTreeSet<u32>) -> Vec<String> {
        pids.iter()
            .map(|pid| {
                let command = fs::read(self.proc.join(pid.to_string()).join("cmdline")).unwrap_or_default();
                let words: String = command.iter().map(|byte| if *byte == 0 { ' ' } else { *byte as char }).collect();
                format!("  {pid} {}", words.chars().take(160).collect::<String>())
            })
            .collect()
    }
}

fn parse_stat(stat: &[u8]) -> Option<(char, u32)> {
    let text = String::from_utf8_lossy(stat);
    let mut fields = text.rsplit_once(')')?.1.split_whitespace();
    Some((fields.next()?.chars().next()?, fields.next()?.parse().ok()?))
}

fn holds(path: &Path, wanted: &str) -> bool {
    fs::read(path).is_ok_and(|bytes| bytes.split(|byte| *byte == 0).any(|entry| entry == wanted.as_bytes()))
}

fn signal(pids: &BTreeSet<u32>, signal: Signal) {
    for pid in pids {
        if let Some(pid) = i32::try_from(*pid).ok().and_then(Pid::from_raw) {
            let _ = kill_process(pid, signal);
        }
    }
}

pub fn close(launch: &Launch, timing: &Timing, out: &mut dyn FnMut(&str), err: &mut dyn FnMut(&str)) -> u8 {
    let pids = launch.members();
    if pids.is_empty() {
        out("warcraft-close: nothing running");
        return 0;
    }
    signal(&pids, Signal::Term);
    for _ in 0..timing.polls {
        sleep(timing.poll);
        if launch.members().is_empty() {
            break;
        }
    }
    signal(&launch.members(), Signal::Kill);
    sleep(timing.settle);
    let left = launch.members();
    if left.is_empty() {
        out(&format!("warcraft-close: closed {} processes of Tom's Steam launch; a process check finds none left", pids.len()));
        return 0;
    }
    err("warcraft-close: FAILED: these processes of Tom's Steam launch survived KILL, so the next Play may do nothing:");
    launch.describe(&left).iter().for_each(|line| err(line));
    1
}

pub fn run(arguments: &[String]) -> u8 {
    let launch = Launch::from_env();
    match arguments {
        [] => close(&launch, &Timing { poll: Duration::from_millis(500), polls: 20, settle: Duration::from_secs(1) }, &mut |line| println!("{line}"), &mut |line| eprintln!("{line}")),
        [flag] if flag == "--list" => {
            println!("{}", launch.members().iter().map(u32::to_string).collect::<Vec<_>>().join(" "));
            0
        }
        _ => {
            eprintln!("usage: north warcraft-close [--list]");
            2
        }
    }
}

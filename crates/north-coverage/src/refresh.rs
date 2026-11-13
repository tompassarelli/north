use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::map::{self, Map};
use crate::take::{self, Recorded};
use crate::run_selected::{host, north, repository};

const TIMEOUT: &str = "3600";
const RECORD: &str = "CARGO_TARGET_DIR=target ./bin/land-shell --dev -- ./bin/head-north --code crates/north-coverage/ test coverage record";

pub fn run(arguments: &[String]) -> u8 {
    let mut host_name = None;
    let mut head = "origin/main".to_owned();
    let mut rest = arguments.iter();
    while let Some(flag) = rest.next() {
        match (flag.as_str(), rest.next()) {
            ("--host", Some(name)) => host_name = Some(name.clone()),
            ("--head", Some(revision)) => head.clone_from(revision),
            ("--repo", Some(name)) => {
                if let Err(why) = enter(name) {
                    eprintln!("north test coverage refresh: {why}");
                    return 1;
                }
            }
            _ => {
                eprintln!("usage: north test coverage refresh [--repo NAME] [--host NAME] [--head REVISION] | --start [--unknown N --listed M]");
                return 2;
            }
        }
    }
    match refresh(&host_name.unwrap_or_else(host), &head) {
        Ok(map) => {
            println!("north test coverage: {} tests reach {} functions at {}", map.tests.len(), map.functions.len(), map.commit);
            0
        }
        Err(why) => {
            eprintln!("north test coverage refresh: {why}");
            1
        }
    }
}

pub(crate) fn checkout(repository: &str) -> PathBuf {
    Path::new(&std::env::var_os("HOME").unwrap_or_default()).join("code").join(repository).join("main")
}

fn enter(repository: &str) -> Result<(), String> {
    let checkout = checkout(repository);
    std::env::set_current_dir(&checkout).map_err(|error| format!("cannot enter {}: {error}", checkout.display()))?;
    let fetched = Command::new("git").args(["fetch", "--quiet", "origin", "main"]).status().map_err(|error| format!("git fetch: {error}"))?;
    if fetched.success() { Ok(()) } else { Err(format!("git fetch origin main failed in {}", checkout.display())) }
}

fn refresh(host: &str, head: &str) -> Result<Map, String> {
    let root = Path::new(".");
    let _running = Running::claim(&map::path_for(&repository(root)).with_extension("refreshing"))?;
    let mut child = Command::new(north())
        .args(["capacity", "offload", "run", "--host", host, "--base", "", "--head", head, "--timeout", TIMEOUT, "--", RECORD])
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|error| format!("north capacity offload run: {error}"))?;
    let stdout = child.stdout.take().ok_or("the offload has no output")?;
    let recorded = Recorded::parse(BufReader::new(stdout).lines(), |line| eprintln!("{line}"))?;
    let status = child.wait().map_err(|error| format!("north capacity offload run: {error}"))?;
    let recorded = recorded.ok_or_else(|| format!("the record on {host} printed no map ({status})"))?;
    let repository = repository(root);
    let payload = take::path_for(&repository);
    recorded.save(&payload)?;
    recorded.install(&repository)?;
    crate::cadence::ingest(root, &payload, &repository);
    Ok(recorded.map)
}

struct Running(std::path::PathBuf);

impl Running {
    fn claim(path: &Path) -> Result<Self, String> {
        if let Some(directory) = path.parent() {
            std::fs::create_dir_all(directory).map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
        }
        for _ in 0..2 {
            match std::fs::OpenOptions::new().write(true).create_new(true).open(path) {
                Ok(mut file) => {
                    std::io::Write::write_all(&mut file, std::process::id().to_string().as_bytes()).map_err(|error| format!("cannot write {}: {error}", path.display()))?;
                    return Ok(Self(path.to_owned()));
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if let Some(pid) = holder(path) {
                        return Err(format!("a refresh is already running as pid {pid}"));
                    }
                    let _ = std::fs::remove_file(path);
                }
                Err(error) => return Err(format!("cannot create {}: {error}", path.display())),
            }
        }
        Err(format!("cannot claim {}", path.display()))
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn holder(path: &Path) -> Option<u32> {
    let pid = std::fs::read_to_string(path).ok()?.trim().parse::<u32>().ok()?;
    let command = std::fs::read(Path::new("/proc").join(pid.to_string()).join("cmdline")).ok()?;
    let words: Vec<&[u8]> = command.split(|byte| *byte == 0).collect();
    words.windows(2).any(|pair| pair == [&b"coverage"[..], &b"refresh"[..]]).then_some(pid)
}

#[cfg(test)]
#[path = "refresh_tests.rs"]
mod tests;

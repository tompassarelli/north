use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};

use crate::map::{self, Map};
use crate::record::MAP_LINE;
use crate::run_selected::{host, north, repository};

const TIMEOUT: &str = "5400";
const RECORD: &str = "CARGO_TARGET_DIR=target ./bin/land-shell --dev -- ./bin/head-north --code crates/north-coverage/ coverage record";

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
                    eprintln!("north coverage refresh: {why}");
                    return 1;
                }
            }
            _ => {
                eprintln!("usage: north coverage refresh [--repo NAME] [--host NAME] [--head REVISION]");
                return 2;
            }
        }
    }
    match refresh(&host_name.unwrap_or_else(host), &head) {
        Ok(map) => {
            println!("north coverage: {} tests reach {} functions at {}", map.tests.len(), map.functions.len(), map.commit);
            0
        }
        Err(why) => {
            eprintln!("north coverage refresh: {why}");
            1
        }
    }
}

fn enter(repository: &str) -> Result<(), String> {
    let checkout = Path::new(&std::env::var_os("HOME").unwrap_or_default()).join("code").join(repository).join("main");
    std::env::set_current_dir(&checkout).map_err(|error| format!("cannot enter {}: {error}", checkout.display()))?;
    let fetched = Command::new("git").args(["fetch", "--quiet", "origin", "main"]).status().map_err(|error| format!("git fetch: {error}"))?;
    if fetched.success() { Ok(()) } else { Err(format!("git fetch origin main failed in {}", checkout.display())) }
}

fn refresh(host: &str, head: &str) -> Result<Map, String> {
    let root = Path::new(".");
    let mut child = Command::new(north())
        .args(["capacity", "offload", "run", "--host", host, "--base", "", "--head", head, "--timeout", TIMEOUT, "--", RECORD])
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|error| format!("north capacity offload run: {error}"))?;
    let stdout = child.stdout.take().ok_or("the offload has no output")?;
    let mut recorded = None;
    for line in BufReader::new(stdout).lines() {
        let line = line.map_err(|error| format!("cannot read the offload's output: {error}"))?;
        match line.strip_prefix(MAP_LINE) {
            Some(text) => recorded = Some(serde_json::from_str::<Map>(text).map_err(|error| format!("cannot parse the recorded map: {error}"))?),
            None => eprintln!("{line}"),
        }
    }
    let status = child.wait().map_err(|error| format!("north capacity offload run: {error}"))?;
    let map = recorded.ok_or_else(|| format!("the record on {host} printed no map ({status})"))?;
    map.save(&map::path_for(&repository(root)))?;
    Ok(map)
}

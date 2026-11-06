use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;

use north_graph::Telling;

use crate::facts::{Facts, text};
use crate::places::Places;

const WRITER: &str = "git";
const WINDOW_DAYS: u64 = 30;
const COMMIT_MARK: &str = "\u{1e}";

pub fn read(places: &Places) -> Result<Telling, String> {
    let now_ms = north_base::foundation::clock::Timestamp::now().unix_seconds_f64() as u64 * 1_000;
    let from_ms = now_ms.saturating_sub(WINDOW_DAYS * 86_400_000);
    let mut landed: HashMap<String, HashSet<String>> = HashMap::new();
    for push in north_fleet::fleet::landings::pushes(&places.pushes).into_iter().filter(|push| !push.landed.is_empty() && push.at_ms >= from_ms) {
        landed.entry(push.repository).or_default().insert(push.landed.to_ascii_lowercase());
    }
    let mut facts = Facts::new(&[]);
    let mut read_any = false;
    for (repository, commits) in &landed {
        let checkout = places.code.join(north_base::platform::repo_names::checkout_name(repository)).join("main");
        if !checkout.join(".git").exists() {
            continue;
        }
        let log = log(&checkout, WINDOW_DAYS)?;
        read_any = true;
        for (commit, files) in parse(&log).into_iter().filter(|(commit, _)| commits.contains(commit)) {
            for file in files {
                facts.tell(WRITER, text(&commit), "touched", text(file));
            }
        }
    }
    if read_any || landed.is_empty() { Ok(facts.finish()) } else { Err(format!("no checkout under {} for the repositories the push log names", places.code.display())) }
}

fn log(checkout: &Path, days: u64) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(checkout)
        .args(["log", "--no-renames", "--name-only", &format!("--since={days}.days"), &format!("--format={COMMIT_MARK}%H")])
        .output()
        .map_err(|why| format!("git: {why}"))?;
    if !output.status.success() {
        return Err(format!("{}: git log: {}", checkout.display(), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn parse(log: &str) -> Vec<(String, Vec<&str>)> {
    log.split(COMMIT_MARK)
        .filter_map(|block| {
            let mut lines = block.lines();
            let commit = lines.next()?.trim().to_ascii_lowercase();
            (!commit.is_empty()).then(|| (commit, lines.map(str::trim).filter(|line| !line.is_empty()).collect()))
        })
        .collect()
}

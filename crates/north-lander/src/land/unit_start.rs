use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use north_base::platform::land_notice::OWNER;

use super::after::send_condition;

const FAILED_STARTS: u32 = 3;
const PROXY: &str = "proxy";
const CAUSE_LINES: &str = "3";
const EPISODES: &str = "lander-unit-start";
const HEALTHY_FOR: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, PartialEq, Eq)]
struct Start {
    state: String,
    restarts: u32,
    result: String,
}

fn parse(text: &str) -> Start {
    let field = |name: &str| text.lines().find_map(|line| line.strip_prefix(name)).map(str::trim).unwrap_or_default().to_owned();
    Start { state: field("ActiveState="), restarts: field("NRestarts=").parse().unwrap_or(0), result: field("Result=") }
}

/// A unit systemd keeps restarting without it running: it has been restarted `FAILED_STARTS` times and is between starts or failed, never active. A unit whose last run ended cleanly (`Result=success`) is restarting onto a changed live runtime, which is not a failure to start, though `NRestarts` counts it.
fn cannot_start(start: &Start) -> bool {
    start.restarts >= FAILED_STARTS && start.result != "success" && matches!(start.state.as_str(), "activating" | "failed")
}

fn message(unit: &str, restarts: u32, cause: &str) -> String {
    format!("north land: {unit} has failed to start {restarts} times and is not serving its queue, so nothing lands there: {cause}")
}

fn output(program: &str, arguments: &[&str]) -> Option<String> {
    let ran = Command::new(program).args(arguments).stdin(Stdio::null()).stderr(Stdio::null()).output().ok().filter(|ran| ran.status.success())?;
    Some(String::from_utf8_lossy(&ran.stdout).trim().to_owned())
}

fn cause(unit: &str) -> String {
    let journal = output("journalctl", &["--user", "-u", unit, "-n", CAUSE_LINES, "-o", "cat", "--no-pager"]).unwrap_or_default();
    let lines: Vec<&str> = journal.lines().map(str::trim).filter(|line| !line.is_empty()).collect();
    if lines.is_empty() { "its journal names no cause".to_owned() } else { lines.join(" | ") }
}

fn seconds(at: SystemTime) -> u64 {
    at.duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

/// The start of the run of failures `unit` is in, kept in a file the run refreshes while it lasts: the unit's own timestamps move with every restart, so only this file names one condition for the whole run, and it ends once the unit has not failed for `HEALTHY_FOR`.
fn episode(home: &Path, unit: &str, failing: bool, now: SystemTime) -> Option<u64> {
    let file = home.join(EPISODES).join(unit);
    let began = fs::read_to_string(&file).ok().and_then(|text| text.trim().parse().ok());
    if !failing {
        let quiet = fs::metadata(&file).and_then(|metadata| metadata.modified()).ok().and_then(|at| now.duration_since(at).ok());
        if quiet.is_some_and(|quiet| quiet >= HEALTHY_FOR) {
            let _ = fs::remove_file(&file);
        }
        return None;
    }
    let began = began.unwrap_or(seconds(now));
    let _ = fs::create_dir_all(home.join(EPISODES));
    let _ = fs::write(&file, format!("{began}\n"));
    Some(began)
}

/// Pages the proxy and the lander's lead once for each of `units` that has failed to start three times, with the journal's last lines as the cause; the fleet store delivers one page per run of failures.
pub fn page(home: &Path, units: &[String]) {
    for unit in units {
        let Some(text) = output("systemctl", &["--user", "show", "-p", "ActiveState,NRestarts,Result", unit]) else {
            println!("north land: could not read the state of {unit}, so a failure to start would go unpaged");
            continue;
        };
        let start = parse(&text);
        let Some(began) = episode(home, unit, cannot_start(&start), SystemTime::now()) else { continue };
        let (key, text) = (format!("lander-unit-start:{unit}:{began}"), message(unit, start.restarts, &cause(unit)));
        let told = [PROXY, OWNER].map(|session| send_condition(session, &key, false, &text));
        if told.contains(&false) {
            println!("north land: the page about {unit} failing to start reached only part of its targets");
        }
    }
}

//! What a dispatch run does to the world: start each placed box through `north fleet launch` (on
//! the laptop through its job queue), record the attempt, and send Tom's one ntfy line.
use super::brief;
use super::plan::Launch;
use super::record::{self, Record};
use super::survey::Survey;
use crate::ops::pacing::placement;
use crate::platform::process::run_within;
use serde_json::json;
use std::time::Duration;

const LAUNCH_WAIT: Duration = Duration::from_secs(180);
const REPEAT_ALERT_S: f64 = 6.0 * 3600.0;

/// `sc-277-d1435`: the session name `north fleet list` shows and the hold reads back.
pub(crate) fn session_name(launch: &Launch, at: f64) -> String {
    let short = if launch.item.repo == "smashcraft" { "sc" } else { launch.item.repo.as_str() };
    let clock = crate::foundation::clock::Timestamp::from_unix_seconds_f64(at).iso_micros_utc_offset();
    let hhmm: String = clock.get(11..16).unwrap_or_default().chars().filter(char::is_ascii_digit).collect();
    format!("{short}-{}-d{hhmm}", launch.item.number)
}

/// The program and arguments that start `launch`: `north fleet launch` here or on an ssh host, or
/// a laptop job that runs it there.
pub(crate) fn command(launch: &Launch, name: &str, survey: &Survey, home: &str, north: &str) -> (String, Vec<String>) {
    let directory = format!("{home}/code/{}", launch.item.repo);
    let remote = launch.host != survey.here && survey.laptop.as_deref() != Some(launch.host.as_str());
    let cwd = if remote { format!("{}:{directory}", launch.host) } else { directory };
    let (provider, label) = launch.account.split_once(':').unwrap_or((launch.account.as_str(), "pooled"));
    let mut words: Vec<String> = ["fleet", "launch", "--provider", provider, "--account", label, "--name", name, "--task", &launch.item.to_string(), "--cwd", &cwd]
        .map(str::to_owned)
        .into();
    for (flag, reason) in [("--local", &launch.local), ("--pin", &launch.pin)] {
        if let Some(reason) = reason {
            words.extend([flag.to_owned(), reason.clone()]);
        }
    }
    words.push(if launch.courier { brief::courier(&launch.brief, name) } else { launch.brief.clone() });
    if survey.laptop.as_deref() == Some(launch.host.as_str()) {
        let mut queued = ["queue", "add", "laptop", &launch.item.to_string(), "--", "north"].map(str::to_owned).to_vec();
        queued.extend(words);
        return ("threads".into(), queued);
    }
    (north.into(), words)
}

pub(crate) fn launch(survey: &Survey, launch: &Launch) -> Result<String, String> {
    let at = placement::now();
    let name = session_name(launch, at);
    let home = std::env::var("HOME").map_err(|_| "HOME is unset".to_owned())?;
    let north = std::env::current_exe().map_err(|error| error.to_string())?;
    let (program, words) = command(launch, &name, survey, &home, &north.to_string_lossy());
    let arguments: Vec<&str> = words.iter().map(String::as_str).collect();
    let result = match run_within(&program, &arguments, LAUNCH_WAIT) {
        None => Err(format!("{program} did not return within {} s", LAUNCH_WAIT.as_secs())),
        Some(output) if output.status.success() => Ok(name.clone()),
        Some(output) => Err(String::from_utf8_lossy(&output.stderr).lines().find(|line| !line.trim().is_empty() && !line.starts_with("  ")).unwrap_or("failed").to_owned()),
    };
    let entry = Record {
        at,
        item: launch.item.to_string(),
        host: launch.host.clone(),
        account: launch.account.clone(),
        name,
        courier: launch.courier,
        launched: result.is_ok(),
        detail: result.as_ref().err().cloned().unwrap_or_default(),
    };
    record::append(&entry)?;
    result
}

/// Sends `line` through nexus-alert unless the same line went out in the last six hours.
pub(crate) fn tell_tom(line: &str, dry_run: bool, at: f64) {
    let path = record::directory().join("alert.json");
    let last = placement::pace_state("dispatch/alert.json");
    if last["text"] == line && last["at"].as_f64().is_some_and(|sent| at - sent < REPEAT_ALERT_S) {
        println!("alert already sent: {line}");
        return;
    }
    if dry_run {
        println!("would alert: {line}");
        return;
    }
    match run_within("nexus-alert", &["normal", line, "north dispatch"], Duration::from_secs(30)) {
        Some(output) if output.status.success() => {
            println!("alerted: {line}");
            let saved = serde_json::to_vec(&json!({"text": line, "at": at})).map_err(|error| error.to_string());
            if let Err(why) = saved.and_then(|bytes| std::fs::write(&path, bytes).map_err(|error| error.to_string())) {
                eprintln!("north dispatch: alert not recorded: {why}");
            }
        }
        _ => eprintln!("north dispatch: nexus-alert failed; the line was: {line}"),
    }
}

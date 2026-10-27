//! What a dispatch run does to the world: start each placed box in the cloud lane or through
//! `north fleet launch` (on the laptop through its job queue), record the attempt, send Tom's one
//! ntfy line, and send the triage lead its daily list of issues that need a box.
use super::{brief, cloud};
use super::issues::Issue;
use super::plan::Launch;
use super::record::{self, Record};
use super::survey::Survey;
use crate::ops::pacing::placement;
use crate::platform::process::run_within;
use serde_json::json;
use std::path::Path;
use std::time::Duration;
use toml_edit::{DocumentMut, Item};

const LAUNCH_WAIT: Duration = Duration::from_secs(180);
const REPEAT_ALERT_S: f64 = 6.0 * 3600.0;
const BOXLESS_EVERY_S: f64 = 24.0 * 3600.0;

/// `sc-277-d1435`: the session name `north fleet list` shows and the hold reads back.
pub(crate) fn session_name(launch: &Launch, at: f64) -> String {
    let short = if launch.item.repo == "smashcraft" { "sc" } else { launch.item.repo.as_str() };
    let clock = crate::foundation::clock::Timestamp::from_unix_seconds_f64(at).iso_micros_utc_offset();
    let hhmm: String = clock.get(11..16).unwrap_or_default().chars().filter(char::is_ascii_digit).collect();
    format!("{short}-{}-d{hhmm}", launch.item.number)
}

/// The program and arguments that start `launch`: `north fleet launch` here or on an ssh host, or
/// a laptop job that runs it there.
pub(crate) fn command(launch: &Launch, name: &str, survey: &Survey, home: &str, north: &str) -> Result<(String, Vec<String>), String> {
    let directory = format!("{home}/code/{}/main", launch.item.repo);
    let remote = launch.host != survey.here && survey.laptop.as_deref() != Some(launch.host.as_str());
    let cwd = if remote { format!("{}:{directory}", launch.host) } else { directory };
    let (provider, label) = launch.account.split_once(':').unwrap_or((launch.account.as_str(), "pooled"));
    let mut words: Vec<String> = ["fleet", "launch", "--provider", provider, "--account", label, "--name", name, "--task", &launch.item.to_string(), "--cwd", &cwd]
        .map(str::to_owned)
        .into();
    words.extend(launch.route.tier.invocation(&survey.cfg)?);
    let held = (!remote && launch.local.is_none()).then(|| format!("north dispatch placed this box on {}", launch.host));
    for (flag, reason) in [("--local", launch.local.as_ref().or(held.as_ref())), ("--pin", launch.pin.as_ref())] {
        if let Some(reason) = reason {
            words.extend([flag.to_owned(), reason.clone()]);
        }
    }
    words.push(launch.brief.clone());
    if survey.laptop.as_deref() == Some(launch.host.as_str()) {
        let mut queued = ["queue", "add", "laptop", &launch.item.to_string(), "--", "north"].map(str::to_owned).to_vec();
        queued.extend(words);
        return Ok(("threads".into(), queued));
    }
    Ok((north.into(), words))
}

pub(crate) fn launch(survey: &Survey, launch: &Launch) -> Result<String, String> {
    let at = placement::now();
    let name = session_name(launch, at);
    let home = std::env::var("HOME").map_err(|_| "HOME is unset".to_owned())?;
    let result = if launch.cloud {
        let text = brief::cloud(&launch.brief, &launch.item.repo, &cloud::branch(&name));
        let started = launch.route.tier.invocation(&survey.cfg).and_then(|model| cloud::launch(&launch.item.repo, &name, &text, &model, Path::new(cloud::SOURCES), Path::new(&home), &mut cloud::run));
        started.map(|launched| format!("{name} {} {}", launched.session, launched.branch))
    } else {
        fleet_launch(launch, &name, survey, &home)
    };
    let entry = Record {
        at,
        item: launch.item.to_string(),
        host: launch.host.clone(),
        account: launch.account.clone(),
        name,
        cloud: launch.cloud,
        launched: result.is_ok(),
        detail: result.clone().unwrap_or_else(|why| why),
        tier: Some(launch.route.tier.name().to_owned()),
        category: Some(launch.route.category.clone()),
        arm: launch.route.arm,
        handoff: launch.handoff.clone(),
    };
    record::append(&record::directory(), &entry)?;
    result
}

fn fleet_launch(launch: &Launch, name: &str, survey: &Survey, home: &str) -> Result<String, String> {
    let north = std::env::current_exe().map_err(|error| error.to_string())?;
    let (program, words) = command(launch, name, survey, home, &north.to_string_lossy())?;
    let arguments: Vec<&str> = words.iter().map(String::as_str).collect();
    match run_within(&program, &arguments, LAUNCH_WAIT) {
        None => Err(format!("{program} did not return within {} s", LAUNCH_WAIT.as_secs())),
        Some(output) if output.status.success() => Ok(name.to_owned()),
        Some(output) => Err(String::from_utf8_lossy(&output.stderr).lines().find(|line| !line.trim().is_empty() && !line.starts_with("  ")).unwrap_or("failed").to_owned()),
    }
}

/// Sends `line` through nexus-alert unless the same line went out in the last six hours.
pub(crate) fn tell_tom(line: &str, dry_run: bool, at: f64) {
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
            remember("alert.json", &json!({"text": line, "at": at}));
        }
        _ => eprintln!("north dispatch: nexus-alert failed; the line was: {line}"),
    }
}

/// The message that asks the triage lead to box the open issues that have none.
pub(crate) fn boxless_summary(issues: &[&Issue]) -> String {
    let listed: Vec<String> = issues.iter().map(|issue| format!("{} {}", issue.reference(), issue.title)).collect();
    format!("north dispatch: {} open issues have no Done-when box, so dispatch cannot staff them; give each a box, a roadmap label, or close it: {}", issues.len(), listed.join("; "))
}

/// Sends `boxless_summary` to the session `[dispatch] triage_lead` names, at most once a day.
pub(crate) fn ask_for_boxes(cfg: &DocumentMut, issues: &[&Issue], dry_run: bool, at: f64) {
    let Some(lead) = cfg.get("dispatch").and_then(|rules| rules.get("triage_lead")).and_then(Item::as_str) else {
        println!("note: no [dispatch] triage_lead, so nobody is asked to box them");
        return;
    };
    let last = placement::pace_state("dispatch/boxless.json");
    if let Some(sent) = last["at"].as_f64().filter(|sent| at - sent < BOXLESS_EVERY_S) {
        println!("needs-a-box summary went to {lead} {:.0} h ago", (at - sent) / 3600.0);
        return;
    }
    let text = boxless_summary(issues);
    if dry_run {
        println!("would send {lead}: {text}");
        return;
    }
    let north = std::env::current_exe().map(|path| path.to_string_lossy().into_owned()).unwrap_or_else(|_| "north".into());
    match run_within(&north, &["fleet", "send", lead, &text], Duration::from_secs(30)) {
        Some(output) if output.status.success() => {
            println!("sent {lead}: {text}");
            remember("boxless.json", &json!({"to": lead, "at": at}));
        }
        _ => eprintln!("north dispatch: north fleet send {lead} failed; the summary was: {text}"),
    }
}

fn remember(file: &str, value: &serde_json::Value) {
    let saved = std::fs::create_dir_all(record::directory()).map_err(|error| error.to_string()).and_then(|()| serde_json::to_vec(value).map_err(|error| error.to_string()));
    if let Err(why) = saved.and_then(|bytes| std::fs::write(record::directory().join(file), bytes).map_err(|error| error.to_string())) {
        eprintln!("north dispatch: {file} not recorded: {why}");
    }
}

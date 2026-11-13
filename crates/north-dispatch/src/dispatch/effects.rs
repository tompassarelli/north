//! What a dispatch run does to the world: start each placed box through `north fleet launch` (on the laptop through
//! its job queue), record the attempt, send Tom's one ntfy line, and send the triage lead, else the proxy, its daily
//! list of issues that need a box.
use super::issues::Issue;
use super::plan::Launch;
use super::revive::Revive;
use super::record::{self, Record};
use super::survey::Survey;
use crate::pacing::placement::{self, gate};
use crate::fleet::runtime::PROXY;
use crate::platform::attribution::{self, Cause, Harness};
use crate::platform::process::run_within;
use serde_json::json;
use std::collections::BTreeSet;
use std::time::Duration;
use toml_edit::{DocumentMut, Item};

const LAUNCH_WAIT: Duration = Duration::from_secs(180);
const PREDICT_WAIT: Duration = Duration::from_secs(30);
const REPEAT_ALERT_S: f64 = 6.0 * 3600.0;
const BOXLESS_EVERY_S: f64 = 24.0 * 3600.0;
const SENDER: &str = "service:north-dispatch";
pub const REVIVED: &str = "revived.json";

/// `sc-277-d1435`: the session name `north fleet list` shows and the hold reads back, with the box
/// slug when the launch staffs one box so two boxes of an issue started in one minute take two names.
pub fn session_name(launch: &Launch, at: f64) -> String {
    let short = if launch.item.repo == "makgora" { "sc" } else { launch.item.repo.as_str() };
    let clock = crate::foundation::clock::Timestamp::from_unix_seconds_f64(at).iso_micros_utc_offset();
    let hhmm: String = clock.get(11..16).unwrap_or_default().chars().filter(char::is_ascii_digit).collect();
    let name = format!("{short}-{}-d{hhmm}", launch.item.number);
    match super::facets::declared_in(&launch.brief, "Box").and_then(crate::threads::claims::box_slug) {
        Some(slug) => format!("{name}-{slug}"),
        None => name,
    }
}

/// The program and arguments that start `launch`: `north fleet launch` here or on an ssh host, or
/// a laptop job that runs it there, queued through north itself because the dispatch unit's PATH has no `threads` shim.
pub fn command(launch: &Launch, name: &str, survey: &Survey, home: &str, north: &str) -> Result<(String, Vec<String>), String> {
    command_in(launch, name, survey, north, format!("{home}/code/{}/main", crate::threads::catalog::checkout(&launch.item.repo)))
}

pub(super) fn command_in(launch: &Launch, name: &str, survey: &Survey, north: &str, directory: String) -> Result<(String, Vec<String>), String> {
    let remote = launch.host != survey.here && survey.laptop.as_deref() != Some(launch.host.as_str());
    let cwd = if remote { format!("{}:{directory}", launch.host) } else { directory };
    let (provider, label) = launch.account.split_once(':').unwrap_or((launch.account.as_str(), "pooled"));
    let mut words: Vec<String> = ["fleet", "launch", "--provider", provider, "--account", label, "--name", name, "--task", &launch.item.to_string(), "--cwd", &cwd]
        .map(str::to_owned)
        .into();
    words.extend(super::deepseek::invocation(launch, &survey.cfg)?);
    let held = (!remote && launch.local.is_none()).then(|| format!("north fleet dispatch placed this box on {}", launch.host));
    if let Some(reason) = launch.local.as_ref().or(held.as_ref()) {
        words.extend(["--local".to_owned(), reason.clone()]);
    }
    words.push(launch.brief.clone());
    if survey.laptop.as_deref() == Some(launch.host.as_str()) {
        let mut queued = ["threads", "queue", "add", "laptop", &launch.item.to_string(), "--", "north", "fleet", "dispatch", "queued", "--by", &survey.here, "--item", &launch.item.to_string()].map(str::to_owned).to_vec();
        if let Some(text) = super::facets::declared_in(&launch.brief, "Box") {
            queued.extend(["--box".to_owned(), text.to_owned()]);
        }
        queued.push("--".to_owned());
        queued.extend(words);
        return Ok((north.into(), queued));
    }
    Ok((north.into(), words))
}

pub fn launch(survey: &Survey, launch: &Launch) -> Result<String, String> {
    let at = placement::now();
    let name = session_name(launch, at);
    let home = std::env::var("HOME").map_err(|_| "HOME is unset".to_owned())?;
    let result = fleet_launch(launch, &name, survey, &home);
    let cell = super::deepseek::cell(launch, &survey.cfg);
    let entry = Record {
        at,
        item: launch.item.to_string(),
        host: launch.host.clone(),
        account: launch.account.clone(),
        name,
        cloud: false,
        launched: result.is_ok(),
        detail: result.clone().unwrap_or_else(|why| why),
        tier: super::deepseek::tier(launch),
        category: Some(launch.route.category.clone()),
        box_text: super::facets::declared_in(&launch.brief, "Box").map(str::to_owned),
        hand: false,
        effort_arm: launch.route.stamp().map(str::to_owned),
        signal: launch.route.signal.map(|signal| signal.key().to_owned()),
        lift: None,
        effort: launch.route.effort.map(|effort| effort.name().to_owned()),
        cell,
        cell_role: launch.route.cell_role.map(str::to_owned),
        route: None,
    };
    record::append(&record::directory(), &entry)?;
    if result.is_ok() && !super::deepseek::is(launch) {
        predict(launch);
    } else if entry.failed() {
        attribution::record(&entry.name, "dispatch", Cause::Harness(Harness::Placement), &format!("{}: {}", launch.host, entry.detail));
    }
    result
}

/// Stores the launched tier's shrunk success probability for the box's first run, so `north tune` scores it; a
/// failed store is printed and leaves the launch standing.
fn predict(launch: &Launch) {
    let north = crate::fleet::runtime::binary("north");
    let words = ["threads", "recommend", "--launched", launch.route.tier.key(), &launch.brief];
    match run_within(&north.to_string_lossy(), &words, PREDICT_WAIT) {
        Some(output) if output.status.success() => {}
        Some(output) => println!("prediction for {} unstored: {}", launch.item, refusal_line(&String::from_utf8_lossy(&output.stderr))),
        None => println!("prediction for {} unstored: north threads recommend did not return within {} s", launch.item, PREDICT_WAIT.as_secs()),
    }
}

fn fleet_launch(launch: &Launch, name: &str, survey: &Survey, home: &str) -> Result<String, String> {
    let north = std::env::current_exe().map_err(|error| error.to_string())?;
    started(name, command(launch, name, survey, home, &north.to_string_lossy())?)
}

pub(super) fn fleet_launch_in(launch: &Launch, name: &str, survey: &Survey, directory: String) -> Result<String, String> {
    let north = std::env::current_exe().map_err(|error| error.to_string())?;
    started(name, command_in(launch, name, survey, &north.to_string_lossy(), directory)?)
}

fn started(name: &str, (program, words): (String, Vec<String>)) -> Result<String, String> {
    let mut child = std::process::Command::new(&program);
    child.args(&words).env_remove(crate::fleet::runtime::SESSION_ENV).env_remove(crate::fleet::runtime::ROLE_ENV);
    match crate::platform::process::try_run_command_within(child, None, LAUNCH_WAIT) {
        Err(error) => {
            let why = format!("{program} did not start: {error}");
            Err(failed_launch(name, why.clone(), &why))
        }
        Ok(None) => {
            let why = format!("{program} did not return within {} s", LAUNCH_WAIT.as_secs());
            Err(failed_launch(name, why.clone(), &why))
        }
        Ok(Some(output)) if output.status.success() => Ok(name.to_owned()),
        Ok(Some(output)) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(failed_launch(name, refusal_line(&stderr), &stderr))
        }
    }
}

fn failed_launch(name: &str, why: String, stderr: &str) -> String {
    let path = record::stderr_path(name);
    if let Some(parent) = path.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        eprintln!("north fleet dispatch: {} not written: {error}", path.display());
    }
    if let Err(error) = std::fs::write(&path, stderr) {
        eprintln!("north fleet dispatch: {} not written: {error}", path.display());
    }
    why
}

/// Revives a stranded session through `north fleet revive`, whose placement admits it as it would a
/// launch, then gives it the turn that resumes its lane, and records the report it answered.
pub fn revive(survey: &Survey, revive: &Revive) -> Result<(), String> {
    let mut words = vec!["fleet".to_owned(), "revive".into(), revive.name.clone()];
    if let Some(reason) = &revive.local {
        words.extend(["--local".to_owned(), reason.clone()]);
    }
    north_on(survey, &revive.registry, &words)?;
    north_on(survey, &revive.registry, &["fleet".into(), "send".into(), "--from".into(), "service:north-dispatch".into(), revive.name.clone(), super::revive::prompt(revive)])?;
    let mut revived = placement::pace_state(&format!("dispatch/{REVIVED}"));
    revived[&revive.name] = json!(revive.ended_ms);
    remember(REVIVED, &revived);
    Ok(())
}

pub fn forget(survey: &Survey, name: &str, registry: &str) -> Result<(), String> {
    north_on(survey, registry, &["fleet".into(), "forget".into(), name.to_owned()])
}

/// Runs `north WORDS` on `host`: here, or over the ssh destination its `[hosts]` entry names.
pub(super) fn north_on(survey: &Survey, host: &str, words: &[String]) -> Result<(), String> {
    let north = std::env::current_exe().map_err(|error| error.to_string())?.to_string_lossy().into_owned();
    let output = if host == survey.here {
        run_within(&north, &words.iter().map(String::as_str).collect::<Vec<_>>(), LAUNCH_WAIT)
    } else {
        let machine = gate::machines(&survey.cfg).into_iter().find(|machine| machine.name == host).ok_or_else(|| format!("no ssh destination for {host}"))?;
        let command = std::iter::once("north".to_owned()).chain(words.iter().map(|word| quoted(word))).collect::<Vec<_>>().join(" ");
        run_within("ssh", &["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", machine.session_login(), &command], LAUNCH_WAIT)
    };
    match output {
        None => Err(format!("north {} did not return within {} s", words[..2].join(" "), LAUNCH_WAIT.as_secs())),
        Some(output) if output.status.success() => Ok(()),
        Some(output) => Err(refusal_line(&String::from_utf8_lossy(&output.stderr))),
    }
}

fn quoted(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}

/// The error `north fleet launch` ended on, its `error:` line, else the last unindented line:
/// the lines before it are the `tier:` and `placement:` notes of a launch that got that far.
pub fn refusal_line(stderr: &str) -> String {
    let lines = || stderr.lines().filter(|line| !line.trim().is_empty() && !line.starts_with("  "));
    let error = lines().filter_map(|line| line.strip_prefix("error: ")).next_back();
    error.or_else(|| lines().next_back()).unwrap_or("failed").to_owned()
}

#[derive(Debug, PartialEq)]
pub enum Delivery {
    Alert,
    Page,
}

pub fn deliver(line: &str, alert: &mut dyn FnMut(&str) -> Result<(), String>, page: &mut dyn FnMut(&str) -> Result<(), String>) -> Result<Delivery, String> {
    match alert(line) {
        Ok(()) => Ok(Delivery::Alert),
        Err(why) => match page(line) {
            Ok(()) => Ok(Delivery::Page),
            Err(page) => Err(format!("nexus-alert: {why}; the proxy page: {page}")),
        },
    }
}

pub fn page_proxy(line: &str) -> Result<(), String> {
    fleet_send(&["fleet", "send", "--now", "--from", SENDER, PROXY, line])
}

/// Pages the proxy under the fleet store's condition `key`, which it delivers once per episode.
pub fn page_condition(key: &str, line: &str) -> Result<(), String> {
    fleet_send(&["fleet", "send", "--now", "--from", SENDER, "--key", key, PROXY, line])
}

fn fleet_send(arguments: &[&str]) -> Result<(), String> {
    let north = std::env::current_exe().map(|path| path.to_string_lossy().into_owned()).unwrap_or_else(|_| "north".into());
    match run_within(&north, arguments, Duration::from_secs(30)) {
        Some(output) if output.status.success() => Ok(()),
        Some(output) => Err(refusal_line(&String::from_utf8_lossy(&output.stderr))),
        None => Err("north fleet send did not return within 30 s".to_owned()),
    }
}

/// Sends `line` through nexus-alert unless the same line went out in the last six hours, and pages the proxy when nexus-alert fails.
pub fn tell_tom(line: &str, dry_run: bool, at: f64) -> Result<(), String> {
    let mut alert = |line: &str| match run_within("nexus-alert", &["normal", line, "north fleet dispatch"], Duration::from_secs(30)) {
        Some(output) if output.status.success() => Ok(()),
        Some(output) => Err(format!("exited {}", output.status)),
        None => Err("did not return within 30 s".to_owned()),
    };
    let mut page = |line: &str| page_proxy(line);
    let mut written = |line: &str, at: f64| remember("alert.json", &json!({"text": line, "at": at}));
    tell_tom_with(line, dry_run, at, &mut alert, &mut page, &mut written)
}

/// `tell_tom` with its sinks and its record passed in: a failed alert is itself a page, and either
/// sink failing still returns `Err`, so the dispatch run exits non-zero even when the proxy page goes out.
pub(super) fn tell_tom_with(
    line: &str,
    dry_run: bool,
    at: f64,
    alert: &mut dyn FnMut(&str) -> Result<(), String>,
    page: &mut dyn FnMut(&str) -> Result<(), String>,
    written: &mut dyn FnMut(&str, f64),
) -> Result<(), String> {
    let last = placement::pace_state("dispatch/alert.json");
    if last["text"] == line && last["at"].as_f64().is_some_and(|sent| at - sent < REPEAT_ALERT_S) {
        println!("alert already sent: {line}");
        return Ok(());
    }
    if dry_run {
        println!("would alert: {line}");
        return Ok(());
    }
    match deliver(line, alert, page) {
        Ok(Delivery::Alert) => {
            println!("alerted: {line}");
            written(line, at);
            Ok(())
        }
        Ok(Delivery::Page) => {
            println!("paged the proxy: {line}");
            written(line, at);
            Err(format!("nexus-alert failed; paged the proxy instead: {line}"))
        }
        Err(why) => Err(format!("{why}; the line was: {line}")),
    }
}

/// Pages the proxy about each launch failure cause under its condition key, which the fleet store delivers once per episode however many dispatch runs repeat it.
pub fn watch_failures(pages: &[record::FailurePage], send: &mut dyn FnMut(&str, &str) -> bool) -> bool {
    let mut sent_all = true;
    for page in pages {
        let line = page.line();
        if send(&page.condition(), &line) {
            println!("paged the proxy: {line}");
        } else {
            eprintln!("north fleet dispatch: could not page the proxy: {line}");
            sent_all = false;
        }
    }
    sent_all
}

pub fn report_failures(records: &[Record], at: f64, dry_run: bool) -> bool {
    let pages = record::failure_pages(records, at);
    if dry_run {
        for page in &pages {
            println!("would page the proxy: {}", page.line());
        }
        return true;
    }
    watch_failures(&pages, &mut |key, line| page_condition(key, line).is_ok())
}

/// The message that asks the triage lead to box the open issues that have none.
pub fn boxless_summary(issues: &[&Issue]) -> String {
    let listed: Vec<String> = issues.iter().map(|issue| format!("{} {}", issue.reference(), issue.title)).collect();
    format!("north fleet dispatch: {} open issues have no Done-when box, so dispatch cannot staff them; give each a box, a roadmap label, or close it: {}", issues.len(), listed.join("; "))
}

/// Who is asked to box them: the session `[dispatch] triage_lead` names while some registry lists
/// it, else the proxy.
pub fn boxless_recipient<'a>(cfg: &'a DocumentMut, listed: &BTreeSet<String>) -> &'a str {
    cfg.get("dispatch").and_then(|rules| rules.get("triage_lead")).and_then(Item::as_str).filter(|lead| listed.contains(*lead)).unwrap_or(PROXY)
}

/// Sends `boxless_summary` to `boxless_recipient` at most once a day; a failed send waits the day too,
/// so a missing recipient is reported once a day, not every round.
pub fn ask_for_boxes(cfg: &DocumentMut, listed: &BTreeSet<String>, issues: &[&Issue], dry_run: bool, at: f64) {
    let to = boxless_recipient(cfg, listed);
    let last = placement::pace_state("dispatch/boxless.json");
    if let Some(sent) = last["at"].as_f64().filter(|sent| at - sent < BOXLESS_EVERY_S) {
        let outcome = last["failed"].as_str().map_or_else(|| "went".to_owned(), |why| format!("failed ({why})"));
        println!("needs-a-box summary to {} {outcome} {:.0} h ago", last["to"].as_str().unwrap_or(to), (at - sent) / 3600.0);
        return;
    }
    let text = boxless_summary(issues);
    if dry_run {
        println!("would send {to}: {text}");
        return;
    }
    let north = std::env::current_exe().map(|path| path.to_string_lossy().into_owned()).unwrap_or_else(|_| "north".into());
    match run_within(&north, &["fleet", "send", "--from", "service:north-dispatch", to, &text], Duration::from_secs(30)) {
        Some(output) if output.status.success() => {
            println!("sent {to}: {text}");
            remember("boxless.json", &json!({"to": to, "at": at}));
        }
        failed => {
            let why = failed.map_or_else(|| "timed out".to_owned(), |output| refusal_line(&String::from_utf8_lossy(&output.stderr)));
            eprintln!("north fleet dispatch: north fleet send {to} failed: {why}; the summary was: {text}");
            remember("boxless.json", &json!({"to": to, "at": at, "failed": why}));
        }
    }
}

fn remember(file: &str, value: &serde_json::Value) {
    let saved = std::fs::create_dir_all(record::directory()).map_err(|error| error.to_string()).and_then(|()| serde_json::to_vec(value).map_err(|error| error.to_string()));
    if let Err(why) = saved.and_then(|bytes| std::fs::write(record::directory().join(file), bytes).map_err(|error| error.to_string())) {
        eprintln!("north fleet dispatch: {file} not recorded: {why}");
    }
}

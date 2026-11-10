use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Duration;

use crate::foundation::clock::Timestamp;
use crate::platform::rolling_issue::Breaches;
use crate::platform::sqlite::{self, Access};
use units::Reading;

type Verdict = Result<(), String>;
type Output = Result<String, String>;

#[derive(Default, Deserialize, Serialize)]
struct Episode {
    failing: bool,
    since: Option<u64>,
}

#[derive(Deserialize, Serialize)]
struct LandCache {
    at: u64,
    error: Option<String>,
}

#[derive(Default, Deserialize, Serialize)]
struct State {
    #[serde(default)]
    checks: BTreeMap<String, Episode>,
    land: Option<LandCache>,
    #[serde(default)]
    breaches: Breaches,
    #[serde(default)]
    issues: Breaches,
}

fn timestamp(text: &str) -> Option<f64> {
    Timestamp::parse_rfc3339(text).map(Timestamp::unix_seconds_f64)
}

struct FreshnessInput {
    now: u64,
    text: Output,
}

fn fresh(now: u64, latest: Option<f64>, limit: u64) -> Verdict {
    let latest = latest.ok_or("no valid sample timestamp")?;
    let age = now as f64 - latest;
    if age >= limit as f64 {
        Err(format!(
            "newest sample is {age:.0}s old; expected under {limit}s"
        ))
    } else {
        Ok(())
    }
}

fn pace(input: FreshnessInput) -> Verdict {
    let value: Value = serde_json::from_str(&input.text?).map_err(|e| e.to_string())?;
    let latest = value
        .get("history")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|row| row.get("ts").and_then(Value::as_str).and_then(timestamp))
        .max_by(f64::total_cmp);
    fresh(input.now, latest, 3 * 60)
}

fn usage(input: FreshnessInput) -> Verdict {
    fresh(input.now, timestamp(&input.text?), 10 * 60)
}

fn timer_names(config: &str) -> Result<Vec<String>, String> {
    let rest = config
        .split_once("capacity-watchdog")
        .ok_or("capacity-watchdog list missing")?
        .1;
    let list = rest
        .split_once("user = [")
        .ok_or("capacity-watchdog user list missing")?
        .1;
    let list = list.split_once("];").ok_or("unterminated user list")?.0;
    let names: Vec<_> = list
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .flat_map(|line| {
            line.split('"')
                .enumerate()
                .filter_map(|(i, s)| (i % 2 == 1 && s.ends_with(".timer")).then_some(s.to_owned()))
        })
        .collect();
    if names.is_empty() {
        Err("capacity-watchdog user list has no timers".into())
    } else {
        Ok(names)
    }
}

struct TimersInput {
    config: Output,
    active: BTreeMap<String, Output>,
}

fn timers(input: TimersInput) -> Verdict {
    let names = timer_names(&input.config?)?;
    let missing: Vec<_> = names
        .iter()
        .filter(
            |name| !matches!(input.active.get(*name), Some(Ok(text)) if text.trim() == "active"),
        )
        .cloned()
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!("inactive or missing: {}", missing.join(", ")))
    }
}

struct CiInput {
    runs: BTreeMap<String, Output>,
}

const RED_CONCLUSIONS: [&str; 3] = ["failure", "timed_out", "startup_failure"];

fn not_modified(error: &str) -> bool {
    error.contains("HTTP 304")
}

fn ci(input: CiInput) -> Verdict {
    let mut failures = Vec::new();
    for repo in ["north", "makgora"] {
        let output = input
            .runs
            .get(repo)
            .ok_or_else(|| format!("{repo}: run list missing"))?;
        let text = match output {
            Err(error) if not_modified(error) => continue,
            other => other.as_ref().map_err(|e| format!("{repo}: {e}"))?,
        };
        let rows: Vec<Value> = serde_json::from_str(text).map_err(|e| format!("{repo}: {e}"))?;
        let mut seen = BTreeSet::new();
        let mut completed = false;
        for row in rows {
            if row["status"] != "completed" {
                continue;
            }
            completed = true;
            let Some(conclusion) = row["conclusion"].as_str() else { continue };
            if matches!(conclusion, "cancelled" | "skipped" | "neutral") {
                continue;
            }
            let name = row["workflowName"].as_str().ok_or("workflowName missing")?;
            if seen.insert(name.to_owned()) && RED_CONCLUSIONS.contains(&conclusion) {
                failures.push(format!("{repo}/{name}: {conclusion}"));
            }
        }
        if !completed {
            failures.push(format!("{repo}: no completed runs in latest five"));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

struct LandInput {
    now: u64,
    output: Option<Output>,
}

fn land_due(now: u64, state: &State) -> bool {
    state
        .land
        .as_ref()
        .is_none_or(|cache| now < cache.at || now - cache.at >= 30 * 60)
}

fn land(input: LandInput, state: &mut State) -> Verdict {
    if land_due(input.now, state) {
        let output = input.output.ok_or("landing probe output missing")?;
        state.land = Some(LandCache {
            at: input.now,
            error: output.err(),
        });
    }
    match state.land.as_ref().and_then(|cache| cache.error.as_ref()) {
        Some(error) => Err(error.clone()),
        None => Ok(()),
    }
}

fn transition(state: &mut State, check: &str, result: &Verdict, confirmed: bool, now: u64) -> Option<String> {
    if result.is_err() && !confirmed {
        return None;
    }
    let episode = state.checks.entry(check.into()).or_default();
    let failing = result.is_err();
    if episode.failing == failing {
        return None;
    }
    episode.failing = failing;
    episode.since = Some(now);
    Some(match result {
        Err(error) => format!("health: {check}: {error}"),
        Ok(()) => format!("health: {check}: recovered"),
    })
}

fn single_line(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(1000)
        .collect()
}

fn command(program: &str, args: &[&str], cwd: Option<&Path>, seconds: u64) -> Output {
    let mut cmd = Command::new("timeout");
    cmd.args(["--kill-after=5s", &format!("{seconds}s"), program])
        .args(args);
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    let output = cmd.output().map_err(|e| format!("{program}: {e}"))?;
    if output.status.success() {
        String::from_utf8(output.stdout).map_err(|e| format!("{program}: {e}"))
    } else {
        Err(format!(
            "{program} exited {}: {}",
            output.status,
            single_line(&format!(
                "{} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ))
        ))
    }
}

fn newest_usage(db: &Path) -> Output {
    let rows = sqlite::rows(db, Access::ReadOnly, Duration::from_secs(5), "select max(ts) from usage").map_err(|e| e.to_string())?;
    Ok(match rows.first().and_then(|row| row.as_object()?.values().next().cloned()) {
        Some(Value::String(text)) => text,
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    })
}

fn read(path: &Path) -> Output {
    fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn landing_probe(repo: &Path, runtime: &Path) -> Output {
    let root = runtime.join("north-health");
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let directory = tempfile::Builder::new()
        .prefix("land-")
        .tempdir_in(root)
        .map_err(|e| e.to_string())?;
    let path = directory.path().join("worktree");
    let path_text = path.to_str().ok_or("scratch path is not UTF-8")?;
    let branch = format!("north-health-probe-{}", std::process::id());
    command(
        "git",
        &["worktree", "add", "-b", branch.as_str(), path_text, "origin/main"],
        Some(repo),
        20,
    )?;
    let result = command(
        "safe-push",
        &["--dry-run", "--to", "main"],
        Some(&path),
        240,
    );
    let cleanup = command(
        "git",
        &["worktree", "remove", "--force", path_text],
        Some(repo),
        20,
    );
    let _ = command("git", &["branch", "-D", branch.as_str()], Some(repo), 20);
    match (result, cleanup) {
        (Ok(output), Ok(_)) => Ok(output),
        (Err(error), Ok(_)) => Err(error),
        (result, Err(error)) => Err(format!(
            "{}; scratch cleanup: {error}",
            result.err().unwrap_or_default()
        )),
    }
}

fn save(path: &Path, state: &impl Serialize) -> Result<(), String> {
    let parent = path.parent().ok_or("state path has no parent")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    serde_json::to_writer(file.as_file_mut(), state).map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}

fn live(dry_run: bool, citations: fn(&Path) -> Verdict) -> Result<(bool, Vec<String>), String> {
    let mut lines = Vec::new();
    let home = PathBuf::from(std::env::var_os("HOME").ok_or("HOME missing")?);
    let now = Timestamp::now().unix_seconds() as u64;
    let state_path = home.join(".local/state/north/health.json");
    let mut state: State = match fs::read(&state_path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| format!("health state: {e}"))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => State::default(),
        Err(e) => return Err(e.to_string()),
    };
    let repo = home.join("code/north/main");
    if let Err(why) = command("git", &["fetch", "-q", "origin"], Some(&repo), 20) {
        lines.push(format!("health: origin/main not fetched, landing judged on the last fetched ref: {why}"));
    }
    let world = flows::probe::world(&home, &repo, now as f64);
    let mut ages = BTreeMap::new();
    let mut results: Vec<(String, Verdict)> = Vec::new();
    for flow in world.judge() {
        let check = format!("flow {}", flow.name);
        ages.insert(check.clone(), flow.age_s.map_or_else(|| "no time recorded".to_owned(), flows::minutes));
        results.push((check, flow.verdict));
    }
    results.extend([
        (
            "pace".into(),
            pace(FreshnessInput {
                now,
                text: read(&home.join(".local/state/agents/pace-control.json")),
            }),
        ),
        (
            "usage".into(),
            usage(FreshnessInput {
                now,
                text: newest_usage(&home.join(".local/state/threads/snapshot.db")),
            }),
        ),
    ]);
    let config = read(&home.join("code/nixos-config/main/hosts/whiterabbit/configuration.nix"));
    let names = config
        .as_ref()
        .ok()
        .and_then(|text| timer_names(text).ok())
        .unwrap_or_default();
    let active = names
        .iter()
        .map(|name| {
            let result = command("systemctl", &["--user", "is-active", name], None, 5);
            (name.clone(), result)
        })
        .collect();
    results.push(("timers".into(), timers(TimersInput { config, active })));
    for timer in &names {
        let unit = units::service_of(timer);
        let shown = command("systemctl", &["--user", "show", "-p", "LoadState", &unit], None, 5);
        let finished = command("journalctl", &["--user", "--no-pager", "-u", &unit, "JOB_TYPE=start", "--reverse", "--lines=100", "--output=cat", "--output-fields=JOB_RESULT"], None, 5);
        let verdict = units::judge(&unit, shown.ok().map(|text| Reading::parse(&text, finished.as_deref().unwrap_or_default())).as_ref());
        let check = format!("unit {unit}");
        if verdict.is_err() || state.checks.get(&check).is_some_and(|episode| episode.failing) {
            results.push((check, verdict));
        }
    }
    let runs: BTreeMap<String, Output> = ["north", "makgora"]
        .into_iter()
        .map(|repo| {
            (
                repo.into(),
                command(
                    "gh",
                    &[
                        "run",
                        "list",
                        "-R",
                        &format!("tompassarelli/{repo}"),
                        "--branch",
                        "main",
                        "--limit",
                        "5",
                        "--json",
                        "conclusion,status,workflowName,headSha,url",
                    ],
                    None,
                    20,
                ),
            )
        })
        .collect();
    let north_runs = runs.get("north").and_then(|text| text.clone().ok());
    results.push(("citations".into(), citations(&repo)));
    let loop_root = std::env::var_os("NORTH_HEALTH_REPO").map_or_else(|| repo.clone(), PathBuf::from);
    let (loop_result, unverifiable) = loops::verdict(&loop_root, home.join(".local/state"));
    results.push(("loops".into(), loop_result));
    if !unverifiable.is_empty() {
        lines.push(format!("health: loops: unverifiable (no reader stamp): {}", unverifiable.join(", ")));
    }
    results.push(("ci".into(), ci(CiInput { runs })));
    let (rates, verdict) = rework::live(&home.join(".local/state/threads/snapshot.db"), now);
    lines.push(format!("health: rework 7d: {rates}"));
    results.push(("rework".into(), verdict));
    let output = if land_due(now, &state) {
        Some(match std::env::var_os("XDG_RUNTIME_DIR") {
            Some(path) => landing_probe(&repo, &PathBuf::from(path)),
            None => Err("XDG_RUNTIME_DIR missing".into()),
        })
    } else {
        None
    };
    results.push(("land".into(), land(LandInput { now, output }, &mut state)));
    let state_file = std::env::var_os("XDG_STATE_HOME").map_or_else(|| home.join(".local/state"), PathBuf::from).join("north/unit-failures.json");
    let started = failed_starts::check(dry_run, &state_file, &failed_starts::journal, &mut failed_starts::tell_proxy, now);
    lines.push(format!("health: unit-failures: {}", started.as_ref().err().map_or("ok", String::as_str)));
    let mut alerts = Vec::new();
    let breaching = results.iter().filter(|(_, result)| result.is_err()).map(|(check, _)| check.clone()).collect();
    let confirmed = state.breaches.tick(&breaching);
    for (check, result) in &results {
        let age = ages.get(check).map_or_else(String::new, |age: &String| format!(" (age {age})"));
        lines.push(format!("health: {check}{age}: {}", result.as_ref().err().map_or("ok", String::as_str)));
        if let Some(line) = transition(&mut state, check, result, confirmed.contains(check), now) {
            alerts.push((check.clone(), line));
        }
    }
    if !dry_run {
        issues::file(&results, north_runs.as_deref(), &mut state.issues);
        save(&state_path, &state)?;
        for (_, line) in alerts {
            if line.ends_with(": recovered") {
                lines.push(line.clone());
            }
            for (program, args) in [
                ("north", vec!["fleet", "send", "--from", "service:north-health", "proxy", line.as_str()]),
                ("notify-send", vec!["-u", "critical", "North harness", line.as_str()]),
            ] {
                if let Err(error) = command(program, &args, None, 20) {
                    eprintln!("health: alert: {error}");
                }
            }
        }
    }
    Ok((results.iter().all(|(_, result)| result.is_ok()), lines))
}

pub fn report(dry_run: bool, citations: fn(&Path) -> Verdict) -> Result<(bool, Vec<String>), String> {
    live(dry_run, citations)
}

pub fn run(arguments: &[String], citations: fn(&Path) -> Result<(), String>) -> ExitCode {
    let dry_run = match arguments {
        [] => false,
        [flag] if flag == "--dry-run" => true,
        [verb] if verb == "facts" => return facts::run(),
        _ => {
            eprintln!("Usage: north health [--dry-run] | facts");
            return ExitCode::from(1);
        }
    };
    match live(dry_run, citations) {
        Ok((ok, lines)) => {
            for line in lines {
                println!("{line}");
            }
            if ok { ExitCode::SUCCESS } else { ExitCode::from(1) }
        }
        Err(error) => {
            eprintln!("health: state: {}", single_line(&error));
            ExitCode::from(1)
        }
    }
}

mod facts;
mod failed_starts;
pub mod flows;
mod issues;
pub mod loops;
mod main_red;
mod rework;
mod units;

#[cfg(test)]
mod tests;

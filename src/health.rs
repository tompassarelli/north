use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Duration;

use crate::foundation::clock::Timestamp;
use crate::platform::sqlite::{self, Access};
use units::{Gh, Reading, Run};

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
    runtime_mismatch_since: Option<u64>,
    land: Option<LandCache>,
    #[serde(default)]
    units: BTreeMap<String, Run>,
}

struct RuntimeInput {
    now: u64,
    current: Output,
    expected: Output,
    fetched: bool,
}

fn runtime(input: RuntimeInput, state: &mut State) -> Verdict {
    let current = input.current?;
    let expected = input.expected?;
    let expected = expected.trim();
    if expected.len() != 40 || !expected.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("origin/main did not resolve to a full SHA".into());
    }
    if current.trim().ends_with(expected) {
        state.runtime_mismatch_since = None;
        return Ok(());
    }
    let since = *state.runtime_mismatch_since.get_or_insert(input.now);
    if input.now.saturating_sub(since) < 15 * 60 {
        return Ok(());
    }
    Err(format!(
        "current runtime differs from origin/main {expected} for at least 15 minutes{}",
        if input.fetched {
            ""
        } else {
            " (fetch failed; using last fetched ref)"
        }
    ))
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

fn ci(input: CiInput) -> Verdict {
    let mut failures = Vec::new();
    for repo in ["north", "smashcraft"] {
        let output = input
            .runs
            .get(repo)
            .ok_or_else(|| format!("{repo}: run list missing"))?;
        let text = output.as_ref().map_err(|e| format!("{repo}: {e}"))?;
        let rows: Vec<Value> = serde_json::from_str(text).map_err(|e| format!("{repo}: {e}"))?;
        let mut seen = BTreeSet::new();
        for row in rows {
            if row["status"] != "completed" {
                continue;
            }
            let name = row["workflowName"].as_str().ok_or("workflowName missing")?;
            if seen.insert(name.to_owned()) && row["conclusion"] != "success" {
                failures.push(format!("{repo}/{name}: {}", row["conclusion"]));
            }
        }
        if seen.is_empty() {
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

fn transition(state: &mut State, check: &str, result: &Verdict, now: u64) -> Option<String> {
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

fn save(path: &Path, state: &State) -> Result<(), String> {
    let parent = path.parent().ok_or("state path has no parent")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    serde_json::to_writer(file.as_file_mut(), state).map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}

fn live(dry_run: bool) -> Result<bool, String> {
    let home = PathBuf::from(std::env::var_os("HOME").ok_or("HOME missing")?);
    let now = Timestamp::now().unix_seconds() as u64;
    let state_path = home.join(".local/state/north/health.json");
    let mut state: State = match fs::read(&state_path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| format!("health state: {e}"))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => State::default(),
        Err(e) => return Err(e.to_string()),
    };
    let repo = home.join("code/north/main");
    let fetched = command("git", &["fetch", "-q", "origin"], Some(&repo), 20).is_ok();
    let current = fs::read_link(home.join(".local/state/north/current"))
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| format!("current runtime: {e}"));
    let expected = command("git", &["rev-parse", "origin/main"], Some(&repo), 5);
    let mut results: Vec<(String, Verdict)> = vec![
        (
            "runtime".into(),
            runtime(
                RuntimeInput {
                    now,
                    current,
                    expected,
                    fetched,
                },
                &mut state,
            ),
        ),
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
    ];
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
        let shown = command("systemctl", &["--user", "show", "-p", "LoadState,ActiveState,Result,InvocationID", &unit], None, 5);
        let verdict = units::judge(&unit, shown.ok().map(|text| Reading::parse(&text)).as_ref(), &mut state.units);
        let check = format!("unit {unit}");
        if verdict.is_err() || state.checks.get(&check).is_some_and(|episode| episode.failing) {
            results.push((check, verdict));
        }
    }
    let runs = ["north", "smashcraft"]
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
                        "conclusion,status,workflowName,headSha",
                    ],
                    None,
                    20,
                ),
            )
        })
        .collect();
    results.push(("ci".into(), ci(CiInput { runs })));
    let output = if land_due(now, &state) {
        Some(match std::env::var_os("XDG_RUNTIME_DIR") {
            Some(path) => landing_probe(&repo, &PathBuf::from(path)),
            None => Err("XDG_RUNTIME_DIR missing".into()),
        })
    } else {
        None
    };
    results.push(("land".into(), land(LandInput { now, output }, &mut state)));
    let mut alerts = Vec::new();
    for (check, result) in &results {
        println!(
            "health: {check}: {}",
            result.as_ref().err().map_or("ok", String::as_str)
        );
        if let Some(line) = transition(&mut state, check, result, now) {
            alerts.push((check.clone(), line));
        }
    }
    if !dry_run {
        save(&state_path, &state)?;
        for (check, line) in alerts {
            if line.ends_with(": recovered") {
                println!("{line}");
            } else if let Some(unit) = check.strip_prefix("unit ")
                && let Err(error) = units::report(&mut Gh, unit, &line)
            {
                eprintln!("health: issue: {error}");
            }
            for (program, args) in [
                (
                    "agents",
                    vec![
                        "event",
                        "STUCK",
                        "--from-node",
                        "harness-health",
                        "--to-node",
                        "proxy",
                        "--body",
                        line.as_str(),
                    ],
                ),
                (
                    "notify-send",
                    vec!["-u", "critical", "North harness", line.as_str()],
                ),
            ] {
                if let Err(error) = command(program, &args, None, 5) {
                    eprintln!("health: alert: {error}");
                }
            }
        }
    }
    Ok(results.iter().all(|(_, result)| result.is_ok()))
}

pub fn run(arguments: &[String]) -> ExitCode {
    let dry_run = match arguments {
        [] => false,
        [flag] if flag == "--dry-run" => true,
        _ => {
            eprintln!("Usage: north health [--dry-run]");
            return ExitCode::from(1);
        }
    };
    match live(dry_run) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(error) => {
            eprintln!("health: state: {}", single_line(&error));
            ExitCode::from(1)
        }
    }
}

mod units;

#[cfg(test)]
mod tests;

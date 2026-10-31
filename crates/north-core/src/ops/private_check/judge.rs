use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::config::{Check, Step};

pub const DESCRIPTION_CHARS: usize = 140;
pub const DEFERRED: i32 = 75;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatusRequest {
    pub repo: String,
    pub sha: String,
    pub context: String,
    pub state: &'static str,
    pub description: String,
}

impl StatusRequest {
    pub fn body(&self) -> String {
        serde_json::json!({ "state": self.state, "context": self.context, "description": self.description }).to_string()
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct Finished {
    pub code: i32,
    pub last_line: String,
}

pub trait Steps {
    fn run(&mut self, dir: &Path, step: &Step, owner: &str) -> Result<Finished, String>;
}

pub trait Statuses {
    fn post(&mut self, request: &StatusRequest) -> Result<(), String>;
}

pub struct Places {
    pub code: PathBuf,
    pub state: PathBuf,
    pub host: String,
}

#[derive(Debug, Default)]
pub struct Overrides {
    pub reference: Option<String>,
    pub since: Option<String>,
    pub dry_run: bool,
}

#[derive(Debug, Eq, PartialEq)]
pub enum Outcome {
    Current(String),
    Untouched(String),
    Judged(StatusRequest),
    Deferred(String),
}

pub fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git").arg("-C").arg(dir).args(args).output().map_err(|error| format!("cannot run git: {error}"))?;
    if !output.status.success() {
        return Err(format!("git {} in {}: {}", args.join(" "), dir.display(), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim_end().to_owned())
}

pub fn worktree(places: &Places, check: &Check) -> PathBuf {
    places.code.join(&check.checkout).join("worktrees").join(format!("private-check-{}", check.name))
}

fn state_file(places: &Places, check: &Check) -> PathBuf {
    places.state.join(format!("{}.json", check.name))
}

pub fn judged_tip(places: &Places, check: &Check) -> Option<String> {
    let text = fs::read_to_string(state_file(places, check)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value.get("judged").and_then(serde_json::Value::as_str).map(str::to_owned)
}

fn record(places: &Places, check: &Check, tip: &str) -> Result<(), String> {
    fs::create_dir_all(&places.state).map_err(|error| format!("cannot create {}: {error}", places.state.display()))?;
    let path = state_file(places, check);
    let staging = path.with_extension("json.tmp");
    let body = serde_json::json!({ "judged": tip }).to_string() + "\n";
    fs::write(&staging, body).and_then(|()| fs::rename(&staging, &path)).map_err(|error| format!("cannot write {}: {error}", path.display()))
}

fn fetch(places: &Places, check: &Check) -> Result<PathBuf, String> {
    let lane = worktree(places, check);
    if lane.join(".git").exists() {
        git(&lane, &["fetch", "--quiet", "origin", "main"])?;
        return Ok(lane);
    }
    let main = places.code.join(&check.checkout).join("main");
    git(&main, &["fetch", "--quiet", "origin", "main"])?;
    let target = lane.to_string_lossy();
    git(&main, &["worktree", "add", "--quiet", "--lock", "--reason", "north private-check", "--detach", &target, "origin/main"])?;
    Ok(lane)
}

fn changed(lane: &Path, check: &Check, since: &str, tip: &str) -> Result<Option<Vec<String>>, String> {
    if git(lane, &["rev-parse", "--verify", "--quiet", &format!("{since}^{{commit}}")]).is_err() {
        return Ok(None);
    }
    let mut args = vec!["diff", "--name-only", since, tip, "--"];
    args.extend(check.paths.iter().map(String::as_str));
    Ok(Some(git(lane, &args)?.lines().map(str::to_owned).collect()))
}

pub fn description(text: &str) -> String {
    text.chars().take(DESCRIPTION_CHARS).collect()
}

fn verdict(code: i32, last_line: &str) -> (&'static str, String) {
    let state = match code {
        0 => "success",
        1 => "failure",
        _ => "error",
    };
    let line = if last_line.is_empty() { format!("exited {code} with no summary line") } else { last_line.to_owned() };
    (state, description(&line))
}

fn steps(lane: &Path, check: &Check, runner: &mut dyn Steps) -> Option<(&'static str, String)> {
    let owner = format!("private-check:{}", check.name);
    let Some((last, setup)) = check.steps.split_last() else { return Some(("error", "no steps".into())) };
    for step in setup {
        match runner.run(&lane.join(&step.dir), step, &owner) {
            Ok(done) if done.code == 0 => {}
            Ok(done) if done.code == DEFERRED => return None,
            Ok(done) => return Some(("error", description(&format!("setup {} exited {}", step.command.join(" "), done.code)))),
            Err(why) => return Some(("error", description(&format!("setup {}: {why}", step.command.join(" "))))),
        }
    }
    match runner.run(&lane.join(&last.dir), last, &owner) {
        Ok(done) if done.code == DEFERRED => None,
        Ok(done) => Some(verdict(done.code, &done.last_line)),
        Err(why) => Some(("error", description(&why))),
    }
}

pub fn judge(places: &Places, check: &Check, overrides: &Overrides, runner: &mut dyn Steps, statuses: &mut dyn Statuses) -> Result<Outcome, String> {
    let lane = fetch(places, check)?;
    let reference = overrides.reference.as_deref().unwrap_or("origin/main");
    let tip = git(&lane, &["rev-parse", "--verify", &format!("{reference}^{{commit}}")])?;
    let since = overrides.since.clone().or_else(|| judged_tip(places, check));
    if since.as_deref() == Some(tip.as_str()) {
        return Ok(Outcome::Current(tip));
    }
    let keep = |tip: &str| if overrides.dry_run { Ok(()) } else { record(places, check, tip) };
    if let Some(since) = since.as_deref()
        && changed(&lane, check, since, &tip)?.is_some_and(|files| files.is_empty())
    {
        keep(&tip)?;
        return Ok(Outcome::Untouched(tip));
    }
    git(&lane, &["checkout", "--quiet", "--force", "--detach", &tip])?;
    let request = |state, description| StatusRequest { repo: check.repo.clone(), sha: tip.clone(), context: check.context.clone(), state, description };
    statuses.post(&request("pending", description(&format!("running on {}", places.host))))?;
    let Some((state, text)) = steps(&lane, check, runner) else { return Ok(Outcome::Deferred(tip)) };
    let last = request(state, text);
    statuses.post(&last)?;
    keep(&tip)?;
    Ok(Outcome::Judged(last))
}

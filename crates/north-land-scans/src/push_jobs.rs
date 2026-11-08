use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::git;
use crate::workflow_paths::matches;

const WORKFLOWS: &str = ".github/workflows";
const MAIN: &[u8] = b"main";

pub struct Job {
    pub name: String,
    pub commands: Vec<String>,
    definition: String,
}

impl Job {
    pub fn runs_only(&self, mut check: impl FnMut(&str) -> bool) -> bool {
        !self.commands.is_empty() && self.commands.iter().all(|command| check(command))
    }
}

/// The jobs a lane adds or changes that run on a push to main. A job whose workflow and definition are unchanged from the base is the repository's existing after-landing gate, not this lane's: refusing every lane until every repository has recorded reasons for jobs it already had would stop work in repositories that have not been migrated yet, so only the jobs this lane brings under the after-landing rule are judged.
pub fn newly_on_main(repo: &Path, base: &str, head: &str) -> Result<Vec<Job>, String> {
    let at_head = on_main(repo, head)?;
    if base.is_empty() {
        return Ok(at_head);
    }
    let at_base = on_main(repo, base)?;
    let known: BTreeMap<String, String> = at_base.into_iter().map(|job| (job.name, job.definition)).collect();
    Ok(at_head.into_iter().filter(|job| known.get(&job.name) != Some(&job.definition)).collect())
}

fn on_main(repo: &Path, revision: &str) -> Result<Vec<Job>, String> {
    let listing = git::text(repo, &["ls-tree", "--name-only", &format!("{revision}:{WORKFLOWS}")]).unwrap_or_default();
    let mut jobs = Vec::new();
    for file in listing.lines().filter(|file| file.ends_with(".yml") || file.ends_with(".yaml")) {
        let path = format!("{WORKFLOWS}/{file}");
        let workflow = parse(repo, revision, &path)?;
        if !pushes_main(workflow.get("on").unwrap_or(&Value::Null)) {
            continue;
        }
        for (name, job) in workflow.get("jobs").and_then(Value::as_object).into_iter().flatten() {
            let steps = job.get("steps").and_then(Value::as_array).into_iter().flatten();
            let commands = steps.filter_map(|step| step.get("run").and_then(Value::as_str)).map(|run| run.trim().to_owned()).collect();
            jobs.push(Job { name: format!("{path}:{name}"), commands, definition: job.to_string() });
        }
    }
    Ok(jobs)
}

fn parse(repo: &Path, revision: &str, path: &str) -> Result<Value, String> {
    let definition = git::bytes(repo, &["show", &format!("{revision}:{path}")]).ok_or_else(|| format!("{path}: git show failed"))?;
    let mut yq = Command::new("yq")
        .args(["-o=json", "."])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("{path}: yq: {error}"))?;
    yq.stdin.take().ok_or_else(|| format!("{path}: yq stdin"))?.write_all(&definition).map_err(|error| format!("{path}: yq: {error}"))?;
    let output = yq.wait_with_output().map_err(|error| format!("{path}: yq: {error}"))?;
    if !output.status.success() {
        return Err(format!("{path}: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    serde_json::from_slice(&output.stdout).map_err(|error| format!("{path}: {error}"))
}

fn pushes_main(on: &Value) -> bool {
    match on {
        Value::String(event) => event == "push",
        Value::Array(events) => events.iter().any(|event| event.as_str() == Some("push")),
        Value::Object(events) => events.get("push").is_some_and(push_filter_admits_main),
        _ => false,
    }
}

fn push_filter_admits_main(push: &Value) -> bool {
    let patterns = |key: &str| -> Option<Vec<&str>> { push.get(key).and_then(Value::as_array).map(|list| list.iter().filter_map(Value::as_str).collect()) };
    if let Some(branches) = patterns("branches") {
        let hit = |negated: bool| branches.iter().filter(|pattern| pattern.starts_with('!') == negated).any(|pattern| matches(pattern.trim_start_matches('!').as_bytes(), MAIN));
        return hit(false) && !hit(true);
    }
    if let Some(ignored) = patterns("branches-ignore") {
        return !ignored.iter().any(|pattern| matches(pattern.as_bytes(), MAIN));
    }
    push.get("tags").is_none() && push.get("tags-ignore").is_none()
}

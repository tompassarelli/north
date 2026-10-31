use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use regex::Regex;
use serde_json::Value;

use super::{env_or, home};

const PROJECTS: [&str; 4] = ["smashcraft", "wisp", "north", "nixos-config"];
const ALERT_FAILED: &str = "supply-alert failed";
const LOCKFILE: &str = r"(^|/)(bun\.lock|Cargo\.lock|uv\.lock|requirements[^/]*\.txt|package-lock\.json)$";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Finding {
    pub(super) key: String,
    pub(super) id: String,
    pub(super) repo: String,
    pub(super) pkg: String,
    pub(super) fixed: Option<String>,
}

pub fn run(_arguments: &[String]) -> u8 {
    match scan() {
        Ok(summary) => {
            println!("supply-chain-scan: {summary}");
            0
        }
        Err(message) => {
            eprintln!("supply-chain-scan: {message}");
            if message != ALERT_FAILED {
                alert("normal", &message);
            }
            1
        }
    }
}

fn alert(level: &str, text: &str) -> bool {
    Command::new("supply-alert").args([level, text]).status().is_ok_and(|status| status.success())
}

pub(super) fn lockfile_names(ls_tree: &[u8]) -> Vec<String> {
    let lockfile = Regex::new(LOCKFILE).expect("lockfile pattern");
    let vendored = Regex::new(r"(^|/)repos/").expect("vendored pattern");
    ls_tree
        .split(|byte| *byte == 0)
        .filter_map(|name| std::str::from_utf8(name).ok())
        .filter(|name| lockfile.is_match(name) && !vendored.is_match(name))
        .map(str::to_owned)
        .collect()
}

fn fetch_lockfiles(scratch: &Path) -> Result<Vec<String>, String> {
    let mut paths = Vec::new();
    for project in PROJECTS {
        let repo = home().join("code").join(project).join("main");
        if !repo.is_dir() {
            eprintln!("supply-chain-scan: skipping {project}: {} is missing", repo.display());
            continue;
        }
        let fetched = Command::new("git").arg("-C").arg(&repo).args(["fetch", "-q", "origin", "main"]).status();
        if !fetched.is_ok_and(|status| status.success()) {
            return Err(format!("git fetch failed in {project}"));
        }
        let listing = Command::new("git").arg("-C").arg(&repo).args(["ls-tree", "-r", "-z", "--name-only", "origin/main"]).output();
        for name in lockfile_names(&listing.map(|output| output.stdout).unwrap_or_default()) {
            let dest = scratch.join(project).join(&name);
            let shown = Command::new("git").arg("-C").arg(&repo).args(["show", &format!("origin/main:{name}")]).output();
            let written = shown.ok().filter(|output| output.status.success()).and_then(|output| {
                fs::create_dir_all(dest.parent()?).ok()?;
                fs::write(&dest, output.stdout).ok()
            });
            if written.is_none() {
                return Err(format!("git show failed for {project}:{name}"));
            }
            paths.push(dest.to_string_lossy().into_owned());
        }
    }
    Ok(paths)
}

fn osv_output(paths: &[String]) -> Result<Value, String> {
    let mut command = Command::new("osv-scanner");
    command.args(["scan", "source", "--format", "json"]);
    paths.iter().for_each(|path| {
        command.args(["-L", path]);
    });
    let output = command.stdin(Stdio::null()).output().map_err(|error| format!("osv-scanner: cannot run osv-scanner: {error}"))?;
    if !matches!(output.status.code(), Some(0 | 1)) {
        let first = String::from_utf8_lossy(&output.stderr).lines().next().unwrap_or_default().to_owned();
        return Err(format!("osv-scanner: {first}"));
    }
    serde_json::from_slice(&output.stdout).map_err(|error| format!("osv-scanner: unreadable output: {error}"))
}

pub(super) fn findings(osv: &Value, scratch_prefix: &str) -> Vec<Finding> {
    let mut found = Vec::new();
    for result in osv["results"].as_array().into_iter().flatten() {
        let path = result["source"]["path"].as_str().unwrap_or_default();
        let repo = path.strip_prefix(scratch_prefix).unwrap_or(path).split('/').next().unwrap_or_default();
        for entry in result["packages"].as_array().into_iter().flatten() {
            let name = entry["package"]["name"].as_str().unwrap_or_default();
            let version = entry["package"]["version"].as_str().unwrap_or_default();
            for vulnerability in entry["vulnerabilities"].as_array().into_iter().flatten() {
                let id = vulnerability["id"].as_str().unwrap_or_default();
                found.push(Finding {
                    key: format!("{id} {repo} {name}"),
                    id: id.to_owned(),
                    repo: repo.to_owned(),
                    pkg: format!("{name}@{version}"),
                    fixed: first_fix(vulnerability, name),
                });
            }
        }
    }
    found.sort_by(|left, right| left.key.cmp(&right.key));
    found.dedup_by(|later, earlier| later.key == earlier.key);
    found
}

fn first_fix(vulnerability: &Value, name: &str) -> Option<String> {
    vulnerability["affected"]
        .as_array()?
        .iter()
        .filter(|affected| affected["package"]["name"].as_str() == Some(name))
        .flat_map(|affected| affected["ranges"].as_array().into_iter().flatten())
        .flat_map(|range| range["events"].as_array().into_iter().flatten())
        .find_map(|event| event["fixed"].as_str().map(str::to_owned))
}

pub(super) fn alert_texts(fresh: &[Finding]) -> (Option<String>, Option<String>) {
    let join = |lines: Vec<String>| (!lines.is_empty()).then(|| lines.join("; "));
    let malware = join(fresh.iter().filter(|f| f.id.starts_with("MAL-")).map(|f| format!("{} {} ({})", f.id, f.pkg, f.repo)).collect());
    let fixable = join(
        fresh
            .iter()
            .filter(|f| !f.id.starts_with("MAL-"))
            .filter_map(|f| Some(format!("{} {} ({}) fixed {}", f.id, f.pkg, f.repo, f.fixed.as_ref()?)))
            .collect(),
    );
    (malware.map(|text| format!("malware flagged: {text}")), fixable.map(|text| format!("fixable advisories: {text}")))
}

pub(super) fn merged_seen(seen: &[String], current: &[Finding]) -> Vec<String> {
    let mut all: Vec<String> = seen.iter().cloned().chain(current.iter().map(|f| f.key.clone())).collect();
    all.sort();
    all.dedup();
    all
}

fn scan() -> Result<String, String> {
    let state = env_or("XDG_STATE_HOME", || home().join(".local/state")).join("supply-chain");
    let seen_path = state.join("seen.json");
    fs::create_dir_all(&state).map_err(|error| format!("{}: {error}", state.display()))?;
    if !seen_path.exists() {
        fs::write(&seen_path, "[]\n").map_err(|error| format!("{}: {error}", seen_path.display()))?;
    }
    let scratch = tempfile::Builder::new().prefix("supply-chain-scan.").tempdir().map_err(|error| error.to_string())?;
    let mut paths = fetch_lockfiles(scratch.path())?;
    let global = home().join(".bun/install/global/bun.lock");
    if global.is_file() {
        let copy = scratch.path().join("bun-global/bun.lock");
        fs::create_dir_all(copy.parent().expect("lockfile parent")).and_then(|()| fs::copy(&global, &copy)).map_err(|error| error.to_string())?;
        paths.push(copy.to_string_lossy().into_owned());
    }
    let prefix = format!("{}/", scratch.path().display());
    let current = findings(&osv_output(&paths)?, &prefix);
    let seen: Vec<String> = serde_json::from_slice(&fs::read(&seen_path).map_err(|error| error.to_string())?).map_err(|error| format!("{}: {error}", seen_path.display()))?;
    let fresh: Vec<Finding> = current.iter().filter(|f| !seen.contains(&f.key)).cloned().collect();
    let (urgent, normal) = alert_texts(&fresh);
    for (level, text) in [("urgent", &urgent), ("normal", &normal)] {
        if let Some(text) = text
            && !alert(level, text)
        {
            return Err(ALERT_FAILED.to_owned());
        }
    }
    let merged = serde_json::to_string(&merged_seen(&seen, &current)).map_err(|error| error.to_string())?;
    let staged = seen_path.with_extension("json.tmp");
    fs::write(&staged, format!("{merged}\n")).and_then(|()| fs::rename(&staged, &seen_path)).map_err(|error| error.to_string())?;
    let yes = |text: &Option<String>| if text.is_some() { "yes" } else { "no" };
    Ok(format!("{} findings, {} new, urgent={}, normal={}", current.len(), fresh.len(), yes(&urgent), yes(&normal)))
}

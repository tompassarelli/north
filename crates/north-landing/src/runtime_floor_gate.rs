use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::LazyLock;

use regex::Regex;

use crate::platform::git as platform_git;

type Result<T> = std::result::Result<T, String>;

const USAGE: &str = "Usage: north runtime-floor-gate --repo DIR --base REV --head REV";
const COMMAND_TABLE: [&str; 4] = ["src/main.rs", "src/cli/mod.rs", "src/cli/tools.rs", "src/cli/session.rs"];

static LITERAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""([a-z][a-z0-9-]*)""#).expect("literal pattern"));
static CALL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:^|[\s;&|({`!])north\s+([a-z][a-z0-9-]*)").expect("call pattern"));

#[derive(Debug, Eq, PartialEq)]
pub struct Floor {
    pub sha: String,
    pub host: String,
}

fn git(repo: &Path, args: &[&str]) -> Result<String> {
    platform_git::stdout(repo, args, |output| format!("git {} failed: {}", args[0], String::from_utf8_lossy(&output.stderr).trim()))
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

fn succeeds(repo: &Path, args: &[&str]) -> bool {
    git(repo, args).is_ok()
}

pub fn commands(repo: &Path, rev: &str) -> BTreeSet<String> {
    COMMAND_TABLE
        .iter()
        .filter_map(|path| git(repo, &["show", &format!("{rev}:{path}")]).ok())
        .flat_map(|source| LITERAL.captures_iter(&source).map(|capture| capture[1].to_owned()).collect::<Vec<_>>())
        .collect()
}

pub fn calls(line: &str) -> Vec<String> {
    let code = line.trim_start();
    if code.starts_with('#') || code.starts_with("//") {
        return Vec::new();
    }
    CALL.captures_iter(code).map(|capture| capture[1].to_owned()).collect()
}

fn guarded(script: &str, command: &str) -> bool {
    script.contains(&format!("north {command} --help")) || script.contains(&format!("north_offers {command}"))
}

fn is_script(path: &str, text: &str) -> bool {
    text.starts_with("#!") || path.ends_with(".sh") || path.ends_with(".bash")
}

pub fn state_dir() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .unwrap_or_default()
        .join("north")
}

pub fn live_runtimes(state: &Path) -> Vec<Floor> {
    let mut live = Vec::new();
    if let Some(sha) = std::fs::read_link(state.join("current")).ok().and_then(|link| link.file_name().map(|name| name.to_string_lossy().into_owned())) {
        live.push(Floor { sha, host: "this host".to_owned() });
    }
    let record = std::fs::read_to_string(state.join("runtime-settle.json")).ok().and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
    for host in record.as_ref().and_then(|record| record["hosts"].as_array()).into_iter().flatten() {
        if let (Some(sha), Some(name)) = (host["runtime"].as_str(), host["host"].as_str()) {
            live.push(Floor { sha: sha.to_owned(), host: name.to_owned() });
        }
    }
    live
}

pub fn oldest(repo: &Path, live: Vec<Floor>, base: &str) -> Floor {
    live.into_iter()
        .filter(|floor| succeeds(repo, &["cat-file", "-e", &format!("{}^{{commit}}", floor.sha)]))
        .reduce(|oldest, next| if succeeds(repo, &["merge-base", "--is-ancestor", &next.sha, &oldest.sha]) { next } else { oldest })
        .unwrap_or_else(|| Floor { sha: base.to_owned(), host: "the landing's base".to_owned() })
}

pub fn violations(repo: &Path, base: &str, head: &str, floor: &str) -> Result<Vec<String>> {
    let newer: BTreeSet<String> = commands(repo, head).difference(&commands(repo, floor)).cloned().collect();
    if newer.is_empty() {
        return Ok(Vec::new());
    }
    let changed = git(repo, &["diff", "--name-only", "--diff-filter=AM", "-z", base, head])?;
    let mut found = Vec::new();
    for path in changed.split('\0').filter(|path| !path.is_empty()) {
        let Ok(script) = git(repo, &["show", &format!("{head}:{path}")]) else { continue };
        if !is_script(path, &script) {
            continue;
        }
        let diff = git(repo, &["diff", "--no-color", "--no-ext-diff", "--unified=0", base, head, "--", path])?;
        let added = diff.lines().filter(|line| !line.starts_with("+++")).filter_map(|line| line.strip_prefix('+'));
        let named: BTreeSet<String> = added.flat_map(calls).filter(|command| newer.contains(command) && !guarded(&script, command)).collect();
        found.extend(named.into_iter().map(|command| format!("{path}: north {command}")));
    }
    Ok(found)
}

fn report(repo: &Path, base: &str, head: &str) -> Result<bool> {
    let floor = oldest(repo, live_runtimes(&state_dir()), base);
    let found = violations(repo, base, head, &floor.sha)?;
    if found.is_empty() {
        println!("runtime-floor-gate: passed");
        return Ok(true);
    }
    let short = git(repo, &["rev-parse", "--short=8", &floor.sha]).map_or_else(|_| floor.sha.clone(), |sha| sha.trim().to_owned());
    for line in &found {
        println!("runtime-floor-gate: {line} is missing from the oldest live runtime {short} ({})", floor.host);
    }
    println!("runtime-floor-gate: land the subcommand first and call it once every host runs a runtime that has it (north runtime settle), or probe it with `north SUB --help` and say to run north runtime update when it is missing");
    Ok(false)
}

pub fn check(repo: &Path, base: &str, head: &str) -> bool {
    report(repo, base, head).unwrap_or_else(|error| {
        eprintln!("runtime-floor-gate: {error}");
        false
    })
}

pub fn run_gate(args: &[String]) -> ExitCode {
    let parsed = (|| {
        let (mut repo, mut base, mut head) = (None, None, None);
        let mut args = args.iter();
        while let Some(flag) = args.next() {
            let value = args.next().ok_or(USAGE)?;
            let slot = match flag.as_str() {
                "--repo" => &mut repo,
                "--base" => &mut base,
                "--head" => &mut head,
                _ => return Err(format!("unknown runtime-floor-gate option {flag}")),
            };
            if slot.replace(value.clone()).is_some() {
                return Err(format!("duplicate option {flag}"));
            }
        }
        repo.zip(base).zip(head).map(|((repo, base), head)| (repo, base, head)).ok_or_else(|| USAGE.to_owned())
    })();
    match parsed {
        Ok((repo, base, head)) if check(Path::new(&repo), &base, &head) => ExitCode::SUCCESS,
        Ok(_) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("runtime-floor-gate: {error}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
#[path = "runtime_floor_gate_tests.rs"]
mod tests;

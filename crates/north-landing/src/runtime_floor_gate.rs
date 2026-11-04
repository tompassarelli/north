use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::platform::git as platform_git;

#[path = "landing/runtime_floor_invocation.rs"]
mod invocation;
#[path = "landing/runtime_floor_surface.rs"]
mod surface;

pub use invocation::{Invocation, invocations};
pub use surface::{Gap, Surface};

type Result<T> = std::result::Result<T, String>;

const USAGE: &str = "Usage: north runtime-floor-gate --repo DIR --base REV --head REV [--north DIR]";
const NORTH_MAIN: &str = "north main";
const OWN_TABLE: &str = "src/cli/mod.rs";

#[derive(Debug, Eq, PartialEq)]
pub struct Floor {
    pub sha: String,
    pub host: String,
}

#[derive(Debug, Eq, PartialEq)]
pub struct Added {
    pub path: String,
    pub script: String,
    pub invocations: Vec<Invocation>,
}

#[derive(Debug, Eq, PartialEq)]
pub enum Verdict {
    Passed,
    Refused(Vec<String>),
}

fn git(repo: &Path, args: &[&str]) -> Result<String> {
    platform_git::stdout(repo, args, |output| format!("git {} failed: {}", args[0], String::from_utf8_lossy(&output.stderr).trim()))
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

fn succeeds(repo: &Path, args: &[&str]) -> bool {
    git(repo, args).is_ok()
}

pub fn commands(repo: &Path, rev: &str) -> BTreeSet<String> {
    Surface::read(repo, rev, &BTreeSet::new()).commands().clone()
}

fn guarded(script: &str, command: &str) -> bool {
    script.contains(&format!("north {command} --help")) || script.contains(&format!("north_offers {command}"))
}

fn is_script(path: &str, text: &str) -> bool {
    text.starts_with("#!") || path.ends_with(".sh") || path.ends_with(".bash") || path.ends_with(".nix")
}

pub fn state_dir() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .unwrap_or_default()
        .join("north")
}

pub fn north_checkout() -> PathBuf {
    std::env::var_os("NORTH_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join("code/north/main")))
        .unwrap_or_default()
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

pub fn oldest(repo: &Path, live: Vec<Floor>, fallback: Floor) -> Floor {
    live.into_iter()
        .filter(|floor| succeeds(repo, &["cat-file", "-e", &format!("{}^{{commit}}", floor.sha)]))
        .reduce(|oldest, next| if succeeds(repo, &["merge-base", "--is-ancestor", &next.sha, &oldest.sha]) { next } else { oldest })
        .unwrap_or(fallback)
}

pub fn added_invocations(repo: &Path, base: &str, head: &str) -> Result<Vec<Added>> {
    let changed = git(repo, &["diff", "--name-only", "--diff-filter=AM", "-z", base, head])?;
    let mut found = Vec::new();
    for path in changed.split('\0').filter(|path| !path.is_empty()) {
        let Ok(script) = git(repo, &["show", &format!("{head}:{path}")]) else { continue };
        if !is_script(path, &script) {
            continue;
        }
        let diff = git(repo, &["diff", "--no-color", "--no-ext-diff", "--unified=0", base, head, "--", path])?;
        let added = diff.lines().filter(|line| !line.starts_with("+++")).filter_map(|line| line.strip_prefix('+'));
        let invocations: Vec<Invocation> = added.flat_map(invocations).collect();
        if !invocations.is_empty() {
            found.push(Added { path: path.to_owned(), script, invocations });
        }
    }
    Ok(found)
}

pub fn refusals(own: bool, head: &Surface, floor: &Surface, invocation: &Invocation) -> Vec<Gap> {
    let beyond_head = head.gaps(invocation);
    floor.gaps(invocation).into_iter().filter(|gap| (!own && gap.is_flag()) || !beyond_head.contains(gap)).collect()
}

pub fn violations(added: &[Added], own: bool, head: &Surface, floor: &Surface) -> Vec<String> {
    let named: BTreeSet<String> = added
        .iter()
        .flat_map(|file| {
            file.invocations.iter().flat_map(move |invocation| {
                refusals(own, head, floor, invocation)
                    .into_iter()
                    .filter(move |gap| !matches!(gap, Gap::Command(command) if guarded(&file.script, command)))
                    .map(move |gap| format!("{}: {}", file.path, gap.name(invocation)))
            })
        })
        .collect();
    named.into_iter().collect()
}

pub fn render(found: &[String], floor: &str, host: &str) -> Vec<String> {
    let mut lines: Vec<String> = found.iter().map(|line| format!("runtime-floor-gate: {line} is missing from the oldest live runtime {floor} ({host})")).collect();
    lines.push("runtime-floor-gate: land the subcommand or flag first and call it once every host runs a runtime that has it (north runtime settle), or probe a subcommand with `north SUB --help` and say to run north runtime update when it is missing".to_owned());
    lines
}

fn is_north_source(repo: &Path, rev: &str) -> bool {
    succeeds(repo, &["cat-file", "-e", &format!("{rev}:{OWN_TABLE}")])
}

fn tip(north: &Path) -> Result<String> {
    ["origin/main", "HEAD"]
        .iter()
        .find_map(|rev| git(north, &["rev-parse", "--verify", "-q", &format!("{rev}^{{commit}}")]).ok())
        .map(|sha| sha.trim().to_owned())
        .ok_or_else(|| format!("cannot read north's source at {}; set NORTH_HOME to a north checkout", north.display()))
}

pub fn judge(repo: &Path, north: Option<&Path>, state: &Path, base: &str, head: &str) -> Result<Verdict> {
    let added = added_invocations(repo, base, head)?;
    if added.is_empty() {
        return Ok(Verdict::Passed);
    }
    let own = is_north_source(repo, head);
    let (source, head_rev, fallback) = if own {
        (repo.to_path_buf(), head.to_owned(), Floor { sha: base.to_owned(), host: "the landing's base".to_owned() })
    } else {
        let source = north.map_or_else(north_checkout, Path::to_path_buf);
        let tip = tip(&source)?;
        (source, tip.clone(), Floor { sha: tip, host: NORTH_MAIN.to_owned() })
    };
    let words: BTreeSet<String> = added.iter().flat_map(|file| &file.invocations).flat_map(|invocation| invocation.words.clone()).collect();
    let floor = oldest(&source, live_runtimes(state), fallback);
    let found = violations(&added, own, &Surface::read(&source, &head_rev, &words), &Surface::read(&source, &floor.sha, &words));
    if found.is_empty() {
        return Ok(Verdict::Passed);
    }
    let short = git(&source, &["rev-parse", "--short=8", &floor.sha]).map_or_else(|_| floor.sha.clone(), |sha| sha.trim().to_owned());
    Ok(Verdict::Refused(render(&found, &short, &floor.host)))
}

pub fn check(repo: &Path, base: &str, head: &str) -> bool {
    check_with(repo, None, base, head)
}

fn check_with(repo: &Path, north: Option<&Path>, base: &str, head: &str) -> bool {
    match judge(repo, north, &state_dir(), base, head) {
        Ok(Verdict::Passed) => {
            println!("runtime-floor-gate: passed");
            true
        }
        Ok(Verdict::Refused(lines)) => {
            lines.iter().for_each(|line| println!("{line}"));
            false
        }
        Err(error) => {
            eprintln!("runtime-floor-gate: {error}");
            false
        }
    }
}

pub fn run_gate(args: &[String]) -> ExitCode {
    let parsed = (|| {
        let (mut repo, mut base, mut head, mut north) = (None, None, None, None);
        let mut args = args.iter();
        while let Some(flag) = args.next() {
            let value = args.next().ok_or(USAGE)?;
            let slot = match flag.as_str() {
                "--repo" => &mut repo,
                "--base" => &mut base,
                "--head" => &mut head,
                "--north" => &mut north,
                _ => return Err(format!("unknown runtime-floor-gate option {flag}")),
            };
            if slot.replace(value.clone()).is_some() {
                return Err(format!("duplicate option {flag}"));
            }
        }
        repo.zip(base).zip(head).map(|((repo, base), head)| (repo, base, head, north)).ok_or_else(|| USAGE.to_owned())
    })();
    match parsed {
        Ok((repo, base, head, north)) if check_with(Path::new(&repo), north.as_deref().map(Path::new), &base, &head) => ExitCode::SUCCESS,
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

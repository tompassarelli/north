use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use north_base::build_inputs::{run_time_data, rust_build_input};

use crate::hunks::{self, Hunks};
use crate::map::{self, Map, Test};
use crate::pick::{Change, Selection, select as pick};

const DEFAULT_TIMEOUT: &str = "1800";
pub const MOST_BEFORE_PUSH: usize = 400;
const OLDEST_MAP_SECONDS: u64 = 3 * 86_400;

struct Request {
    base: String,
    head: String,
    map: Option<PathBuf>,
    timeout: String,
}

fn request(arguments: &[String]) -> Option<Request> {
    let [base, head, rest @ ..] = arguments else { return None };
    let mut request = Request { base: base.clone(), head: head.clone(), map: None, timeout: DEFAULT_TIMEOUT.to_owned() };
    let mut rest = rest.iter();
    while let Some(flag) = rest.next() {
        match (flag.as_str(), rest.next()) {
            ("--map", Some(path)) => request.map = Some(PathBuf::from(path)),
            ("--timeout", Some(seconds)) if seconds.parse::<u64>().is_ok() => request.timeout.clone_from(seconds),
            _ => return None,
        }
    }
    Some(request)
}

pub fn select(arguments: &[String]) -> u8 {
    let Some(request) = request(arguments) else { return usage() };
    match selection_between(&request.base, &request.head, request.map.clone()) {
        Ok(selection) => {
            report(&selection);
            if let Selection::Tests { tests, .. } = &selection
                && !tests.is_empty()
            {
                println!("{}", nextest_arguments(tests).join(" "));
            }
            0
        }
        Err(why) => fail(&why),
    }
}

pub fn test(arguments: &[String]) -> u8 {
    let Some(request) = request(arguments) else { return usage() };
    let selection = match selection_between(&request.base, &request.head, request.map.clone()) {
        Ok(selection) => selection,
        Err(why) => {
            eprintln!("north coverage: {why}; no tests run before the push, the full suite runs after it");
            return 0;
        }
    };
    report(&selection);
    let Selection::Tests { tests, .. } = selection else { return 0 };
    if tests.is_empty() {
        return 0;
    }
    if tests.len() > MOST_BEFORE_PUSH {
        eprintln!("north coverage: {} tests is over the {MOST_BEFORE_PUSH} run before a push; the full suite runs after it", tests.len());
        return 0;
    }
    let command = format!("CARGO_TARGET_DIR=target ./bin/land-shell --dev -- cargo nextest run --locked --offline --no-tests=pass {}", nextest_arguments(&tests).join(" "));
    let host = host();
    eprintln!("north coverage: running {} selected tests on {host}", tests.len());
    let status = Command::new(north()).args(["capacity", "offload", "run", "--host", &host, "--base", &request.base, "--head", &request.head, "--timeout", &request.timeout, "--", &command]).status();
    status.ok().and_then(|status| status.code()).map_or(1, |code| u8::try_from(code).unwrap_or(1))
}

fn usage() -> u8 {
    eprintln!("usage: north coverage select|test BASE HEAD [--map FILE] [--timeout SECONDS]");
    2
}

fn fail(why: &str) -> u8 {
    eprintln!("north coverage: {why}");
    1
}

fn report(selection: &Selection) {
    match selection {
        Selection::Tests { tests, file_level } if file_level.is_empty() => eprintln!("north coverage: {} tests reach the changed functions", tests.len()),
        Selection::Tests { tests, file_level } => eprintln!("north coverage: {} tests reach the changed code; the map is older than the change in {}, so those files select every test reaching them", tests.len(), file_level.join(", ")),
        Selection::Unmapped(why) => eprintln!("north coverage: {why}; no tests run before the push, the full suite runs after it"),
    }
}

pub fn selection_between(base: &str, head: &str, map: Option<PathBuf>) -> Result<Selection, String> {
    let root = PathBuf::from(git(Path::new("."), &["rev-parse", "--show-toplevel"])?);
    let path = map.unwrap_or_else(|| map::path_for(&repository(&root)));
    let map = Map::load(&path)?;
    let age = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |now| now.as_secs()).saturating_sub(map.recorded);
    if age > OLDEST_MAP_SECONDS {
        return Ok(Selection::Unmapped(format!("the coverage map is {} h old, over the {} h a selection trusts", age / 3600, OLDEST_MAP_SECONDS / 3600)));
    }
    let changed: Vec<String> = git(&root, &["diff", "--name-only", "--no-renames", base, head])?.lines().map(str::to_owned).collect();
    let rust: Vec<&str> = changed.iter().map(String::as_str).filter(|path| path.ends_with(".rs") && *path != "build.rs").collect();
    let other_paths: Vec<String> = changed.iter().filter(|path| !path.ends_with(".rs") || *path == "build.rs").filter(|path| rust_build_input(path) || run_time_data(path) || path.starts_with("tests/")).cloned().collect();
    let lane = diff(&root, base, head, &rust)?;
    let since_map = if rust.is_empty() { Some(Hunks::new()) } else { diff(&root, &map.commit, base, &rust).ok() };
    Ok(pick(&Change { map: &map, since_map: since_map.as_ref(), lane: &lane, other_paths: &other_paths }))
}

fn diff(root: &Path, from: &str, to: &str, paths: &[&str]) -> Result<Hunks, String> {
    if paths.is_empty() {
        return Ok(Hunks::new());
    }
    let mut arguments = vec!["diff", "-U0", "--no-renames", "--no-color", "--no-ext-diff", from, to, "--"];
    arguments.extend(paths);
    git(root, &arguments).map(|text| hunks::parse(&text))
}

fn git(root: &Path, arguments: &[&str]) -> Result<String, String> {
    let output = Command::new("git").arg("-C").arg(root).args(arguments).output().map_err(|error| format!("git: {error}"))?;
    if output.status.success() { Ok(String::from_utf8_lossy(&output.stdout).trim_end().to_owned()) } else { Err(format!("git {} failed: {}", arguments.join(" "), String::from_utf8_lossy(&output.stderr).trim())) }
}

pub fn repository(root: &Path) -> String {
    let url = git(root, &["config", "--get", "remote.origin.url"]).unwrap_or_default();
    let name = url.rsplit(['/', ':']).next().unwrap_or_default().trim_end_matches(".git");
    if name.is_empty() { root.file_name().map_or_else(String::new, |name| name.to_string_lossy().into_owned()) } else { name.to_owned() }
}

pub fn north() -> std::ffi::OsString {
    std::env::current_exe().map_or_else(|_| "north".into(), PathBuf::into_os_string)
}

pub fn host() -> String {
    let here = north_base::platform::host::name();
    let chosen = Command::new(north()).args(["capacity", "offload", "choose", "--any", "--avoid", &here]).output();
    let chosen = chosen.ok().filter(|output| output.status.success()).map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned()).unwrap_or_default();
    if !chosen.is_empty() {
        return chosen;
    }
    let listed = git(Path::new("."), &["config", "--file", ".safe-push", "--get", "landing.remote-hosts"]).unwrap_or_default();
    listed.split_whitespace().next().map_or(here, str::to_owned)
}

pub fn nextest_arguments(tests: &BTreeSet<Test>) -> Vec<String> {
    let mut packages = BTreeSet::new();
    let mut targets = BTreeSet::new();
    for test in tests {
        let (package, target) = test.binary.split_once("::").unwrap_or((&test.binary, ""));
        packages.insert(format!("-p {package}"));
        targets.insert(match target {
            "" => "--lib".to_owned(),
            bin if bin.starts_with("bin/") => format!("--bin {}", &bin[4..]),
            integration => format!("--test {integration}"),
        });
    }
    let filter = tests.iter().map(|test| format!("(binary_id(={}) & test(={}))", test.binary, test.name)).collect::<Vec<_>>().join(" | ");
    packages.into_iter().chain(targets).chain([format!("-E '{filter}'")]).collect()
}

#[cfg(test)]
#[path = "run_selected_tests.rs"]
mod tests;

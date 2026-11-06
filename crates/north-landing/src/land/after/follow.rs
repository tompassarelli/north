use std::fs::{self, File};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::land::telemetry::epoch;
use crate::land::trace::{self, AFTER, Category, PHASE, Span};

const USAGE: &str = "usage: north land follow --repo DIR --git-dir DIR --base SHA --head SHA --safe-push PATH";

pub fn program() -> String {
    std::env::var("NORTH_LAND_NORTH").ok().filter(|value| !value.is_empty()).unwrap_or_else(|| "north".to_owned())
}

pub fn executable(name: &str) -> PathBuf {
    if name.contains('/') {
        return fs::canonicalize(name).unwrap_or_else(|_| PathBuf::from(name));
    }
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
        .map_or_else(|| PathBuf::from(name), |found| fs::canonicalize(&found).unwrap_or(found))
}

pub struct Follow<'a> {
    pub repo: &'a Path,
    pub git_dir: &'a Path,
    pub base: &'a str,
    pub head: &'a str,
    pub safe_push: &'a Path,
    pub log: &'a Path,
}

pub fn spawn(follow: &Follow) -> Result<(), String> {
    if let Some(parent) = follow.log.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    let log = File::create(follow.log).map_err(|error| format!("{}: {error}", follow.log.display()))?;
    let errors = log.try_clone().map_err(|error| error.to_string())?;
    let executable = std::env::current_exe().map_err(|error| format!("current_exe: {error}"))?;
    let mut child = Command::new(executable)
        .args(["land", "follow", "--repo"])
        .arg(follow.repo)
        .arg("--git-dir")
        .arg(follow.git_dir)
        .args(["--base", follow.base, "--head", follow.head])
        .arg("--safe-push")
        .arg(follow.safe_push)
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(errors)
        .env(trace::SPANS_ENV, follow.log.parent().map(|logs| trace::Spans::beside(logs).path().to_owned()).unwrap_or_default())
        .process_group(0)
        .spawn()
        .map_err(|error| format!("north land follow: {error}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

fn step(program: &str, arguments: &[&std::ffi::OsStr]) {
    println!("north land follow: {program} {}", arguments.iter().map(|argument| argument.to_string_lossy()).collect::<Vec<_>>().join(" "));
    match Command::new(program).args(arguments).stdin(Stdio::null()).status() {
        Ok(status) if status.success() => {}
        Ok(status) => println!("north land follow: {program} exited {status}"),
        Err(error) => println!("north land follow: {program}: {error}"),
    }
}

pub fn run(arguments: &[String]) -> u8 {
    let mut values = std::collections::HashMap::new();
    let mut rest = arguments.iter();
    while let Some(flag) = rest.next() {
        match (flag.as_str(), rest.next()) {
            ("--repo" | "--git-dir" | "--base" | "--head" | "--safe-push", Some(value)) => {
                values.insert(flag.as_str(), value.as_str());
            }
            _ => {
                eprintln!("{USAGE}");
                return 64;
            }
        }
    }
    let (Some(repo), Some(git_dir), Some(base), Some(head), Some(safe_push)) = (values.get("--repo"), values.get("--git-dir"), values.get("--base"), values.get("--head"), values.get("--safe-push")) else {
        eprintln!("{USAGE}");
        return 64;
    };
    let north = program();
    let os = std::ffi::OsStr::new;
    let host = trace::this_host();
    let timed = |name: &str, category: Category, arguments: &[&std::ffi::OsStr]| {
        let start = epoch();
        step(&north, arguments);
        Span::new(head, name, category, start, epoch()).on(&host).with(PHASE, AFTER)
    };
    let spans = [
        timed("after-landing gates", Category::Ci, &[os("after-landing"), os("--git-dir"), os(git_dir), os("--base"), os(base), os("--head"), os(head), os("--safe-push"), os(safe_push)]),
        timed("follows-release", Category::Follow, &[os("follows-release"), os("--repo-dir"), os(repo)]),
        timed("runtime follow", Category::Follow, &[os("runtime"), os("follow"), os("--repo-dir"), os(repo)]),
        timed("graph ingest", Category::Follow, &[os("graph"), os("ingest")]),
    ];
    trace::record(&spans);
    0
}

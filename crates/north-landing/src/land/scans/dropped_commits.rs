use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use super::git;

const USAGE: &str = "usage: north land scan dropped-commits --lane TIP --head HEAD --onto BRANCH [--destination REVISION]";

#[derive(Default)]
struct Arguments {
    lane: String,
    head: String,
    onto: String,
    destination: String,
}

fn parse(arguments: &[String]) -> Option<Arguments> {
    let mut parsed = Arguments::default();
    let mut rest = arguments.iter();
    while let Some(flag) = rest.next() {
        let value = rest.next()?.clone();
        match flag.as_str() {
            "--lane" => parsed.lane = value,
            "--head" => parsed.head = value,
            "--onto" => parsed.onto = value,
            "--destination" => parsed.destination = value,
            _ => return None,
        }
    }
    (!parsed.head.is_empty()).then_some(parsed)
}

fn patch_ids(directory: &Path, range: &str) -> Result<Vec<(String, String)>, String> {
    let log = git::bytes(directory, &["log", "-p", "-U0", "--no-merges", range])
        .ok_or_else(|| format!("git log {range} failed"))?;
    let mut child = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(["patch-id", "--stable"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|error| format!("git patch-id: {error}"))?;
    let mut stdin = child.stdin.take().ok_or("git patch-id has no stdin")?;
    let writer = std::thread::spawn(move || stdin.write_all(&log));
    let output = child.wait_with_output().map_err(|error| format!("git patch-id: {error}"))?;
    writer.join().map_err(|_| "git patch-id writer panicked")?.map_err(|error| format!("git patch-id: {error}"))?;
    if !output.status.success() {
        return Err("git patch-id failed".to_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split_once(' '))
        .map(|(id, commit)| (id.to_owned(), commit.to_owned()))
        .collect())
}

fn count(directory: &Path, range: &str) -> Option<String> {
    git::text(directory, &["rev-list", "--count", "--no-merges", range]).map(|text| text.trim().to_owned())
}

fn landed_whole(directory: &Path, destination: &str, lane: &str, head: &str, fork: &str) -> bool {
    let unlanded = git::text(directory, &["cherry", destination, lane]).unwrap_or_default();
    if !unlanded.lines().any(|line| line.starts_with('+')) {
        return true;
    }
    let merged = git::text(directory, &["merge-tree", "--write-tree", destination, lane]).and_then(|text| text.lines().next().map(str::to_owned));
    let tree = git::text(directory, &["rev-parse", &format!("{head}^{{tree}}")]).map(|text| text.trim().to_owned());
    merged.is_some()
        && merged == tree
        && count(directory, &format!("{destination}..{head}")).is_some_and(|picked| Some(picked) == count(directory, &format!("{fork}..{lane}")))
}

pub fn missing(directory: &Path, lane: &str, head: &str, destination: Option<&str>) -> Result<Vec<String>, String> {
    let Some(fork) = git::text(directory, &["merge-base", lane, head]).map(|text| text.trim().to_owned()) else {
        return Ok(Vec::new());
    };
    let present: BTreeSet<String> = patch_ids(directory, &format!("{fork}..{head}"))?.into_iter().map(|(id, _)| id).collect();
    let missing: Vec<String> = patch_ids(directory, &format!("{fork}..{lane}"))?
        .into_iter()
        .filter(|(id, _)| !present.contains(id))
        .map(|(_, commit)| commit)
        .collect();
    if missing.is_empty() || destination.is_some_and(|destination| landed_whole(directory, destination, lane, head, &fork)) {
        return Ok(Vec::new());
    }
    Ok(missing)
}

fn destination(directory: &Path, arguments: &Arguments) -> Option<String> {
    if !arguments.destination.is_empty() {
        return Some(arguments.destination.clone());
    }
    let reference = format!("refs/remotes/origin/{}^{{commit}}", arguments.onto);
    git::text(directory, &["rev-parse", "-q", "--verify", &reference]).map(|text| text.trim().to_owned())
}

pub fn run(directory: &Path, arguments: &[String]) -> u8 {
    let Some(arguments) = parse(arguments) else {
        eprintln!("{USAGE}");
        return 64;
    };
    if arguments.lane.is_empty() {
        return 0;
    }
    match missing(directory, &arguments.lane, &arguments.head, destination(directory, &arguments).as_deref()) {
        Ok(missing) if missing.is_empty() => 0,
        Ok(missing) => {
            eprintln!(
                "safe-push: ✗ lane commits missing from {} after the rebase onto origin {}: {}; NOT pushed.",
                arguments.head,
                arguments.onto,
                missing.join(" ")
            );
            1
        }
        Err(error) => {
            eprintln!("safe-push: ✗ cannot check the rebase onto origin {} for dropped lane commits: {error}; NOT pushed.", arguments.onto);
            1
        }
    }
}

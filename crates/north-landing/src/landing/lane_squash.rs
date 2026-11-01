use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

const USAGE: &str = "usage: north lane-squash [--repo DIR] [--message FILE] DESTINATION_TIP";

#[derive(Debug, Eq, PartialEq)]
pub enum Squash {
    Unchanged,
    Kept(String),
    Squashed { commits: usize, head: String },
}

struct LaneCommit {
    subject: String,
    body: String,
    trailers: Vec<String>,
    merge: bool,
}

fn git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|error| format!("git {}: {error}", args.join(" ")))?;
    if !output.status.success() {
        return Err(format!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn succeeds(repo: &Path, args: &[&str]) -> bool {
    Command::new("git").arg("-C").arg(repo).args(args).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|status| status.success())
}

fn is_trailer(line: &str) -> bool {
    let Some((key, value)) = line.split_once(": ") else { return false };
    !value.trim().is_empty()
        && key.chars().next().is_some_and(|first| first.is_ascii_alphanumeric())
        && key.chars().all(|character| character.is_ascii_alphanumeric() || character == '-')
}

fn split_trailers(message: &str) -> (String, Vec<String>) {
    let message = message.trim_end();
    let Some((head, last)) = message.rsplit_once("\n\n") else { return (message.to_owned(), Vec::new()) };
    let lines: Vec<&str> = last.lines().filter(|line| !line.trim().is_empty()).collect();
    if lines.first().is_none_or(|line| !is_trailer(line))
        || !lines.iter().all(|line| is_trailer(line) || line.starts_with([' ', '\t']))
    {
        return (message.to_owned(), Vec::new());
    }
    let mut trailers: Vec<String> = Vec::new();
    for line in lines {
        match trailers.last_mut() {
            Some(previous) if line.starts_with([' ', '\t']) => {
                previous.push(' ');
                previous.push_str(line.trim());
            }
            _ => trailers.push(line.trim_end().to_owned()),
        }
    }
    (head.trim_end().to_owned(), trailers)
}

fn lane_commit(repo: &Path, oid: &str) -> Result<LaneCommit, String> {
    let parents = git(repo, &["show", "-s", "--format=%P", oid])?;
    let message = git(repo, &["show", "-s", "--format=%B", oid])?;
    let (text, trailers) = split_trailers(&message);
    let (subject, body) = text.split_once("\n\n").map_or((text.as_str(), ""), |(subject, body)| (subject, body));
    Ok(LaneCommit {
        subject: subject.trim().to_owned(),
        body: body.trim().to_owned(),
        trailers,
        merge: parents.split_whitespace().count() > 1,
    })
}

fn message(commits: &[&LaneCommit], supplied: Option<&str>) -> String {
    let mut text = match supplied {
        Some(supplied) => split_trailers(supplied).0,
        None => {
            let first = commits[0];
            let mut text = first.subject.clone();
            if !first.body.is_empty() {
                text.push_str("\n\n");
                text.push_str(&first.body);
            }
            let others: Vec<String> = commits[1..].iter().map(|commit| format!("- {}", commit.subject)).collect();
            if !others.is_empty() {
                text.push_str("\n\n");
                text.push_str(&others.join("\n"));
            }
            text
        }
    };
    let mut seen = BTreeSet::new();
    let supplied_trailers = supplied.map(|supplied| split_trailers(supplied).1).unwrap_or_default();
    let trailers: Vec<&String> = supplied_trailers
        .iter()
        .chain(commits.iter().flat_map(|commit| commit.trailers.iter()))
        .filter(|trailer| seen.insert(trailer.as_str()))
        .collect();
    if !trailers.is_empty() {
        text.push_str("\n\n");
        text.push_str(&trailers.iter().map(|trailer| trailer.as_str()).collect::<Vec<_>>().join("\n"));
    }
    text.push('\n');
    text
}

fn published_tips(repo: &Path, destination: &str) -> Result<Vec<String>, String> {
    let listing = git(repo, &["ls-remote", "origin"])?;
    let query: String = listing.lines().filter_map(|line| line.split_whitespace().next()).map(|oid| format!("{oid}\n")).collect();
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["cat-file", "--batch-check=%(objectname) %(objecttype)"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("git cat-file: {error}"))?;
    let mut stdin = child.stdin.take().ok_or("git cat-file: no stdin")?;
    let writer = std::thread::spawn(move || stdin.write_all(query.as_bytes()));
    let present = child.wait_with_output().map_err(|error| format!("git cat-file: {error}"))?;
    writer.join().map_err(|_| "git cat-file: stdin writer panicked".to_owned())?.map_err(|error| format!("git cat-file: {error}"))?;
    let mut tips: Vec<String> = String::from_utf8_lossy(&present.stdout)
        .lines()
        .filter_map(|line| line.strip_suffix(" commit"))
        .map(str::to_owned)
        .collect();
    tips.push(destination.to_owned());
    tips.sort();
    tips.dedup();
    Ok(tips)
}

fn squash_parent(repo: &Path, boundaries: &[String], destination: &str) -> Option<String> {
    let merge_base = git(repo, &["merge-base", "HEAD", destination]).ok().map(|base| base.trim().to_owned());
    merge_base.into_iter().chain(boundaries.iter().cloned()).find(|candidate| {
        boundaries.iter().all(|boundary| boundary == candidate || succeeds(repo, &["merge-base", "--is-ancestor", boundary, candidate]))
    })
}

pub fn squash(repo: &Path, destination: &str, supplied: Option<&str>) -> Result<Squash, String> {
    let tips = published_tips(repo, destination)?;
    let mut walk = vec!["rev-list".to_owned(), "--reverse".to_owned(), "--topo-order".to_owned(), "--boundary".to_owned(), "HEAD".to_owned(), "--not".to_owned()];
    walk.extend(tips);
    let walk: Vec<&str> = walk.iter().map(String::as_str).collect();
    let listing = git(repo, &walk)?;
    let boundaries: Vec<String> = listing.lines().filter_map(|line| line.strip_prefix('-')).map(str::to_owned).collect();
    let lane: Vec<&str> = listing.lines().filter(|line| !line.starts_with('-') && !line.is_empty()).collect();
    if lane.is_empty() || (lane.len() < 2 && supplied.is_none()) {
        return Ok(Squash::Unchanged);
    }
    let Some(parent) = squash_parent(repo, &boundaries, destination) else {
        return Ok(Squash::Kept(format!(
            "its {} commits rest on published commits that are not one line of history; landing them as they are",
            lane.len()
        )));
    };
    let commits = lane.iter().map(|oid| lane_commit(repo, oid)).collect::<Result<Vec<_>, _>>()?;
    let authored: Vec<&LaneCommit> = commits.iter().filter(|commit| !commit.merge).collect();
    if authored.is_empty() {
        return Ok(Squash::Unchanged);
    }
    let first = lane.iter().zip(&commits).find(|(_, commit)| !commit.merge).map(|(oid, _)| *oid).unwrap_or(lane[0]);
    let author = git(repo, &["show", "-s", "--format=%an%x00%ae%x00%ad", "--date=raw", first])?;
    let mut author = author.trim_end_matches('\n').split('\0');
    let (name, email, date) = (author.next().unwrap_or(""), author.next().unwrap_or(""), author.next().unwrap_or(""));
    let old_head = git(repo, &["rev-parse", "HEAD"])?.trim().to_owned();
    let text = message(&authored, supplied);
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["commit-tree", "HEAD^{tree}", "-p", &parent, "-F", "-"])
        .env("GIT_AUTHOR_NAME", name)
        .env("GIT_AUTHOR_EMAIL", email)
        .env("GIT_AUTHOR_DATE", date)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("git commit-tree: {error}"))?;
    child.stdin.take().ok_or("git commit-tree: no stdin")?.write_all(text.as_bytes()).map_err(|error| format!("git commit-tree: {error}"))?;
    let output = child.wait_with_output().map_err(|error| format!("git commit-tree: {error}"))?;
    if !output.status.success() {
        return Err(format!("git commit-tree failed: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    let head = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    git(repo, &["update-ref", "-m", "safe-push: land the lane as one commit", "HEAD", &head, &old_head])?;
    Ok(Squash::Squashed { commits: lane.len(), head })
}

pub fn run(arguments: &[String]) -> u8 {
    if let [flag] = arguments
        && (flag == "-h" || flag == "--help")
    {
        println!("{USAGE}");
        return 0;
    }
    let (repo, rest) = match arguments {
        [flag, dir, rest @ ..] if flag == "--repo" => (Path::new(dir), rest),
        rest => (Path::new("."), rest),
    };
    let (supplied, rest) = match rest {
        [flag, file, rest @ ..] if flag == "--message" => match std::fs::read_to_string(file) {
            Ok(text) if !text.trim().is_empty() => (Some(text), rest),
            Ok(_) => {
                eprintln!("safe-push: ✗ --message {file} is empty; NOT pushed.");
                return 1;
            }
            Err(error) => {
                eprintln!("safe-push: ✗ cannot read --message {file}: {error}; NOT pushed.");
                return 1;
            }
        },
        rest => (None, rest),
    };
    let [destination] = rest else {
        eprintln!("{USAGE}");
        return 64;
    };
    match squash(repo, destination, supplied.as_deref()) {
        Ok(Squash::Unchanged) => 0,
        Ok(Squash::Kept(reason)) => {
            println!("safe-push: lane not squashed: {reason}");
            0
        }
        Ok(Squash::Squashed { commits, head }) => {
            println!("safe-push: landing the lane's {commits} unpublished commits as one commit {head} (--several-commits REASON keeps them apart)");
            0
        }
        Err(error) => {
            eprintln!("safe-push: ✗ cannot land the lane as one commit: {error}; NOT pushed.");
            1
        }
    }
}

use std::path::Path;
use std::process::{Command, Output};

use crate::error::{NorthError, NorthResult};

use super::configuration;
use super::live_refs::{Facts, SCHEMA};

const REF_PREFIX: &str = "refs/north/host-facts/";
const FILE: &str = "facts.json";

fn git(repo: &Path, arguments: &[&str], stdin: Option<&[u8]>) -> NorthResult<Output> {
    use std::io::Write;
    use std::process::Stdio;
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(repo)
        .args(arguments)
        .env("GIT_AUTHOR_NAME", "north host facts")
        .env("GIT_AUTHOR_EMAIL", "host-facts@north.invalid")
        .env("GIT_COMMITTER_NAME", "north host facts")
        .env("GIT_COMMITTER_EMAIL", "host-facts@north.invalid")
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| NorthError::Configuration(format!("cannot run git in {}: {error}", repo.display())))?;
    if let (Some(bytes), Some(mut pipe)) = (stdin, child.stdin.take()) {
        pipe.write_all(bytes)?;
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return configuration(format!("git {} failed in {}: {}", arguments.join(" "), repo.display(), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(output)
}

fn text(output: Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

pub fn reference(host: &str) -> String {
    format!("{REF_PREFIX}{host}")
}

const PRIVATE_REPO: &str = "south";

fn require_private(repo: &Path, remote: &str) -> NorthResult<()> {
    let url = text(git(repo, &["remote", "get-url", remote], None)?);
    let url = url.trim_end_matches('/');
    let url = url.strip_suffix(".git").unwrap_or(url);
    if url.rsplit(['/', ':']).next() == Some(PRIVATE_REPO) {
        return Ok(());
    }
    configuration(format!("host facts name private host configuration, so remote {remote} of {} ({url}) must be the private {PRIVATE_REPO} repository", repo.display()))
}

pub fn publish(repo: &Path, remote: &str, facts: &Facts) -> NorthResult<String> {
    require_private(repo, remote)?;
    let reference = reference(&facts.host);
    let fetch = format!("+{reference}:{reference}");
    let known = git(repo, &["fetch", "-q", "--no-tags", remote, &fetch], None).is_ok();
    let blob = text(git(repo, &["hash-object", "-w", "--stdin"], Some(&serde_json::to_vec_pretty(facts)?))?);
    let tree = text(git(repo, &["mktree"], Some(format!("100644 blob {blob}\t{FILE}\n").as_bytes()))?);
    let mut commit_arguments = vec!["commit-tree", &tree, "-m", "host facts"];
    let parent = if known { text(git(repo, &["rev-parse", "--verify", &reference], None)?) } else { String::new() };
    if known {
        commit_arguments.extend(["-p", &parent]);
    }
    let commit = text(git(repo, &commit_arguments, None)?);
    git(repo, &["push", "-q", remote, &format!("{commit}:{reference}")], None)?;
    Ok(commit)
}

pub fn fetch_all(repo: &Path, remote: &str) -> NorthResult<Vec<Facts>> {
    require_private(repo, remote)?;
    let fetch = format!("+{REF_PREFIX}*:{REF_PREFIX}*");
    git(repo, &["fetch", "-q", "--no-tags", remote, &fetch], None)?;
    let listing = text(git(repo, &["for-each-ref", "--format=%(refname)", REF_PREFIX], None)?);
    let mut all = Vec::new();
    for reference in listing.lines() {
        let blob = text(git(repo, &["show", &format!("{reference}:{FILE}")], None)?);
        let facts: Facts = serde_json::from_str(&blob).map_err(|error| NorthError::Configuration(format!("cannot parse {reference}: {error}")))?;
        if facts.schema == SCHEMA {
            all.push(facts);
        }
    }
    all.sort_by(|a, b| a.host.cmp(&b.host));
    Ok(all)
}

#[cfg(test)]
#[path = "live_refs_transport_tests.rs"]
mod tests;

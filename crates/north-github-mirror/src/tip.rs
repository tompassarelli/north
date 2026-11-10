use std::path::Path;
use std::process::Command;

use serde_json::Value;

use crate::store::{BRANCHES, Mirror, OWNER, REPOS, key};

pub const TIP_FRESH_S: f64 = 60.0;

pub fn mirrored_tips(mirror: &Mirror, repo: &str, at: f64) -> Option<Vec<(String, String)>> {
    let copy = mirror.read(&key(repo, BRANCHES)).filter(|copy| at - copy.synced <= TIP_FRESH_S)?;
    let tips: Vec<(String, String)> = copy.body.as_array()?.iter().filter_map(tip).collect();
    (!tips.is_empty()).then_some(tips)
}

fn tip(node: &Value) -> Option<(String, String)> {
    Some((node["name"].as_str()?.to_owned(), node["target"]["oid"].as_str()?.to_owned()))
}

pub fn holds_every_tip(mirror: &Mirror, repo: &str, at: f64, local: impl Fn(&str) -> Option<String>) -> bool {
    mirrored_tips(mirror, repo, at).is_some_and(|tips| tips.iter().all(|(branch, oid)| local(branch).as_deref() == Some(oid.as_str())))
}

fn git(directory: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(directory).args(args).output().ok().filter(|out| out.status.success())?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn repo_of(url: &str) -> Option<&str> {
    let tail = url.trim_end_matches('/').trim_end_matches(".git");
    let (head, name) = tail.rsplit_once('/')?;
    head.ends_with(OWNER).then_some(name).filter(|name| REPOS.contains(name))
}

pub fn run(arguments: &[String]) -> u8 {
    let directory = match arguments {
        [flag, directory] if flag == "-C" => directory.as_str(),
        [] => ".",
        _ => {
            eprintln!("usage: north github-mirror fetch [-C DIR]");
            return 2;
        }
    };
    let directory = Path::new(directory);
    let skip = git(directory, &["remote", "get-url", "origin"]).is_some_and(|url| {
        repo_of(&url).is_some_and(|repo| holds_every_tip(&Mirror::local(), repo, crate::read::now(), |branch| git(directory, &["rev-parse", "--verify", "--quiet", &format!("refs/remotes/origin/{branch}")])))
    });
    if skip {
        return 0;
    }
    let status = Command::new("git").arg("-C").arg(directory).args(["fetch", "origin"]).status();
    u8::from(!status.is_ok_and(|status| status.success()))
}

#[cfg(test)]
#[path = "tip_tests.rs"]
mod tests;

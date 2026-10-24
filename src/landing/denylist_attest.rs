use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::denylist;
use crate::platform::git as platform_git;
use crate::platform::github::{BranchTip, CommitStatuses, Gh};

const USAGE: &str = "usage: north denylist attest --south DIR --repo OWNER/NAME [--repo ...] [--branch claude/X]";
const CONTEXT: &str = "denylist";
const PREFIX: &str = "claude/";
const WORKFLOW: &str = "autoland.yml";
const CLEAN: &str = "deny-list clean";
const REFUSED: &str = "deny-list refused: reword and push again";

#[derive(Debug, Eq, PartialEq)]
struct Request {
    south: PathBuf,
    repos: Vec<String>,
    branch: Option<String>,
}

#[derive(Debug, Eq, PartialEq)]
enum Outcome {
    Skipped,
    Redispatched,
    Clean,
    Refused(BTreeSet<String>),
}

struct Attester<'a, F: CommitStatuses> {
    forge: &'a F,
    config: &'a Path,
    cache: &'a Path,
}

fn valid_repo(repo: &str) -> bool {
    let parts: Vec<&str> = repo.split('/').collect();
    parts.len() == 2 && parts.iter().all(|part| !part.is_empty() && !part.starts_with('.'))
}

fn parse(args: &[String]) -> Option<Request> {
    let (mut south, mut repos, mut branch) = (None, Vec::new(), None);
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--south" => south = Some(PathBuf::from(rest.next()?)),
            "--repo" => repos.push(rest.next()?.clone()),
            "--branch" => branch = Some(rest.next()?.clone()),
            _ => return None,
        }
    }
    let branch_ok = branch.as_deref().is_none_or(|name: &str| name.len() > PREFIX.len() && name.starts_with(PREFIX));
    (branch_ok && !repos.is_empty() && repos.iter().all(|repo| valid_repo(repo)))
        .then_some(Request { south: south?, repos, branch })
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    denylist::git(root, args)
}

fn merge_base(root: &Path, head: &str) -> Result<String, String> {
    let output = platform_git::run(root, &["merge-base", "refs/heads/main", head]).map_err(|e| e.to_string())?;
    match output.status.code() {
        Some(0) => Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned()),
        Some(1) if output.stdout.is_empty() => Ok(String::new()),
        _ => Err(format!("git merge-base failed: {}", String::from_utf8_lossy(&output.stderr).trim())),
    }
}

impl<F: CommitStatuses> Attester<'_, F> {
    fn tips(&self, repo: &str, branch: Option<&str>) -> Result<Vec<BranchTip>, String> {
        let tips = self.forge.matching_branches(repo, branch.unwrap_or(PREFIX))?;
        Ok(tips.into_iter().filter(|tip| tip.branch.starts_with(PREFIX) && branch.is_none_or(|name| tip.branch == name)).collect())
    }

    fn mirror(&self, repo: &str, tip: &BranchTip) -> Result<PathBuf, String> {
        let dir = self.cache.join(format!("{repo}.git"));
        if !dir.join("HEAD").is_file() {
            std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create the cache clone {}: {e}", dir.display()))?;
            git(&dir, &["init", "-q", "--bare"])?;
        }
        let url = self.forge.clone_url(repo);
        let tip_ref = format!("+refs/heads/{0}:refs/heads/{0}", tip.branch);
        git(&dir, &["fetch", "-q", "--no-tags", "--end-of-options", &url, "+refs/heads/main:refs/heads/main", &tip_ref])?;
        git(&dir, &["rev-parse", "--verify", "--end-of-options", &format!("{}^{{commit}}", tip.sha)])
            .map_err(|_| format!("{} moved past {} before it was fetched", tip.branch, tip.sha))?;
        Ok(dir)
    }

    fn scan(&self, repo: &str, tip: &BranchTip) -> Result<BTreeSet<String>, String> {
        let dir = self.mirror(repo, tip)?;
        let base = merge_base(&dir, &tip.sha)?;
        denylist::scan_range(&dir, self.config, &base, &tip.sha, Some(&tip.branch))
    }

    fn dispatched(&self, repo: &str, tip: &BranchTip) -> PathBuf {
        self.cache.join(format!("{repo}.dispatched")).join(&tip.sha)
    }

    fn dispatch(&self, repo: &str, tip: &BranchTip) -> Result<(), String> {
        self.forge
            .dispatch_workflow(repo, WORKFLOW, &tip.branch)
            .map_err(|e| format!("posted success, but the Autoland dispatch failed and is retried next run: {e}"))?;
        let marker = self.dispatched(repo, tip);
        let recorded = marker.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::write(&marker, b""));
        recorded.map_err(|e| format!("dispatched Autoland but cannot record it at {}: {e}", marker.display()))
    }

    fn attest(&self, repo: &str, tip: &BranchTip) -> Result<Outcome, String> {
        match self.forge.status_state(repo, &tip.sha, CONTEXT)?.as_deref() {
            Some("success") if !self.dispatched(repo, tip).is_file() => {
                self.dispatch(repo, tip)?;
                return Ok(Outcome::Redispatched);
            }
            Some(_) => return Ok(Outcome::Skipped),
            None => {}
        }
        let findings = self.scan(repo, tip).map_err(|e| format!("nothing posted: {e}"))?;
        if !findings.is_empty() {
            self.forge.post_status(repo, &tip.sha, CONTEXT, "failure", REFUSED)?;
            return Ok(Outcome::Refused(findings));
        }
        self.forge.post_status(repo, &tip.sha, CONTEXT, "success", CLEAN)?;
        self.dispatch(repo, tip)?;
        Ok(Outcome::Clean)
    }

    fn attest_repo(&self, repo: &str, branch: Option<&str>) -> u8 {
        let tips = match self.tips(repo, branch) {
            Ok(tips) => tips,
            Err(error) => {
                eprintln!("denylist: {repo}: cannot list branches: {error}");
                return 2;
            }
        };
        tips.iter().map(|tip| report(repo, tip, self.attest(repo, tip))).max().unwrap_or(0)
    }
}

fn report(repo: &str, tip: &BranchTip, outcome: Result<Outcome, String>) -> u8 {
    let place = format!("{repo} {} {}", tip.branch, &tip.sha[..tip.sha.len().min(12)]);
    match outcome {
        Ok(Outcome::Skipped) => println!("denylist: {place}: already attested; skipped"),
        Ok(Outcome::Redispatched) => println!("denylist: {place}: attested clean earlier; dispatched Autoland"),
        Ok(Outcome::Clean) => println!("denylist: {place}: clean; posted success and dispatched Autoland"),
        Ok(Outcome::Refused(findings)) => {
            findings.iter().for_each(|finding| println!("denylist: {place}: {finding}"));
            println!("denylist: {place}: refused; posted failure");
            return 1;
        }
        Err(error) => {
            eprintln!("denylist: {place}: cannot attest: {error}");
            return 2;
        }
    }
    0
}

fn attest_all<F: CommitStatuses>(forge: &F, config: &Path, cache: &Path, request: &Request) -> u8 {
    let attester = Attester { forge, config, cache };
    request.repos.iter().map(|repo| attester.attest_repo(repo, request.branch.as_deref())).max().unwrap_or(0)
}

fn cache_root() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))?;
    Some(base.join("north").join("denylist"))
}

pub fn run(args: &[String]) -> u8 {
    let Some(request) = parse(args) else {
        eprintln!("{USAGE}");
        return 2;
    };
    let Some(cache) = cache_root() else {
        eprintln!("denylist: cannot attest: neither XDG_CACHE_HOME nor HOME is set");
        return 2;
    };
    match denylist::south_denylist(&request.south) {
        Ok(config) => attest_all(&Gh, config.path(), &cache, &request),
        Err(error) => {
            eprintln!("denylist: cannot attest: {error}");
            2
        }
    }
}

#[cfg(test)]
#[path = "denylist_attest_tests.rs"]
mod tests;

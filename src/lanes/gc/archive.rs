use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use super::Gc;
use super::evidence::refs_on;
use super::exec::{command, git, git_slow, git_with, real, verified, LIMIT};
use super::super::live_process::{any_inside, open_paths};
use super::state::{mirror_path, restore_command};
use crate::foundation::clock::Timestamp;

const ZERO: &str = "0000000000000000000000000000000000000000";
const NOTED_REPOS: [&str; 4] = ["life", "accounts", "travel", "ops"];

pub(super) struct Lane<'a> {
    pub(super) path: Option<&'a Path>,
    pub(super) branch: &'a str,
    pub(super) tip: &'a str,
    pub(super) dirty: bool,
    pub(super) label: &'a str,
}

pub(super) struct Retired {
    pub(super) retired: bool,
    pub(super) tag: String,
    pub(super) why: String,
}

fn tree_of(main: &Path, commit: &str) -> String {
    git(main, &["rev-parse", &format!("{commit}^{{tree}}")]).out
}

pub(super) fn snapshot(path: &Path, main: &Path, tip: &str, label: &str) -> String {
    let Ok(dir) = tempfile::Builder::new().prefix("lane-gc-").tempdir() else {
        return String::new();
    };
    let index = dir.path().join("index");
    let with_index = |args: &[&str]| git_with(path, args, LIMIT, &[("GIT_INDEX_FILE", &index)]);
    if !with_index(&["read-tree", tip]).ok || !with_index(&["add", "-A"]).ok {
        return String::new();
    }
    let tree = with_index(&["write-tree"]);
    if !tree.ok || git(main, &["ls-tree", "-r", &tree.out]).out.lines().any(|line| line.starts_with("160000 ")) {
        return String::new();
    }
    if tree.out == tree_of(main, tip) {
        return tip.to_string();
    }
    let message = format!("Archive {label} working state");
    let commit = git(main, &["-c", "user.name=lane-gc", "-c", "user.email=lane-gc@localhost", "commit-tree", &tree.out, "-p", tip, "-m", &message]);
    if commit.ok { commit.out } else { String::new() }
}

fn delete_origin(main: &Path, branch: &str, target: &str) -> String {
    if branch.is_empty() || !git(main, &["remote", "get-url", "origin"]).ok {
        return String::new();
    }
    let listed = git(main, &["ls-remote", "origin", &format!("refs/heads/{branch}")]);
    if !listed.ok {
        return "origin branch unverified; kept".into();
    }
    let Some(remote_tip) = listed.out.split_whitespace().next() else {
        return String::new();
    };
    if !git(main, &["merge-base", "--is-ancestor", remote_tip, target]).ok {
        return "origin branch holds other work; kept".into();
    }
    let lease = format!("--force-with-lease=refs/heads/{branch}:{remote_tip}");
    let deletion = git_slow(main, &["push", "-q", &lease, "origin", &format!(":refs/heads/{branch}")]);
    if deletion.ok { String::new() } else { "origin branch delete failed".into() }
}

fn push_failure(err: &str) -> String {
    let last = err.lines().rfind(|line| !line.is_empty() && !line.starts_with("error: failed to push some refs"));
    last.map_or_else(|| "archive tag push unverified".into(), |line| format!("archive tag push unverified: {line}"))
}

impl Gc {
    pub(super) fn notice(&mut self, repo: &str, branch: &str, action: &str, tag: &str) -> Result<(), String> {
        let owner = self.owner(repo)?;
        let mut line = format!("{} {repo} {branch} {action} owner={owner}", Timestamp::from_unix_millis(self.now_ms).iso_millis());
        if !tag.is_empty() {
            line.push_str(&format!(" tag={tag} restore={}", restore_command(&self.home, repo, branch, tag)));
        }
        self.stale.notices.push(line.clone());
        let path = self.stale_path.with_file_name("notices.md");
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        OpenOptions::new().create(true).append(true).open(&path).and_then(|mut file| writeln!(file, "{line}")).map_err(|error| error.to_string())
    }

    fn archive(&mut self, main: &Path, container: &str, lane: &Lane, target: &str, shepherd: &str) -> Result<Result<String, String>, String> {
        let shepherd_ref = format!("refs/archive/lanes/{shepherd}");
        if let Some(held) = (!shepherd.is_empty()).then(|| verified(main, &format!("{shepherd_ref}^{{commit}}"))).flatten()
            && tree_of(main, &held) == tree_of(main, target)
            && git(main, &["merge-base", "--is-ancestor", lane.tip, &held]).ok
        {
            return Ok(Ok(shepherd_ref));
        }
        let tag = format!("archive/{container}/{}/{}", lane.label, &target[..target.len().min(12)]);
        let reference = format!("refs/tags/{tag}");
        if !git(main, &["check-ref-format", &reference]).ok {
            return Ok(Err("invalid archive tag name".into()));
        }
        let peeled = format!("{reference}^{{commit}}");
        match verified(main, &peeled) {
            Some(existing) if existing != target => return Ok(Err("archive tag conflict".into())),
            None if !git(main, &["update-ref", &reference, target, ZERO]).ok => return Ok(Err("archive tag write failed".into())),
            _ => {}
        }
        if verified(main, &peeled).as_deref() != Some(target) {
            return Ok(Err("archive tag unverified".into()));
        }
        if git(main, &["remote", "get-url", "origin"]).ok {
            let pushed = git_slow(main, &["push", "-q", "origin", &format!("{reference}:{reference}")]);
            let peeled_name = format!("{reference}^{{}}");
            let seen = git(main, &["ls-remote", "origin", &reference, &peeled_name]);
            let refs: HashMap<&str, &str> = seen.out.lines().filter_map(|line| {
                let mut fields = line.split_whitespace();
                let oid = fields.next()?;
                Some((fields.next().unwrap_or(""), oid))
            }).collect();
            if !pushed.ok || !seen.ok || refs.get(peeled_name.as_str()).or_else(|| refs.get(reference.as_str())).copied() != Some(target) {
                let why = push_failure(&pushed.err);
                self.notice(container, lane.label, &format!("kept ({why})"), "")?;
                return Ok(Err(why));
            }
        }
        Ok(Ok(tag))
    }

    pub(super) fn archive_to_mirror(&self, main: &Path, container: &str, branch: &str, tip: &str) -> Result<String, String> {
        let mirror = mirror_path(&self.home, container);
        let parent = mirror.parent().unwrap_or(Path::new("/"));
        if !mirror.exists() {
            let _ = fs::create_dir_all(parent);
            if !git(parent, &["init", "-q", "--bare", &mirror.to_string_lossy()]).ok {
                return Err("archive mirror init failed".into());
            }
        }
        let day = Timestamp::from_unix_millis(self.now_ms).iso_date();
        let mut reference = format!("refs/archive/{day}/{branch}");
        if verified(&mirror, &format!("{reference}^{{commit}}")).is_some_and(|held| held != tip) {
            reference = format!("{reference}-{}", &tip[..tip.len().min(12)]);
        }
        if !git(&mirror, &["check-ref-format", &reference]).ok {
            return Err("invalid archive ref name".into());
        }
        let copied = git_slow(main, &["push", "-q", &mirror.to_string_lossy(), &format!("{tip}:{reference}")]).ok || {
            let url = git(main, &["config", "--get", "remote.origin.url"]).out;
            git_slow(&mirror, &["fetch", "-q", &url, &format!("+refs/heads/{branch}:{reference}")]).ok
        };
        if !copied {
            return Err("archive mirror copy failed".into());
        }
        if verified(&mirror, &format!("{reference}^{{commit}}")).as_deref() != Some(tip) {
            return Err("archive mirror ref unverified".into());
        }
        Ok(reference)
    }

    pub(super) fn retire_lane(&mut self, main: &Path, name: &str, lane: &Lane) -> Result<Retired, String> {
        let target = if lane.dirty { snapshot(lane.path.unwrap_or(main), main, lane.tip, &format!("{name}/{}", lane.label)) } else { lane.tip.to_string() };
        if target.is_empty() {
            return Ok(Retired { retired: false, tag: String::new(), why: "working state not archivable".into() });
        }
        let shepherd = lane.path.map_or_else(|| lane.branch.to_string(), |path| path.file_name().unwrap_or_default().to_string_lossy().into_owned());
        let tag = match self.archive(main, name, lane, &target, &shepherd)? {
            Ok(tag) => tag,
            Err(why) => return Ok(Retired { retired: false, tag: String::new(), why }),
        };
        let kept = |why: &str| Ok(Retired { retired: false, tag: tag.clone(), why: why.into() });
        if let Some(path) = lane.path {
            let status = git(path, &["status", "--porcelain", "--untracked-files=normal"]);
            if !status.ok || git(path, &["rev-parse", "HEAD"]).out != lane.tip || any_inside(&open_paths(Path::new("/proc")), &real(path)) {
                return kept("lane changed or became active after archive");
            }
            let changed = if lane.dirty { tree_of(main, &snapshot(path, main, lane.tip, name)) != tree_of(main, &target) } else { !status.out.is_empty() };
            if changed {
                return kept("working state changed after archive");
            }
            let path_text = path.to_string_lossy();
            let removal: Vec<&str> = ["worktree", "remove"].into_iter().chain(lane.dirty.then_some("--force")).chain([path_text.as_ref()]).collect();
            let removed = git(main, &removal);
            if !removed.ok {
                return kept(&format!("worktree remove failed: {}", removed.last_err_line()));
            }
        }
        if !lane.branch.is_empty() && !git(main, &["update-ref", "-d", &format!("refs/heads/{}", lane.branch), lane.tip]).ok {
            return Ok(Retired { retired: true, tag, why: "branch delete failed".into() });
        }
        let why = delete_origin(main, lane.branch, &target);
        Ok(Retired { retired: true, tag, why })
    }

    pub(super) fn archive_notes(&self, main: &Path, name: &str, base: &str, branch: &str, tip: &str, tag: &str) -> Result<(), String> {
        let text = format!("Archived {name}:{branch} as {tag}. Restore: {}", restore_command(&self.home, name, branch, tag));
        for item in refs_on(main, base, tip)? {
            let (repo, number) = item.split_once('#').unwrap_or((&item, ""));
            if NOTED_REPOS.contains(&repo) {
                if !command(&["threads", "note", &item, &text]).ok {
                    println!("{name}: threads note unavailable for {item}; restore command retained in notices");
                }
                continue;
            }
            let endpoint = format!("repos/tompassarelli/{repo}/issues/{number}/comments");
            if !command(&["gh", "api", &endpoint, "-f", &format!("body={text}")]).ok {
                println!("{name}: archive comment failed for {item}");
            }
        }
        Ok(())
    }
}

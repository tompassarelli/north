use std::path::Path;

use serde_json::Value;

use super::Gc;
use super::evidence::{landed, origin_repo, protected_branch};
use super::exec::{git, git_slow, in_dir, verified};
use super::liveness::tree_is_live;
use super::local::activity::reflog_ms;
use super::state::RemoteStale;
use super::supersession::{archived_as, superseding};

const BASE: &str = "refs/remotes/origin/main";
const LANDING: &str = "claude/land-";
const HOUR_MS: i64 = 3_600_000;
const LANDING_HOLD_H: f64 = 4.0;

struct Tip {
    name: String,
    oid: String,
    committed_ms: i64,
}

enum Changes {
    OnMain,
    Superseded(Vec<String>),
    Archived(String),
    Unlanded,
}

struct Branch<'a> {
    name: &'a str,
    tip: &'a str,
    landing: bool,
    age_h: f64,
    on_main: bool,
    key: String,
}

#[derive(Default)]
struct Split {
    skipped: usize,
    deletable: usize,
    kept: usize,
}

fn origin_tips(main: &Path) -> Result<Vec<Tip>, String> {
    let listed = git(main, &["for-each-ref", "--format=%(refname:lstrip=3) %(objectname) %(committerdate:unix)", "refs/remotes/origin"]);
    if !listed.ok {
        return Err(format!("origin branches unavailable: {}", listed.last_err_line()));
    }
    Ok(listed
        .out
        .lines()
        .filter_map(|line| {
            let mut fields = line.split(' ');
            let (name, oid, at) = (fields.next()?, fields.next()?, fields.next()?);
            (name != "HEAD").then(|| Tip { name: name.to_owned(), oid: oid.to_owned(), committed_ms: at.parse::<i64>().unwrap_or(0) * 1000 })
        })
        .collect())
}

fn source_of(branch: &str) -> String {
    let source = &branch[LANDING.len()..];
    let hashed = source.len() > 13 && source.as_bytes()[source.len() - 13] == b'-' && source[source.len() - 12..].bytes().all(|b| matches!(b, b'a'..=b'f' | b'0'..=b'9'));
    if hashed { source[..source.len() - 13].to_string() } else { source.to_string() }
}

fn slug(text: &str) -> String {
    text.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '-' }).collect()
}

fn live_ticket(home: &Path, branch: &str) -> Option<String> {
    let queues = std::fs::read_dir(home.join(".local/state/north/land-queue")).ok()?;
    let names = |ticket: &Value| ["branch", "published"].iter().any(|field| ticket.get(field).and_then(Value::as_str) == Some(branch));
    queues.flatten().flat_map(|queue| [queue.path(), queue.path().join("outbox")]).filter_map(|dir| std::fs::read_dir(dir).ok()).flatten().flatten().map(|entry| entry.path()).find(|path| {
        path.extension().is_some_and(|extension| extension == "json") && std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok()).is_some_and(|ticket| names(&ticket))
    }).map(|path| path.file_name().unwrap_or_default().to_string_lossy().into_owned())
}

impl Gc {
    pub(super) fn remote_sweep(&mut self, container: &Path) -> Result<(), String> {
        let name = container.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let main = container.join("main");
        if origin_repo(&main).is_none() {
            println!("{name}: origin is not tompassarelli GitHub; remote read-only");
            return Ok(());
        }
        if !git_slow(&main, &["fetch", "-q", "--prune", "origin"]).ok {
            return Err("origin fetch failed; remote read-only".into());
        }
        if verified(&main, BASE).is_none() {
            return Err("origin/main unavailable".into());
        }
        let tips = origin_tips(&main)?;
        let report = self.stale.repo(&name);
        report.remote_landed.clear();
        report.remote_stale.clear();
        let mut split = Split::default();
        for tip in &tips {
            self.remote_branch(&name, &main, tip, &mut split)?;
        }
        self.say(format!("{name}: remote split: {} LIVE, {} deleted, {} unlanded kept", split.skipped, split.deletable, split.kept));
        Ok(())
    }

    pub(super) fn say(&mut self, line: String) {
        println!("{line}");
        self.remote_rows.push(line);
    }

    fn branch_facts<'a>(&self, name: &str, main: &Path, tip: &'a Tip) -> Branch<'a> {
        let landing = tip.name.starts_with(LANDING);
        let common = std::path::PathBuf::from(git(main, &["rev-parse", "--path-format=absolute", "--git-common-dir"]).out);
        let seen_ms = if landing { tip.committed_ms.max(reflog_ms(&common.join("logs/refs/remotes/origin").join(&tip.name)) as i64) } else { tip.committed_ms };
        Branch {
            name: &tip.name,
            tip: &tip.oid,
            landing,
            age_h: (self.now_ms - seen_ms) as f64 / HOUR_MS as f64,
            on_main: landed(main, BASE, &tip.oid).landed,
            key: format!("{name}:{}", tip.name),
        }
    }

    fn live_reason(&self, name: &str, branch: &Branch, changes: &Changes) -> String {
        if let Some(ticket) = live_ticket(&self.home, branch.name) {
            return format!("c live land ticket {ticket}");
        }
        if protected_branch(branch.name) {
            return "protected branch".into();
        }
        if matches!(changes, Changes::OnMain | Changes::Superseded(_)) {
            return String::new();
        }
        if branch.landing && branch.age_h < LANDING_HOLD_H {
            return "a landing ref pushed under 4h".into();
        }
        self.holder_reason(name, branch)
    }

    fn holder_reason(&self, name: &str, branch: &Branch) -> String {
        let source = if branch.landing { source_of(branch.name) } else { String::new() };
        let prefix = format!("{name}:");
        let (laptop, nexus) = (&self.hosts.laptop, &self.hosts.nexus);
        let author = (!source.is_empty())
            .then(|| laptop.branches.iter().chain(&nexus.branches).find(|(key, _)| key.strip_prefix(&prefix).is_some_and(|rest| slug(rest) == source)))
            .flatten();
        if let Some((_, why)) = author {
            format!("c live source author {why}")
        } else if let Some(why) = tree_is_live(laptop, &branch.key) {
            format!("c laptop {why}")
        } else if let Some(why) = tree_is_live(nexus, &branch.key) {
            format!("c nexus {why}")
        } else if !laptop.ok || !nexus.ok {
            "c worktree liveness unavailable".into()
        } else {
            String::new()
        }
    }

    fn remote_branch(&mut self, name: &str, main: &Path, tip: &Tip, split: &mut Split) -> Result<(), String> {
        let branch = self.branch_facts(name, main, tip);
        let changes = if branch.on_main {
            Changes::OnMain
        } else if let Some(commits) = superseding(main, BASE, branch.tip)? {
            Changes::Superseded(commits)
        } else {
            archived_as(main, branch.tip).map_or(Changes::Unlanded, Changes::Archived)
        };
        let reason = self.live_reason(name, &branch, &changes);
        if !reason.is_empty() {
            split.skipped += 1;
            self.say(format!("{name}: origin {}: LIVE({reason})", branch.name));
            return Ok(());
        }
        let age = branch.age_h.floor() as i64;
        match changes {
            Changes::OnMain => {
                self.stale.repo(name).remote_landed.push(branch.name.into());
                let by = verified(main, BASE).ok_or("origin/main unavailable")?;
                self.delete_remote(name, main, &branch, "landed", &by, split)
            }
            Changes::Superseded(commits) => self.delete_remote(name, main, &branch, &format!("superseded by {}", commits.join(" ")), &commits[0], split),
            Changes::Archived(archive) => self.delete_remote(name, main, &branch, &format!("archived locally as {archive}"), &archive, split),
            Changes::Unlanded => self.keep_unlanded(name, &branch, age, split),
        }
    }

    fn keep_unlanded(&mut self, name: &str, branch: &Branch, age: i64, split: &mut Split) -> Result<(), String> {
        let owner = self.owner(name);
        self.stale.repo(name).remote_stale.push(RemoteStale { branch: branch.name.into(), age_h: age, owner });
        split.kept += 1;
        self.say(format!("{name}: origin {}: KEEP unlanded ({age}h)", branch.name));
        Ok(())
    }

    fn delete_remote(&mut self, name: &str, main: &Path, branch: &Branch, class: &str, by: &str, split: &mut Split) -> Result<(), String> {
        split.deletable += 1;
        let age = branch.age_h.floor() as i64;
        self.say(format!("{name}: origin {}: STALE {class} ({age}h; delete)", branch.name));
        if self.options.dry_run {
            return Ok(());
        }
        let deletion = in_dir(main, &["north", "safe-push", "abandon", "--superseded", branch.name, "--by", by]);
        if !deletion.ok {
            self.say(format!("{name}: keep origin {}: delete failed: {}", branch.name, deletion.last_err_line()));
            return Ok(());
        }
        self.counts.deleted_remote += 1;
        let report = self.stale.repo(name);
        report.remote_landed.retain(|kept| kept != branch.name);
        self.notice(name, branch.name, &format!("deleted ({class})"))
    }
}

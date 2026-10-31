use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde_json::Value;

use super::Gc;
use super::evidence::{LaneClass, LaneFacts, lane_class, landed, named_issues, origin_repo, protected_branch, refs_on};
use super::exec::{api, command, git, git_slow, truthy, verified};
use super::liveness::{open_pull_heads, pending_autoland, tree_is_live};
use super::state::{LandingPing, RemoteStale};
use super::super::verdict::{Facts, Verdict, retirement_verdict};
use crate::foundation::clock::Timestamp;

const BASE: &str = "refs/remotes/origin/main";
const LANDING: &str = "claude/land-";
const HOUR_MS: i64 = 3_600_000;
const RUN_STATUSES: [&str; 5] = ["queued", "in_progress", "waiting", "pending", "requested"];

struct Evidence {
    branches: Vec<Value>,
    prs: HashSet<String>,
    active: HashSet<String>,
    active_tips: HashSet<String>,
    pushes: Option<HashMap<String, i64>>,
}

struct Branch<'a> {
    name: &'a str,
    tip: &'a str,
    target: &'a Value,
    landing: bool,
    pushed_at: i64,
    age_h: f64,
    ancestor: bool,
    key: String,
}

#[derive(Default)]
struct Split {
    skipped: usize,
    deletable: usize,
    archivable: usize,
}

fn branch_query(repo: &str) -> String {
    format!(
        "query($endCursor:String){{repository(owner:\"tompassarelli\",name:{}){{refs(refPrefix:\"refs/heads/\",first:100,after:$endCursor){{nodes{{name target{{... on Commit{{oid committedDate status{{contexts{{context state}}}}}}}}}} pageInfo{{hasNextPage endCursor}}}}}}}}",
        serde_json::to_string(repo).unwrap_or_default()
    )
}

fn remote_branches(repo: &str) -> Result<Vec<Value>, String> {
    let listed = command(&["gh", "api", "graphql", "--paginate", "--slurp", "-f", &format!("query={}", branch_query(repo))]);
    if !listed.ok {
        return Err("remote status evidence unavailable".into());
    }
    let pages: Value = serde_json::from_str(&listed.out).map_err(|error| error.to_string())?;
    let refs_of = |page: &Value| page.get("data").and_then(|d| d.get("repository")).and_then(|r| r.get("refs")).cloned();
    let pages = pages.as_array().filter(|pages| !pages.is_empty()).ok_or("remote status evidence incomplete")?;
    let last = pages.len() - 1;
    let complete = pages.iter().enumerate().all(|(i, page)| {
        let refs = refs_of(page);
        !truthy(page.get("errors"))
            && refs.as_ref().and_then(|r| r.get("nodes")).is_some_and(Value::is_array)
            && refs.as_ref().and_then(|r| r.get("pageInfo")).and_then(|info| info.get("hasNextPage")).and_then(Value::as_bool) == Some(i < last)
    });
    if !complete {
        return Err("remote status evidence incomplete".into());
    }
    Ok(pages.iter().filter_map(|page| refs_of(page)?.get("nodes")?.as_array().cloned()).flatten().collect())
}

fn run_pages_complete(pages: &[Value]) -> bool {
    let total = |page: &Value| page.get("total_count").and_then(Value::as_i64).filter(|count| (0..=9_007_199_254_740_991).contains(count));
    let runs = |page: &Value| page.get("workflow_runs").and_then(Value::as_array).map(Vec::len);
    let Some(first) = pages.first().and_then(total) else {
        return false;
    };
    pages.iter().all(|page| runs(page).is_some() && total(page) == Some(first)) && pages.iter().filter_map(runs).sum::<usize>() as i64 == first
}

fn active_runs(repo: &str) -> Result<(HashSet<String>, HashSet<String>), String> {
    let (mut branches, mut tips) = (HashSet::new(), HashSet::new());
    for status in RUN_STATUSES {
        let listed = api(&format!("repos/tompassarelli/{repo}/actions/runs?status={status}&per_page=100"))?;
        let pages = listed.as_array().filter(|pages| run_pages_complete(pages)).ok_or("Actions eligibility incomplete")?;
        for run in pages.iter().filter_map(|page| page.get("workflow_runs")?.as_array()).flatten() {
            let (Some(branch), Some(tip)) = (run.get("head_branch").and_then(Value::as_str), run.get("head_sha").and_then(Value::as_str)) else {
                return Err("Actions eligibility incomplete".into());
            };
            branches.insert(branch.to_string());
            tips.insert(tip.to_string());
        }
    }
    Ok((branches, tips))
}

fn source_of(branch: &str) -> String {
    let source = &branch[LANDING.len()..];
    let hashed = source.len() > 13 && source.as_bytes()[source.len() - 13] == b'-' && source[source.len() - 12..].bytes().all(|b| matches!(b, b'a'..=b'f' | b'0'..=b'9'));
    if hashed { source[..source.len() - 13].to_string() } else { source.to_string() }
}

fn slug(text: &str) -> String {
    text.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '-' }).collect()
}

fn github_hold(evidence: &Evidence, branch: &Branch) -> Option<&'static str> {
    if protected_branch(branch.name) {
        Some("protected branch")
    } else if evidence.pushes.is_none() {
        Some("a push activity unavailable")
    } else if evidence.prs.contains(branch.name) {
        Some("b open PR")
    } else if evidence.active.contains(branch.name) || evidence.active_tips.contains(branch.tip) {
        Some("b active Actions")
    } else if pending_autoland(branch.target.get("status")) && (!branch.landing || branch.age_h < 4.0) {
        Some("b pending autoland")
    } else {
        None
    }
}

fn age_hold(evidence: &Evidence, branch: &Branch) -> Option<&'static str> {
    let pushed = evidence.pushes.as_ref().is_some_and(|pushes| pushes.contains_key(&format!("refs/heads/{}", branch.name)));
    if branch.landing {
        (branch.age_h < 4.0).then_some("a landing ref pushed under 4h")
    } else if branch.ancestor {
        Some("")
    } else if branch.age_h < 24.0 {
        Some("a fresh commit")
    } else {
        pushed.then_some("a branch pushed under 24h")
    }
}

impl Gc {
    pub(super) fn remote_sweep(&mut self, container: &Path, remote: bool) -> Result<(), String> {
        let name = container.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let main = container.join("main");
        let Some(repo) = origin_repo(&main) else {
            println!("{name}: origin is not tompassarelli GitHub; remote read-only");
            return Ok(());
        };
        if !git_slow(&main, &["fetch", "-q", "--prune", "origin"]).ok {
            return Err("origin fetch failed; remote read-only".into());
        }
        if verified(&main, BASE).is_none() {
            return Err("origin/main unavailable".into());
        }
        let branches = remote_branches(&repo)?;
        let prs = open_pull_heads(&repo)?;
        let (active, active_tips) = active_runs(&repo)?;
        let evidence = Evidence { branches, prs, active, active_tips, pushes: self.pushed_refs(&repo) };
        let report = self.stale.repo(&name);
        report.remote_landed.clear();
        report.remote_stale.clear();
        let mut split = Split::default();
        for target in &evidence.branches {
            self.remote_branch(&name, &main, &evidence, target, remote, &mut split)?;
        }
        println!(
            "{name}: remote split: {} LIVE, {} STALE ({} landed, {} archive-and-delete)",
            split.skipped,
            split.deletable + split.archivable,
            split.deletable,
            split.archivable
        );
        Ok(())
    }

    fn branch_facts<'a>(&self, name: &str, main: &Path, evidence: &Evidence, node: &'a Value) -> Result<Branch<'a>, String> {
        let branch = node.get("name").and_then(Value::as_str).unwrap_or("undefined");
        let target = node.get("target").unwrap_or(&Value::Null);
        let tip = target.get("oid").and_then(Value::as_str).filter(|oid| !oid.is_empty());
        let committed = target.get("committedDate").and_then(Value::as_str).filter(|at| !at.is_empty());
        let (Some(tip), Some(committed)) = (tip, committed) else {
            return Err("branch tip evidence incomplete".into());
        };
        let landing = branch.starts_with(LANDING);
        let pushed_at = evidence.pushes.as_ref().and_then(|pushes| pushes.get(&format!("refs/heads/{branch}"))).copied().unwrap_or(0);
        let since = if landing {
            Some(if pushed_at != 0 { pushed_at } else { self.now_ms - 24 * HOUR_MS })
        } else {
            Timestamp::parse_rfc3339(committed).map(Timestamp::unix_millis)
        };
        let since = since.ok_or("commit date unavailable")?;
        Ok(Branch {
            name: branch,
            tip,
            target,
            landing,
            pushed_at,
            age_h: (self.now_ms - since) as f64 / HOUR_MS as f64,
            ancestor: git(main, &["merge-base", "--is-ancestor", tip, BASE]).ok,
            key: format!("{name}:{branch}"),
        })
    }

    fn live_reason(&mut self, name: &str, main: &Path, evidence: &Evidence, branch: &Branch) -> String {
        match github_hold(evidence, branch).or_else(|| age_hold(evidence, branch)) {
            Some(reason) => reason.to_string(),
            None => self.holder_reason(name, main, branch).unwrap_or_else(|_| "c Fleet holder liveness unavailable".into()),
        }
    }

    fn holder_reason(&mut self, name: &str, main: &Path, branch: &Branch) -> Result<String, String> {
        let claims = self.live_claims()?;
        let source = if branch.landing { source_of(branch.name) } else { String::new() };
        let prefix = format!("{name}:");
        let (laptop, nexus) = (&self.hosts.laptop, &self.hosts.nexus);
        let author = (!source.is_empty())
            .then(|| laptop.branches.iter().chain(&nexus.branches).find(|(key, _)| key.strip_prefix(&prefix).is_some_and(|rest| slug(rest) == source)))
            .flatten();
        Ok(if let Some((_, why)) = author {
            format!("c live source author {why}")
        } else if let Some(why) = tree_is_live(laptop, &branch.key) {
            format!("c laptop {why}")
        } else if let Some(why) = tree_is_live(nexus, &branch.key) {
            format!("c nexus {why}")
        } else if !laptop.ok || !nexus.ok {
            "c worktree liveness unavailable".into()
        } else if refs_on(main, BASE, branch.tip)?.iter().any(|item| claims.contains(item)) {
            "d live holder session (claim clock ignored)".into()
        } else {
            String::new()
        })
    }

    fn remote_branch(&mut self, name: &str, main: &Path, evidence: &Evidence, node: &Value, remote: bool, split: &mut Split) -> Result<(), String> {
        let branch = self.branch_facts(name, main, evidence, node)?;
        let reason = self.live_reason(name, main, evidence, &branch);
        let mut facts = Facts {
            age: (branch.age_h * HOUR_MS as f64).floor() as i64,
            minimum_age: if branch.landing { 4 * HOUR_MS } else if branch.ancestor { i64::MIN } else { 24 * HOUR_MS },
            merged: branch.ancestor,
            equivalent: false,
            evil_merge: false,
            archive_age: Some(if branch.landing { 4 } else { 24 } * HOUR_MS),
            protections: u32::from(!reason.is_empty()),
        };
        if retirement_verdict(&facts) == Verdict::Keep {
            if branch.landing && !self.options.dry_run {
                self.stale.forget_ping(&branch.key);
            }
            split.skipped += 1;
            println!("{name}: origin {}: LIVE({reason})", branch.name);
            return Ok(());
        }
        if branch.landing && !self.landing_grace_over(name, &branch)? {
            split.skipped += 1;
            return Ok(());
        }
        facts.equivalent = !branch.ancestor && landed(main, BASE, branch.tip).landed;
        let is_landed = !branch.landing && matches!(retirement_verdict(&facts), Verdict::Retire | Verdict::RetireEquivalent);
        self.retire_remote(name, main, &branch, is_landed, remote, split)
    }

    fn landing_grace_over(&mut self, name: &str, branch: &Branch) -> Result<bool, String> {
        let parent = self.owner(name)?;
        let ping = self.stale.landing_pings.get_or_insert_default().get(&branch.key).cloned();
        let current = ping.as_ref().is_some_and(|ping| ping.tip == branch.tip && ping.pushed_at == branch.pushed_at && ping.owner == parent);
        let Some(ping) = ping.filter(|_| current) else {
            if self.options.dry_run {
                println!("{name}: origin {}: would notify {parent}; keep for 1h grace", branch.name);
                return Ok(false);
            }
            let text = format!(
                "{parent}: orphan landing {}:{} idle {}h; lane-gc will archive and retire after 1h unless live Fleet ownership resumes",
                name,
                branch.name,
                branch.age_h.floor() as i64
            );
            if !command(&["north", "fleet", "send", &parent, &text]).ok {
                println!("{name}: origin {}: keep (Fleet parent notification failed)", branch.name);
                return Ok(false);
            }
            let pinged_at = Timestamp::from_unix_millis(self.now_ms).iso_millis();
            let ping = LandingPing { tip: branch.tip.into(), pushed_at: branch.pushed_at, pinged_at, owner: parent.clone() };
            self.stale.landing_pings.get_or_insert_default().insert(branch.key.clone(), ping);
            self.notice(name, branch.name, "Fleet parent notified; 1h grace", "")?;
            println!("{name}: origin {}: notified {parent}; keep for 1h grace", branch.name);
            return Ok(false);
        };
        let pinged = Timestamp::parse_rfc3339(&ping.pinged_at).map(Timestamp::unix_millis);
        if pinged.is_none_or(|at| self.now_ms - at < HOUR_MS) {
            println!("{name}: origin {}: keep (owner ping grace)", branch.name);
            return Ok(false);
        }
        Ok(true)
    }

    fn retire_remote(&mut self, name: &str, main: &Path, branch: &Branch, is_landed: bool, remote: bool, split: &mut Split) -> Result<(), String> {
        let class = if is_landed {
            "landed"
        } else {
            let issues = named_issues(main, name, BASE, branch.tip)?;
            let states = self.issue_states(&issues);
            let class = lane_class(&LaneFacts { dirty: false, equivalent: false, issues: &states, idle_hours: 0.0, held: false });
            if class == LaneClass::Superseded { "superseded" } else { "owned" }
        };
        let age = branch.age_h.floor() as i64;
        if is_landed {
            self.stale.repo(name).remote_landed.push(branch.name.into());
            split.deletable += 1;
        } else {
            let owner = self.owner(name)?;
            self.stale.repo(name).remote_stale.push(RemoteStale { branch: branch.name.into(), age_h: age, owner });
            split.archivable += 1;
        }
        let action = if is_landed { "delete (landed)".into() } else { format!("archive to the {name} mirror under refs/archive/, then delete") };
        println!("{name}: origin {}: STALE {class} ({age}h; {action})", branch.name);
        if self.options.dry_run || (!remote && !branch.landing && !is_landed) {
            return Ok(());
        }
        let tag = if is_landed { String::new() } else {
            match self.archive_to_mirror(main, name, branch.name, branch.tip) {
                Ok(tag) => tag,
                Err(why) => {
                    println!("{name}: keep origin {}: {why}", branch.name);
                    return Ok(());
                }
            }
        };
        let lease = format!("--force-with-lease=refs/heads/{}:{}", branch.name, branch.tip);
        let deletion = git_slow(main, &["push", "-q", &lease, "origin", &format!(":refs/heads/{}", branch.name)]);
        if !deletion.ok {
            println!("{name}: keep origin {}: delete failed: {}", branch.name, deletion.last_err_line());
            return Ok(());
        }
        self.counts.deleted_remote += 1;
        self.stale.forget_ping(&branch.key);
        let report = self.stale.repo(name);
        report.remote_landed.retain(|kept| kept != branch.name);
        report.remote_stale.retain(|row| row.branch != branch.name);
        self.notice(name, branch.name, &if is_landed { "deleted (landed)".into() } else { format!("archived ({class})") }, &tag)?;
        if !is_landed {
            self.archive_notes(main, name, BASE, branch.name, branch.tip, &tag)?;
        }
        Ok(())
    }
}

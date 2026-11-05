use std::collections::{HashMap, HashSet};
use std::path::Path;

use north_github_mirror::read;
use north_github_mirror::store::{self, Mirror};
use serde_json::Value;

use super::Gc;
use super::evidence::{LaneClass, LaneFacts, lane_class, landed, named_issues, origin_repo, own_repo, protected_branch, refs_on};
use super::exec::{api, git_slow, verified};
use super::liveness::{open_pull_heads, pending_autoland, tree_is_live};
use super::state::RemoteStale;
use north_core::foundation::clock::Timestamp;

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

pub(super) fn remote_branches(repo: &str) -> Result<Vec<Value>, String> {
    read::listed(&Mirror::local(), read::now(), repo, store::BRANCHES, || store::fetch_branches(repo))
}

fn run_pages_complete(pages: &[Value]) -> bool {
    let total = |page: &Value| page.get("total_count").and_then(Value::as_i64).filter(|count| (0..=9_007_199_254_740_991).contains(count));
    let runs = |page: &Value| page.get("workflow_runs").and_then(Value::as_array).map(Vec::len);
    let Some(first) = pages.first().and_then(total) else {
        return false;
    };
    pages.iter().all(|page| runs(page).is_some() && total(page) == Some(first)) && pages.iter().filter_map(runs).sum::<usize>() as i64 == first
}

pub(super) fn active_runs(repo: &str) -> Result<(HashSet<String>, HashSet<String>), String> {
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

fn live_ticket(home: &Path, branch: &str) -> Option<String> {
    let queues = std::fs::read_dir(home.join(".local/state/north/land-queue")).ok()?;
    let names = |ticket: &Value| ["branch", "published"].iter().any(|field| ticket.get(field).and_then(Value::as_str) == Some(branch));
    queues.flatten().flat_map(|queue| [queue.path(), queue.path().join("outbox")]).filter_map(|dir| std::fs::read_dir(dir).ok()).flatten().flatten().map(|entry| entry.path()).find(|path| {
        path.extension().is_some_and(|extension| extension == "json") && std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok()).is_some_and(|ticket| names(&ticket))
    }).map(|path| path.file_name().unwrap_or_default().to_string_lossy().into_owned())
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
    if branch.on_main {
        Some("")
    } else if branch.landing {
        (branch.age_h < 4.0).then_some("a landing ref pushed under 4h")
    } else if branch.age_h < 24.0 {
        Some("a fresh commit")
    } else {
        pushed.then_some("a branch pushed under 24h")
    }
}

impl Gc {
    pub(super) fn remote_sweep(&mut self, container: &Path) -> Result<(), String> {
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
            self.remote_branch(&name, &main, &evidence, target, &mut split)?;
        }
        println!(
            "{name}: remote split: {} LIVE, {} deleted, {} unlanded kept",
            split.skipped,
            split.deletable,
            split.kept
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
            age_h: (self.now_ms - since) as f64 / HOUR_MS as f64,
            on_main: landed(main, BASE, tip).landed,
            key: format!("{name}:{branch}"),
        })
    }

    fn live_reason(&mut self, name: &str, main: &Path, evidence: &Evidence, branch: &Branch) -> String {
        if let Some(ticket) = live_ticket(&self.home, branch.name) {
            return format!("c live land ticket {ticket}");
        }
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

    fn remote_branch(&mut self, name: &str, main: &Path, evidence: &Evidence, node: &Value, split: &mut Split) -> Result<(), String> {
        let branch = self.branch_facts(name, main, evidence, node)?;
        let reason = self.live_reason(name, main, evidence, &branch);
        if !reason.is_empty() {
            split.skipped += 1;
            println!("{name}: origin {}: LIVE({reason})", branch.name);
            return Ok(());
        }
        let age = branch.age_h.floor() as i64;
        if branch.on_main {
            self.stale.repo(name).remote_landed.push(branch.name.into());
            return self.delete_remote(name, main, &branch, "landed", age, split);
        }
        let issues = named_issues(main, &own_repo(main, name), BASE, branch.tip)?;
        let states = self.issue_states(&issues);
        if lane_class(&LaneFacts { dirty: false, equivalent: false, issues: &states, idle_hours: 0.0, held: false }) == LaneClass::Superseded {
            return self.delete_remote(name, main, &branch, "superseded", age, split);
        }
        let owner = self.owner(name)?;
        let described = if issues.is_empty() { vec!["fault: names no issue".to_string()] } else { issues.iter().zip(&states).map(|(item, state)| format!("{item}={}", state.label())).collect() };
        self.stale.repo(name).remote_stale.push(RemoteStale { branch: branch.name.into(), age_h: age, issues: described.clone(), owner });
        split.kept += 1;
        println!("{name}: origin {}: KEEP unlanded ({age}h; {})", branch.name, described.join(" "));
        Ok(())
    }

    fn delete_remote(&mut self, name: &str, main: &Path, branch: &Branch, class: &str, age: i64, split: &mut Split) -> Result<(), String> {
        split.deletable += 1;
        println!("{name}: origin {}: STALE {class} ({age}h; delete)", branch.name);
        if self.options.dry_run {
            return Ok(());
        }
        let lease = format!("--force-with-lease=refs/heads/{}:{}", branch.name, branch.tip);
        let deletion = git_slow(main, &["push", "-q", &lease, "origin", &format!(":refs/heads/{}", branch.name)]);
        if !deletion.ok {
            println!("{name}: keep origin {}: delete failed: {}", branch.name, deletion.last_err_line());
            return Ok(());
        }
        self.counts.deleted_remote += 1;
        let report = self.stale.repo(name);
        report.remote_landed.retain(|kept| kept != branch.name);
        self.notice(name, branch.name, &format!("deleted ({class})"))
    }
}

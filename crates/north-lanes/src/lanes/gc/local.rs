use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use super::Gc;
use super::archive::{self, window_hours};
use super::evidence::{IssueState, LaneFacts, Landed, committed, committed_each, lane_class, landed, landed_each, lane_issues, origin_repo, protected_branch};
use super::exec::{command, git, git_slow, real, verified};
use super::super::live_process::any_inside;
use super::super::reap::irreplaceable_ignored;
use super::super::repo::is_lander_tree;
use super::liveness::{holds_git_dir, worktree_admin};
use super::report::Row;
use super::strays::modified_within;
use super::state::{LocalStale, Owned};
use super::super::verdict::{Facts, Verdict, retirement_verdict};
use north_base::foundation::clock::Timestamp;
use north_base::platform::land_queue;

const BASES: [&str; 3] = ["refs/remotes/origin/main", "refs/remotes/origin/HEAD", "refs/heads/main"];
const HOUR_MS: f64 = 3_600_000.0;

struct Container {
    name: String,
    main: PathBuf,
    base: String,
    common: PathBuf,
    lanes: PathBuf,
    cutoff_ms: f64,
    own: String,
    previous: HashMap<String, String>,
    local_stale: Vec<LocalStale>,
    owned: Vec<Owned>,
    checked_out: HashSet<String>,
    landed: HashMap<String, Landed>,
    committed: HashMap<String, i64>,
}

impl Container {
    fn landed(&self, tip: &str) -> Landed {
        self.landed.get(tip).cloned().unwrap_or_else(|| landed(&self.main, &self.base, tip))
    }

    fn committed(&self, tip: &str) -> i64 {
        self.committed.get(tip).copied().unwrap_or_else(|| committed(&self.main, tip))
    }
}

struct ArchiveLane<'a> {
    worktree: Option<&'a Path>,
    branch: &'a str,
    tip: &'a str,
    place: &'a str,
    issues: &'a [String],
}

struct Worktree {
    path: PathBuf,
    place: String,
    branch: String,
    tip: String,
    locked: bool,
    dirty: &'static str,
    landed: Landed,
    activity_ms: f64,
    age_h: f64,
}

pub(super) fn parse_worktrees(porcelain: &str) -> Vec<BTreeMap<String, String>> {
    porcelain
        .split("\n\n")
        .map(|block| block.lines().filter(|line| !line.is_empty()).map(|line| line.split_once(' ').map_or((line.into(), "true".into()), |(k, v)| (k.into(), v.into()))).collect::<BTreeMap<String, String>>())
        .filter(|fields| fields.contains_key("worktree"))
        .collect()
}

fn branch_of(fields: &BTreeMap<String, String>) -> String {
    fields.get("branch").map(|branch| branch.trim_start_matches("refs/heads/").to_string()).unwrap_or_default()
}

fn newest_mtime(paths: &[PathBuf]) -> f64 {
    paths
        .iter()
        .filter_map(|path| std::fs::metadata(path).and_then(|meta| meta.modified()).ok())
        .filter_map(|at| at.duration_since(UNIX_EPOCH).ok())
        .map(|since| since.as_secs_f64() * 1000.0)
        .fold(0.0, f64::max)
}

pub(super) fn reflog_ms(path: &Path) -> f64 {
    let Ok(text) = std::fs::read_to_string(path) else { return 0.0 };
    let stamp = |line: &str| line.split('\t').next()?.split_whitespace().rev().nth(1)?.parse::<u64>().ok();
    text.lines().filter_map(stamp).max().map_or(0.0, |secs| secs as f64 * 1000.0)
}

pub(super) fn lane_activity_ms(path: &Path, admin: &Path, common: &Path, branch: &str, committed_ms: f64) -> f64 {
    let mut stamps = vec![reflog_ms(&admin.join("logs/HEAD")), newest_mtime(&[path.to_path_buf(), admin.join("HEAD"), admin.join("index")])];
    if !branch.is_empty() {
        stamps.push(reflog_ms(&common.join("logs/refs/heads").join(branch)));
    }
    stamps.into_iter().fold(committed_ms, f64::max)
}

fn cited(issues: &[String]) -> String {
    if issues.is_empty() { String::new() } else { format!(" ({})", issues.join(" ")) }
}

impl Gc {
    pub(super) fn sweep(&mut self, container: &Path, live: &[PathBuf]) -> Result<(), String> {
        let name = container.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let main = container.join("main");
        let dry = self.options.dry_run;
        if !dry && !git_slow(&main, &["fetch", "-q", "--prune", "origin"]).ok {
            println!("{name}: origin fetch failed; kept all lanes");
            return Ok(());
        }
        let Some(base) = BASES.iter().find(|base| verified(&main, base).is_some()) else {
            println!("{name}: no main branch; skipped");
            return Ok(());
        };
        let common = real(Path::new(&git(&main, &["rev-parse", "--path-format=absolute", "--git-common-dir"]).out));
        if !dry {
            git(&main, &["worktree", "prune"]);
        }
        self.move_strays(container, &main, &common, live);
        let worktrees = parse_worktrees(&git(&main, &["worktree", "list", "--porcelain"]).out);
        let listed = git(&main, &["for-each-ref", "--format=%(objectname)", "refs/heads"]).out;
        let mut tips: Vec<String> = worktrees.iter().filter_map(|fields| fields.get("HEAD").cloned()).chain(listed.lines().map(str::to_owned)).filter(|tip| !tip.is_empty()).collect();
        tips.sort();
        tips.dedup();
        let landed = landed_each(&main, base, &tips);
        let committed = committed_each(&main, &tips);
        let origin = origin_repo(&main);
        let previous = self.stale.repo(&name).local_stale.iter().map(|row| (row.path.clone(), row.flagged_at.clone())).collect();
        let mut sweep = Container {
            own: origin.unwrap_or_else(|| name.clone()),
            cutoff_ms: self.now_ms as f64 - self.options.idle_hours * HOUR_MS,
            checked_out: worktrees.iter().map(branch_of).filter(|branch| !branch.is_empty()).collect(),
            lanes: real(&container.join("worktrees")),
            name,
            main,
            base: base.to_string(),
            common,
            previous,
            local_stale: Vec::new(),
            owned: Vec::new(),
            landed,
            committed,
        };
        let swept = self.sweep_lanes(&mut sweep, &worktrees, live);
        self.stale.repo(&sweep.name).local_stale = std::mem::take(&mut sweep.local_stale);
        swept?;
        sweep.owned.sort_by_key(|row| std::cmp::Reverse(row.age_h));
        self.stale.repo(&sweep.name).owned = Some(sweep.owned);
        if !dry {
            git(&sweep.main, &["worktree", "prune"]);
            git_slow(&sweep.main, &["remote", "prune", "origin"]);
        }
        Ok(())
    }

    fn sweep_lanes(&mut self, sweep: &mut Container, worktrees: &[BTreeMap<String, String>], live: &[PathBuf]) -> Result<(), String> {
        for fields in worktrees {
            self.sweep_worktree(sweep, fields, live)?;
        }
        let listed = git(&sweep.main, &["for-each-ref", "--format=%(refname:short)\t%(objectname)", "refs/heads"]).out;
        for line in listed.lines().filter(|line| !line.is_empty()) {
            let (branch, tip) = line.split_once('\t').unwrap_or((line, ""));
            self.sweep_branch(sweep, branch, tip)?;
        }
        Ok(())
    }

    fn keep(&mut self, sweep: &Container, path: &Path, why: &str) {
        self.counts.kept += 1;
        if self.options.dry_run {
            println!("{}: keep {}: {why}", sweep.name, self.tilde(path));
        }
    }

    fn modified_since(&self, path: &Path, hours: f64) -> bool {
        modified_within(path, self.now_ms, hours)
    }

    fn idle_for(&self, lane: &Worktree, hours: f64) -> bool {
        lane.age_h >= hours && !self.modified_since(&lane.path, hours)
    }

    fn delete_branch(&self, main: &Path, branch: &str, tip: &str) -> bool {
        self.options.dry_run || git(main, &["update-ref", "-d", &format!("refs/heads/{branch}"), tip]).ok
    }

    fn sweep_worktree(&mut self, sweep: &mut Container, fields: &BTreeMap<String, String>, live: &[PathBuf]) -> Result<(), String> {
        let path = PathBuf::from(&fields["worktree"]);
        if real(&path).parent() != Some(sweep.lanes.as_path()) || is_lander_tree(&path) {
            return Ok(());
        }
        if fields.contains_key("prunable") || !path.exists() {
            self.keep(sweep, &path, "prunable");
            return Ok(());
        }
        if holds_git_dir(&path, &sweep.common) {
            self.keep(sweep, &path, "holds .git");
            return Ok(());
        }
        let branch = branch_of(fields);
        let tip = fields.get("HEAD").cloned().unwrap_or_default();
        let status = git(&path, &["status", "--porcelain", "--untracked-files=normal"]);
        let dirty = if !status.ok { "status failed" } else if status.out.is_empty() { "" } else { "dirty" };
        let admin = PathBuf::from(worktree_admin(&path));
        let activity_ms = lane_activity_ms(&path, &admin, &sweep.common, &branch, sweep.committed(&tip) as f64 * 1000.0);
        let lane = Worktree {
            place: self.tilde(&path),
            landed: sweep.landed(&tip),
            age_h: (self.now_ms as f64 - activity_ms) / HOUR_MS,
            locked: fields.contains_key("locked"),
            path,
            branch,
            tip,
            dirty,
            activity_ms,
        };
        let flagged = (!lane.landed.landed || !dirty.is_empty()) && lane.age_h >= 24.0 && !self.modified_since(&lane.path, 24.0);
        if flagged {
            self.flag(sweep, &lane)?;
        }
        let retired = if lane.landed.landed && dirty.is_empty() { self.retire_worktree(sweep, &lane, live, &lane.landed.how.clone()) } else { self.decide_worktree(sweep, &lane, live)? };
        if retired || lane.locked || !lane.path.join("target").is_dir() {
            return Ok(());
        }
        let basis = if lane.landed.landed {
            "landed".to_string()
        } else if self.idle_for(&lane, self.options.idle_hours) {
            format!("idle {}h", lane.age_h.floor())
        } else {
            return Ok(());
        };
        self.drop_target(&sweep.name, &lane.path, live, &basis);
        Ok(())
    }

    fn flag(&mut self, sweep: &mut Container, lane: &Worktree) -> Result<(), String> {
        let path = lane.path.display().to_string();
        let now = Timestamp::from_unix_millis(self.now_ms).iso_millis();
        sweep.local_stale.push(LocalStale {
            flagged_at: sweep.previous.get(&path).filter(|at| !at.is_empty()).cloned().unwrap_or(now),
            owner: self.owner(&sweep.name)?,
            branch: lane.branch.clone(),
            age_h: lane.age_h.floor() as i64,
            dirty: !lane.dirty.is_empty(),
            path,
        });
        Ok(())
    }

    fn held(&mut self, lane: &Worktree, live: &[PathBuf]) -> String {
        let why = if lane.locked {
            "locked"
        } else if any_inside(live, &real(&lane.path)) {
            "process inside"
        } else if protected_branch(&lane.branch) {
            "protected branch"
        } else if lane.dirty == "status failed" {
            "status failed"
        } else {
            match self.session_in(&lane.path) {
                Ok(true) => "live Fleet session in lane",
                Ok(false) => "",
                Err(_) => "Fleet unavailable",
            }
        };
        why.to_string()
    }

    fn surface(&mut self, sweep: &mut Container, place: &str, branch: &str, age_h: i64, refs: &[String]) -> Result<String, String> {
        let owner = self.owner(&sweep.name)?;
        let issues = if refs.is_empty() { vec!["fault: names no issue".to_string()] } else { refs.iter().zip(self.issue_states(refs)).map(|(item, state)| format!("{item}={}", state.label())).collect() };
        sweep.owned.push(Owned { place: place.into(), branch: branch.into(), age_h, refs: refs.to_vec(), issues, owner: owner.clone() });
        Ok(owner)
    }

    fn decide_worktree(&mut self, sweep: &mut Container, lane: &Worktree, live: &[PathBuf]) -> Result<bool, String> {
        let dirty = !lane.dirty.is_empty();
        let issues = lane_issues(&sweep.main, &sweep.own, &sweep.base, &lane.branch, &lane.tip).unwrap_or_default();
        let states = self.issue_states(&issues);
        let held = self.held(lane, live);
        let idle_hours = if dirty && self.idle_for(lane, 24.0) { lane.age_h } else { 0.0 };
        let class = lane_class(&LaneFacts { dirty, equivalent: lane.landed.landed, issues: &states, idle_hours, held: !held.is_empty() });
        if let Some(window) = window_hours(&states) {
            let why = if held.is_empty() { self.archive_hold(sweep, lane, window, &issues) } else { held };
            if why.is_empty() {
                let found = ArchiveLane { worktree: Some(&lane.path), branch: &lane.branch, tip: &lane.tip, place: &lane.place, issues: &issues };
                if self.archive_lane(sweep, &found) {
                    return Ok(true);
                }
            } else if self.options.dry_run {
                println!("{}: {} {}: keep ({why})", sweep.name, class.label(), lane.place);
            }
        }
        self.unlanded_row(&sweep.name, &sweep.main, &sweep.base, &lane.tip, sweep.committed(&lane.tip), &Row { place: &lane.place, branch: &lane.branch, ahead: lane.landed.ahead, dirty: lane.dirty, class });
        let shown = if lane.branch.is_empty() { "(detached)" } else { &lane.branch };
        let owner = self.surface(sweep, &lane.place, shown, lane.age_h.floor() as i64, &issues)?;
        self.list_open(&issues, &states, &format!("{} ({shown})", lane.place));
        self.counts.kept += 1;
        if self.options.dry_run {
            println!("{}: {} {}: surface to {owner}{}", sweep.name, class.label(), lane.place, cited(&issues));
        }
        Ok(false)
    }

    fn archive_hold(&mut self, sweep: &Container, lane: &Worktree, window: f64, issues: &[String]) -> String {
        if !self.idle_for(lane, window) {
            return format!("touched within {} days", window / 24.0);
        }
        if let Some(ticket) = land_queue::waiting_on(&land_queue::queues(&self.home.join(".local/state")), &land_queue::local_host(), &lane.path).first() {
            return format!("land ticket {} still waits", ticket.display());
        }
        let ignored = irreplaceable_ignored(&lane.path);
        if let Some(first) = ignored.first() {
            return format!("ignored files ({first}, {} total)", ignored.len());
        }
        self.local_archive_guard(&sweep.main, &sweep.name, issues, &lane.branch, Some(&lane.path))
    }

    fn list_open(&mut self, issues: &[String], states: &[IssueState], place: &str) {
        for (issue, state) in issues.iter().zip(states) {
            if *state == IssueState::Open {
                self.kept_open.entry(issue.clone()).or_default().push(place.to_owned());
            }
        }
    }

    fn archive_lane(&mut self, sweep: &mut Container, lane: &ArchiveLane) -> bool {
        let why = if lane.issues.is_empty() { "no issue, no owner".to_owned() } else { format!("issue closed: {}", lane.issues.join(" ")) };
        let directory = self.options.archive.join(&sweep.name);
        if self.options.dry_run {
            println!("{}: archive {} ({why}) to {}, then reap", sweep.name, lane.place, self.tilde(&directory));
            self.count_archived(lane);
            return true;
        }
        let spec = archive::Lane { main: &sweep.main, base: &sweep.base, project: &sweep.name, worktree: lane.worktree, branch: lane.branch, tip: lane.tip, issues: lane.issues, why: &why };
        match archive::archive(&spec, &self.options.archive, self.now_ms) {
            Ok(file) => {
                println!("{}: archived {} ({why}) to {}, then reaped", sweep.name, lane.place, self.tilde(&file));
                sweep.checked_out.remove(lane.branch);
                self.count_archived(lane);
                true
            }
            Err(error) => {
                println!("{}: kept {}: {error}", sweep.name, lane.place);
                false
            }
        }
    }

    fn count_archived(&mut self, lane: &ArchiveLane) {
        self.counts.archived += 1;
        self.counts.retired_worktrees += usize::from(lane.worktree.is_some());
        self.counts.deleted_branches += usize::from(!lane.branch.is_empty());
    }

    fn retire_worktree(&mut self, sweep: &mut Container, lane: &Worktree, live: &[PathBuf], basis: &str) -> bool {
        let reason = if let Some(ticket) = land_queue::waiting_on(&land_queue::queues(&self.home.join(".local/state")), &land_queue::local_host(), &lane.path).first() {
            format!("land ticket {} still waits", ticket.display())
        } else if lane.landed.how != "ancestor" {
            self.held(lane, live)
        } else if lane.locked {
            "locked".into()
        } else if protected_branch(&lane.branch) {
            "protected branch".into()
        } else if any_inside(live, &real(&lane.path)) {
            "process inside".into()
        } else {
            String::new()
        };
        let facts = Facts { age: (sweep.cutoff_ms - lane.activity_ms).floor() as i64, minimum_age: 0, merged: true, equivalent: false, evil_merge: false, archive_age: None, protections: u32::from(!reason.is_empty()) };
        if retirement_verdict(&facts) == Verdict::Keep {
            let why = if reason.is_empty() { format!("touched within {}h", self.options.idle_hours) } else { reason };
            self.keep(sweep, &lane.path, &why);
            return false;
        }
        let read_only = command(&["find", &lane.path.to_string_lossy(), "-type", "d", "!", "-perm", "-u+w", "-print", "-quit"]).out;
        if !read_only.is_empty() {
            let why = format!("read-only directory {}", self.tilde(Path::new(&read_only)));
            self.keep(sweep, &lane.path, &why);
            return false;
        }
        if self.options.dry_run {
            println!("{}: retire {} ({}, {basis})", sweep.name, lane.place, if lane.branch.is_empty() { "detached" } else { &lane.branch });
            self.counts.retired_worktrees += 1;
            self.counts.deleted_branches += usize::from(!lane.branch.is_empty());
            return true;
        }
        let removed = git(&sweep.main, &["worktree", "remove", &lane.path.to_string_lossy()]);
        if !removed.ok {
            self.keep(sweep, &lane.path, &format!("worktree remove failed: {}", removed.last_err_line()));
            println!("{}: kept {}: {}", sweep.name, lane.place, removed.last_err_line());
            return false;
        }
        self.counts.retired_worktrees += 1;
        sweep.checked_out.remove(&lane.branch);
        if !lane.branch.is_empty() && lane.branch != "main" && self.delete_branch(&sweep.main, &lane.branch, &lane.tip) {
            self.counts.deleted_branches += 1;
        }
        true
    }

    fn sweep_branch(&mut self, sweep: &mut Container, branch: &str, tip: &str) -> Result<(), String> {
        if protected_branch(branch) || sweep.checked_out.contains(branch) {
            return Ok(());
        }
        let verdict = sweep.landed(tip);
        let branch_age = sweep.cutoff_ms - reflog_ms(&sweep.common.join("logs/refs/heads").join(branch));
        if !verdict.landed {
            return self.decide_branch(sweep, branch, tip, &verdict);
        }
        self.delete_branch_after_idle(sweep, branch, tip, branch_age, &verdict.how);
        Ok(())
    }

    fn delete_branch_after_idle(&mut self, sweep: &Container, branch: &str, tip: &str, branch_age: f64, basis: &str) {
        let facts = Facts { age: branch_age.floor() as i64, minimum_age: 0, merged: true, equivalent: false, evil_merge: false, archive_age: None, protections: 0 };
        if retirement_verdict(&facts) == Verdict::Keep {
            self.counts.kept += 1;
        } else if self.options.dry_run {
            println!("{}: delete branch {branch} ({basis})", sweep.name);
            self.counts.deleted_branches += 1;
        } else if self.delete_branch(&sweep.main, branch, tip) {
            self.counts.deleted_branches += 1;
        }
    }

    fn decide_branch(&mut self, sweep: &mut Container, branch: &str, tip: &str, verdict: &Landed) -> Result<(), String> {
        let name = sweep.name.clone();
        let issues = lane_issues(&sweep.main, &sweep.own, &sweep.base, branch, tip).unwrap_or_default();
        let states = self.issue_states(&issues);
        let class = lane_class(&LaneFacts { dirty: false, equivalent: false, issues: &states, idle_hours: 0.0, held: false });
        if let Some(window) = window_hours(&states) {
            let touched_ms = reflog_ms(&sweep.common.join("logs/refs/heads").join(branch)).max(sweep.committed(tip) as f64 * 1000.0);
            let age_h = (self.now_ms as f64 - touched_ms) / HOUR_MS;
            let why = if age_h < window { format!("touched within {} days", window / 24.0) } else { self.local_archive_guard(&sweep.main, &name, &issues, branch, None) };
            if why.is_empty() {
                let found = ArchiveLane { worktree: None, branch, tip, place: &format!("branch {branch}"), issues: &issues };
                if self.archive_lane(sweep, &found) {
                    return Ok(());
                }
            } else if self.options.dry_run {
                println!("{name}: {} branch {branch}: keep ({why})", class.label());
            }
        }
        let row = Row { place: "(branch)", branch, ahead: verdict.ahead, dirty: "", class };
        let hours = self.unlanded_row(&name, &sweep.main, &sweep.base, tip, sweep.committed(tip), &row);
        let owner = self.surface(sweep, "(branch)", branch, hours, &issues)?;
        self.list_open(&issues, &states, &format!("branch {branch}"));
        self.counts.kept += 1;
        if self.options.dry_run {
            println!("{name}: {} branch {branch}: surface to {owner}{}", class.label(), cited(&issues));
        }
        Ok(())
    }
}

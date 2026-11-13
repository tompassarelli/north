use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::Gc;
use super::archive::window_hours;
use super::evidence::{LaneFacts, committed, committed_each, lane_class, lane_issues, landed, landed_each, origin_repo, protected_branch};
use super::exec::{command, git, git_slow, real, verified};
use super::half::Lanes;
use super::judged::Landed;
use super::liveness::{holds_git_dir, worktree_admin};
use super::report::Row;
use super::state::{LocalStale, Owned};
use super::strays::modified_within;
use super::super::landed_tickets::LandedTickets;
use super::super::{lane_issue, lane_rejection};
use super::super::live_process::any_inside;
use super::super::reap::irreplaceable_ignored;
use super::super::repo::is_lander_tree;
use super::super::verdict::{Facts, Verdict, retirement_verdict};
use north_base::foundation::clock::Timestamp;
use north_base::platform::land_queue;

pub(super) mod activity;
mod archive_lane;
mod branch;
mod config;
mod surface;

use self::activity::{branch_of, lane_activity_ms, parse_worktrees};

pub(super) const HOUR_MS: f64 = 3_600_000.0;
const BASES: [&str; 3] = ["refs/remotes/origin/main", "refs/remotes/origin/HEAD", "refs/heads/main"];

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
    tickets: LandedTickets,
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
    rejected: Option<&'a str>,
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
        let tickets = LandedTickets::read(&self.home.join(".local/state"), &main);
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
            tickets,
            landed,
            committed,
        };
        let swept = self.sweep_lanes(&mut sweep, &worktrees, live);
        self.stale.repo(&sweep.name).local_stale = std::mem::take(&mut sweep.local_stale);
        swept?;
        self.prune_branch_config(&sweep);
        sweep.owned.sort_by_key(|row| std::cmp::Reverse(row.age_h));
        self.stale.repo(&sweep.name).owned = Some(sweep.owned);
        if !dry {
            git(&sweep.main, &["worktree", "prune"]);
            git_slow(&sweep.main, &["remote", "prune", "origin"]);
        }
        Ok(())
    }

    fn sweep_lanes(&mut self, sweep: &mut Container, worktrees: &[BTreeMap<String, String>], live: &[PathBuf]) -> Result<(), String> {
        let registered: Vec<PathBuf> = worktrees.iter().filter(|fields| !fields.contains_key("prunable")).map(|fields| real(Path::new(&fields["worktree"]))).collect();
        let lanes = Lanes { main: &sweep.main, base: &sweep.base, project: &sweep.name, directory: &sweep.lanes, common: &sweep.common, registered: &registered, live };
        self.archive_unregistered(&lanes);
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
        if self.options.dry_run {
            return true;
        }
        let deleted = git(main, &["update-ref", "-d", &format!("refs/heads/{branch}"), tip]).ok;
        if deleted {
            lane_issue::forget(main, branch);
        }
        deleted
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
        let landed = sweep.landed(&tip);
        let proven = if landed.landed || !dirty.is_empty() { None } else { sweep.tickets.landed(&sweep.main, &sweep.base, &path, &tip).map(|commit| format!("landed as {commit}")) };
        let landed = match proven {
            Some(how) => Landed { landed: true, ahead: landed.ahead, how },
            None => landed,
        };
        let admin = PathBuf::from(worktree_admin(&path));
        let activity_ms = lane_activity_ms(&path, &admin, &sweep.common, &branch, sweep.committed(&tip) as f64 * 1000.0);
        let lane = Worktree {
            place: self.tilde(&path),
            landed,
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

    fn decide_worktree(&mut self, sweep: &mut Container, lane: &Worktree, live: &[PathBuf]) -> Result<bool, String> {
        let dirty = !lane.dirty.is_empty();
        let issues = lane_issues(&sweep.main, &sweep.own, &sweep.base, &lane.branch, &lane.tip).unwrap_or_default();
        let states = self.issue_states(&issues);
        let held = self.held(lane, live);
        if held.is_empty() && self.archive_rejected(sweep, lane, &issues) {
            return Ok(true);
        }
        let idle_hours = if dirty && self.idle_for(lane, 24.0) { lane.age_h } else { 0.0 };
        let class = lane_class(&LaneFacts { dirty, equivalent: lane.landed.landed, issues: &states, idle_hours, held: !held.is_empty() });
        if let Some(window) = window_hours(&states) {
            let why = if held.is_empty() { self.archive_hold(sweep, lane, window, &issues) } else { held };
            if why.is_empty() {
                let found = ArchiveLane { worktree: Some(&lane.path), branch: &lane.branch, tip: &lane.tip, place: &lane.place, issues: &issues, rejected: None };
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

    fn archive_rejected(&mut self, sweep: &mut Container, lane: &Worktree, issues: &[String]) -> bool {
        let Some(verdict) = lane_rejection::recorded(&sweep.main, &lane.branch) else { return false };
        if !irreplaceable_ignored(&lane.path).is_empty() {
            return false;
        }
        if !land_queue::waiting_on(&land_queue::queues(&self.home.join(".local/state")), &land_queue::local_host(), &lane.path).is_empty() {
            return false;
        }
        let found = ArchiveLane { worktree: Some(&lane.path), branch: &lane.branch, tip: &lane.tip, place: &lane.place, issues, rejected: Some(&verdict) };
        self.archive_lane(sweep, &found)
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
}

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use super::Gc;
use super::archive::{Lane, Retired};
use super::evidence::{LaneClass, LaneFacts, Landed, lane_class, landed, named_issues, origin_repo, protected_branch};
use super::exec::{command, git, git_slow, real, verified};
use super::super::live_process::any_inside;
use super::liveness::{holds_git_dir, worktree_admin};
use super::report::Row;
use super::state::{LocalStale, Owned};
use super::super::verdict::{Facts, Verdict, retirement_verdict};
use crate::foundation::clock::Timestamp;

const BASES: [&str; 3] = ["refs/remotes/origin/main", "refs/remotes/origin/HEAD", "refs/heads/main"];
const HOUR_MS: f64 = 3_600_000.0;

struct Container {
    name: String,
    main: PathBuf,
    base: String,
    common: PathBuf,
    lanes: PathBuf,
    cutoff_ms: f64,
    github: bool,
    previous: HashMap<String, String>,
    local_stale: Vec<LocalStale>,
    owned: Vec<Owned>,
    checked_out: HashSet<String>,
}

struct Classified {
    label: &'static str,
    expired: bool,
    dirty: bool,
    unlanded: bool,
    issues: Vec<String>,
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

impl Worktree {
    fn label(&self) -> String {
        if self.branch.is_empty() { self.path.file_name().unwrap_or_default().to_string_lossy().into_owned() } else { self.branch.clone() }
    }
}

fn parse_worktrees(porcelain: &str) -> Vec<BTreeMap<String, String>> {
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

fn short(tip: &str) -> &str {
    &tip[..tip.len().min(12)]
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
        let worktrees = parse_worktrees(&git(&main, &["worktree", "list", "--porcelain"]).out);
        let previous = self.stale.repo(&name).local_stale.iter().map(|row| (row.path.clone(), row.flagged_at.clone())).collect();
        let mut sweep = Container {
            github: origin_repo(&main).is_some(),
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
        let since = Timestamp::from_unix_millis(self.now_ms - (hours * HOUR_MS) as i64).iso_millis();
        let found = command(&["find", &path.to_string_lossy(), "-type", "f", "-newermt", &since, "-print", "-quit"]);
        !found.ok || !found.out.is_empty()
    }

    fn idle_for(&self, lane: &Worktree, hours: f64) -> bool {
        lane.age_h >= hours && !self.modified_since(&lane.path, hours)
    }

    fn delete_branch(&self, main: &Path, branch: &str, tip: &str) -> bool {
        self.options.dry_run || git(main, &["update-ref", "-d", &format!("refs/heads/{branch}"), tip]).ok
    }

    fn sweep_worktree(&mut self, sweep: &mut Container, fields: &BTreeMap<String, String>, live: &[PathBuf]) -> Result<(), String> {
        let path = PathBuf::from(&fields["worktree"]);
        if real(&path).parent() != Some(sweep.lanes.as_path()) {
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
        let mut stamps = vec![path.clone(), admin.join("HEAD"), admin.join("index"), admin.join("logs/HEAD")];
        if !branch.is_empty() {
            stamps.push(sweep.common.join("logs/refs/heads").join(&branch));
        }
        let committed = git(&sweep.main, &["log", "-1", "--format=%ct", &tip]).out.parse::<f64>().unwrap_or(0.0) * 1000.0;
        let activity_ms = committed.max(newest_mtime(&stamps));
        let lane = Worktree {
            place: self.tilde(&path),
            landed: landed(&sweep.main, &sweep.base, &tip),
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
        if lane.landed.how != "ancestor" || !dirty.is_empty() {
            return self.classify_worktree(sweep, &lane, flagged, live);
        }
        self.retire_merged_worktree(sweep, &lane, live);
        Ok(())
    }

    fn flag(&mut self, sweep: &mut Container, lane: &Worktree) -> Result<(), String> {
        let path = lane.path.display().to_string();
        let now = Timestamp::from_unix_millis(self.now_ms).iso_millis();
        sweep.local_stale.push(LocalStale {
            flagged_at: sweep.previous.get(&path).filter(|at| !at.is_empty()).cloned().unwrap_or(now),
            retire_at: Timestamp::from_unix_millis_f64(lane.activity_ms + 72.0 * HOUR_MS).iso_millis(),
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
        sweep.owned.push(Owned { place: place.into(), branch: branch.into(), age_h, refs: refs.to_vec(), owner: owner.clone() });
        Ok(owner)
    }

    fn classify_worktree(&mut self, sweep: &mut Container, lane: &Worktree, flagged: bool, live: &[PathBuf]) -> Result<(), String> {
        let dirty = !lane.dirty.is_empty();
        let issues = named_issues(&sweep.main, &sweep.name, &sweep.base, &lane.tip).unwrap_or_default();
        let held = self.held(lane, live);
        let states = if dirty || lane.landed.landed { Vec::new() } else { self.issue_states(&issues) };
        let idle_hours = if dirty && self.idle_for(lane, 24.0) { lane.age_h } else { 0.0 };
        let class = lane_class(&LaneFacts { dirty, equivalent: lane.landed.landed, issues: &states, idle_hours, held: !held.is_empty() });
        let expired = class == LaneClass::Owned && !dirty && flagged && !lane.branch.is_empty() && self.idle_for(lane, 72.0);
        let verdict = Classified { label: class.label(), expired, dirty, unlanded: !lane.landed.landed || dirty, issues };
        if class == LaneClass::Owned && !expired {
            return self.surface_worktree(sweep, lane, &verdict.issues);
        }
        let why = self.worktree_hold(sweep, lane, class, held);
        if !why.is_empty() {
            return self.hold_worktree(sweep, lane, &verdict, &why);
        }
        self.retire_classified(sweep, lane, &verdict)
    }

    fn row<'a>(lane: &'a Worktree) -> Row<'a> {
        Row { place: &lane.place, branch: &lane.branch, ahead: lane.landed.ahead, dirty: lane.dirty }
    }

    fn surface_worktree(&mut self, sweep: &mut Container, lane: &Worktree, issues: &[String]) -> Result<(), String> {
        self.unlanded_row(&sweep.name, &sweep.main, &sweep.base, &lane.tip, &Self::row(lane));
        let shown = if lane.branch.is_empty() { "(detached)" } else { &lane.branch };
        let owner = self.surface(sweep, &lane.place, shown, lane.age_h.floor() as i64, issues)?;
        self.counts.kept += 1;
        if self.options.dry_run {
            println!("{}: owned {}: surface to {owner}{}", sweep.name, lane.place, cited(issues));
        }
        Ok(())
    }

    fn hold_worktree(&mut self, sweep: &mut Container, lane: &Worktree, verdict: &Classified, why: &str) -> Result<(), String> {
        self.counts.kept += 1;
        if verdict.unlanded {
            self.unlanded_row(&sweep.name, &sweep.main, &sweep.base, &lane.tip, &Self::row(lane));
        }
        if verdict.expired {
            self.surface(sweep, &lane.place, &lane.branch, lane.age_h.floor() as i64, &verdict.issues)?;
        }
        if self.options.dry_run {
            println!("{}: {} {}: keep ({why})", sweep.name, verdict.label, lane.place);
        }
        Ok(())
    }

    fn retire_classified(&mut self, sweep: &mut Container, lane: &Worktree, verdict: &Classified) -> Result<(), String> {
        let (name, label) = (sweep.name.clone(), verdict.label);
        if self.options.dry_run {
            let target = if verdict.dirty { "<snapshot>" } else { short(&lane.tip) };
            let action = if verdict.expired { "archive and retire (idle 72h)".into() } else { format!("archive to archive/{name}/{}/{target}, then retire", lane.label()) };
            println!("{name}: {label} {}: {action}", lane.place);
            self.counts.retired_worktrees += 1;
            self.counts.deleted_branches += usize::from(!lane.branch.is_empty());
            return Ok(());
        }
        let retiring = Lane { path: Some(&lane.path), branch: &lane.branch, tip: &lane.tip, dirty: verdict.dirty, label: &lane.label() };
        let retired = self.retire_lane(&sweep.main, &name, &retiring)?;
        if !retired.retired {
            self.counts.kept += 1;
            if verdict.unlanded {
                self.unlanded_row(&name, &sweep.main, &sweep.base, &lane.tip, &Self::row(lane));
            }
            println!("{name}: {label} {}: kept ({})", lane.place, retired.why);
            return Ok(());
        }
        self.counts.retired_worktrees += 1;
        sweep.checked_out.remove(&lane.branch);
        self.counts.deleted_branches += usize::from(!lane.branch.is_empty() && !retired.why.starts_with("branch"));
        let path = lane.path.display().to_string();
        sweep.local_stale.retain(|row| row.path != path);
        self.announce(sweep, if verdict.expired { "owned" } else { label }, &lane.place, &lane.label(), &lane.tip, &retired)
    }

    fn worktree_hold(&mut self, sweep: &Container, lane: &Worktree, class: LaneClass, held: String) -> String {
        if !held.is_empty() {
            return held;
        }
        let idle = self.options.idle_hours;
        if matches!(class, LaneClass::LandedElsewhere | LaneClass::Superseded) && !self.idle_for(lane, idle) {
            return format!("touched within {idle}h");
        }
        self.local_archive_guard(&sweep.main, &sweep.name, &sweep.base, &lane.branch, &lane.tip, Some(&lane.path))
    }

    fn announce(&mut self, sweep: &Container, class: &str, place: &str, label: &str, tip: &str, retired: &Retired) -> Result<(), String> {
        let note = if retired.why.is_empty() { String::new() } else { format!("; {}", retired.why) };
        println!("{}: {class} {place}: archived to {} and retired{note}", sweep.name, retired.tag);
        self.notice(&sweep.name, label, &format!("retired ({class})"), &retired.tag)?;
        if sweep.github {
            self.archive_notes(&sweep.main, &sweep.name, &sweep.base, label, tip, &retired.tag)?;
        }
        Ok(())
    }

    fn retire_merged_worktree(&mut self, sweep: &mut Container, lane: &Worktree, live: &[PathBuf]) {
        let reason = if lane.locked {
            "locked"
        } else if protected_branch(&lane.branch) {
            "protected branch"
        } else if any_inside(live, &real(&lane.path)) {
            "process inside"
        } else {
            ""
        };
        let facts = Facts { age: (sweep.cutoff_ms - lane.activity_ms).floor() as i64, minimum_age: 0, merged: true, equivalent: false, evil_merge: false, archive_age: None, protections: u32::from(!reason.is_empty()) };
        if retirement_verdict(&facts) == Verdict::Keep {
            let why = if reason.is_empty() { format!("touched within {}h", self.options.idle_hours) } else { reason.to_string() };
            return self.keep(sweep, &lane.path, &why);
        }
        let read_only = command(&["find", &lane.path.to_string_lossy(), "-type", "d", "!", "-perm", "-u+w", "-print", "-quit"]).out;
        if !read_only.is_empty() {
            let why = format!("read-only directory {}", self.tilde(Path::new(&read_only)));
            return self.keep(sweep, &lane.path, &why);
        }
        if self.options.dry_run {
            println!("{}: retire {} ({}, {})", sweep.name, lane.place, if lane.branch.is_empty() { "detached" } else { &lane.branch }, lane.landed.how);
            self.counts.retired_worktrees += 1;
            self.counts.deleted_branches += usize::from(!lane.branch.is_empty());
            return;
        }
        let removed = git(&sweep.main, &["worktree", "remove", &lane.path.to_string_lossy()]);
        if !removed.ok {
            self.keep(sweep, &lane.path, &format!("worktree remove failed: {}", removed.last_err_line()));
            println!("{}: kept {}: {}", sweep.name, lane.place, removed.last_err_line());
            return;
        }
        self.counts.retired_worktrees += 1;
        sweep.checked_out.remove(&lane.branch);
        if !lane.branch.is_empty() && lane.branch != "main" && self.delete_branch(&sweep.main, &lane.branch, &lane.tip) {
            self.counts.deleted_branches += 1;
        }
    }

    fn sweep_branch(&mut self, sweep: &mut Container, branch: &str, tip: &str) -> Result<(), String> {
        if protected_branch(branch) || sweep.checked_out.contains(branch) {
            return Ok(());
        }
        let verdict = landed(&sweep.main, &sweep.base, tip);
        let branch_age = sweep.cutoff_ms - newest_mtime(&[sweep.common.join("logs/refs/heads").join(branch)]);
        if verdict.how != "ancestor" {
            return self.classify_branch(sweep, branch, tip, &verdict, branch_age);
        }
        let facts = Facts { age: branch_age.floor() as i64, minimum_age: 0, merged: verdict.landed, equivalent: false, evil_merge: false, archive_age: None, protections: 0 };
        if retirement_verdict(&facts) == Verdict::Keep {
            self.counts.kept += 1;
        } else if self.options.dry_run {
            println!("{}: delete branch {branch} ({})", sweep.name, verdict.how);
            self.counts.deleted_branches += 1;
        } else if self.delete_branch(&sweep.main, branch, tip) {
            self.counts.deleted_branches += 1;
        }
        Ok(())
    }

    fn classify_branch(&mut self, sweep: &mut Container, branch: &str, tip: &str, verdict: &Landed, branch_age: f64) -> Result<(), String> {
        let name = sweep.name.clone();
        let issues = named_issues(&sweep.main, &name, &sweep.base, tip).unwrap_or_default();
        let states = if verdict.landed { Vec::new() } else { self.issue_states(&issues) };
        let class = lane_class(&LaneFacts { dirty: false, equivalent: verdict.landed, issues: &states, idle_hours: 0.0, held: false });
        let row = Row { place: "(branch)", branch, ahead: verdict.ahead, dirty: "" };
        if class == LaneClass::Owned {
            let hours = self.unlanded_row(&name, &sweep.main, &sweep.base, tip, &row);
            let owner = self.surface(sweep, "(branch)", branch, hours, &issues)?;
            self.counts.kept += 1;
            if self.options.dry_run {
                println!("{name}: owned branch {branch}: surface to {owner}{}", cited(&issues));
            }
            return Ok(());
        }
        let label = class.label();
        let why = if branch_age < 0.0 { format!("touched within {}h", self.options.idle_hours) } else { self.local_archive_guard(&sweep.main, &name, &sweep.base, branch, tip, None) };
        if !why.is_empty() {
            if !verdict.landed {
                self.unlanded_row(&name, &sweep.main, &sweep.base, tip, &row);
            }
            self.counts.kept += 1;
            if self.options.dry_run {
                println!("{name}: {label} branch {branch}: keep ({why})");
            }
            return Ok(());
        }
        if self.options.dry_run {
            println!("{name}: {label} branch {branch}: archive to archive/{name}/{branch}/{}, then delete", short(tip));
            self.counts.deleted_branches += 1;
            return Ok(());
        }
        let retired = self.retire_lane(&sweep.main, &name, &Lane { path: None, branch, tip, dirty: false, label: branch })?;
        if !retired.retired || retired.why.starts_with("branch") {
            if !verdict.landed {
                self.unlanded_row(&name, &sweep.main, &sweep.base, tip, &row);
            }
            self.counts.kept += 1;
            println!("{name}: {label} branch {branch}: kept ({})", retired.why);
            return Ok(());
        }
        self.counts.deleted_branches += 1;
        self.announce(sweep, label, &format!("branch {branch}"), branch, tip, &retired)
    }
}

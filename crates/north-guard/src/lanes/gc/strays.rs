use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use super::Gc;
use super::exec::{command, git, real};
use super::liveness::{holds_git_dir, worktree_admin};
use super::local::parse_worktrees;
use super::super::layout_check::exempt;
use super::super::live_process::any_inside;
use north_core::foundation::clock::Timestamp;

const HOUR_MS: f64 = 3_600_000.0;

pub(super) struct Sweep<'a> {
    pub(super) code: &'a Path,
    pub(super) home: &'a Path,
    pub(super) container: &'a Path,
    pub(super) main: &'a Path,
    pub(super) common: &'a Path,
    pub(super) live: &'a [PathBuf],
    pub(super) now_ms: i64,
    pub(super) idle_hours: f64,
    pub(super) dry_run: bool,
}

pub(super) struct Outcome {
    pub(super) from: PathBuf,
    pub(super) to: PathBuf,
    pub(super) result: Result<(), String>,
}

fn newest_mtime_ms(paths: &[PathBuf]) -> f64 {
    paths
        .iter()
        .filter_map(|path| std::fs::metadata(path).and_then(|meta| meta.modified()).ok())
        .filter_map(|at| at.duration_since(UNIX_EPOCH).ok())
        .map(|since| since.as_secs_f64() * 1000.0)
        .fold(0.0, f64::max)
}

pub(super) fn modified_within(path: &Path, now_ms: i64, hours: f64) -> bool {
    let since = Timestamp::from_unix_millis(now_ms - (hours * HOUR_MS) as i64).iso_millis();
    let found = command(&["find", &path.to_string_lossy(), "-type", "f", "-newermt", &since, "-print", "-quit"]);
    !found.ok || !found.out.is_empty()
}

fn stray_in(sweep: &Sweep, path: &Path) -> bool {
    let real_path = real(path);
    let outside = |sub: &str| !real_path.starts_with(real(&sweep.container.join(sub)));
    real_path != real(sweep.main) && outside("worktrees") && outside("pins") && !exempt(sweep.code, sweep.home, path)
}

fn hold(sweep: &Sweep, fields: &BTreeMap<String, String>, path: &Path, in_fleet: &mut dyn FnMut(&Path) -> Result<bool, String>) -> String {
    let admin = PathBuf::from(worktree_admin(path));
    let touched = newest_mtime_ms(&[path.to_path_buf(), admin.join("HEAD"), admin.join("index")]);
    if fields.contains_key("locked") {
        "locked".into()
    } else if fields.contains_key("prunable") || !path.exists() {
        "prunable".into()
    } else if holds_git_dir(path, sweep.common) {
        "holds .git".into()
    } else if any_inside(sweep.live, &real(path)) {
        "process inside".into()
    } else if (sweep.now_ms as f64 - touched) / HOUR_MS < sweep.idle_hours || modified_within(path, sweep.now_ms, sweep.idle_hours) {
        format!("touched within {}h", sweep.idle_hours)
    } else {
        match in_fleet(path) {
            Ok(true) => "live Fleet session in lane".into(),
            Ok(false) => String::new(),
            Err(_) => "Fleet unavailable".into(),
        }
    }
}

pub(super) fn move_idle(sweep: &Sweep, in_fleet: &mut dyn FnMut(&Path) -> Result<bool, String>) -> Vec<Outcome> {
    let porcelain = git(sweep.main, &["worktree", "list", "--porcelain"]).out;
    let mut outcomes = Vec::new();
    for fields in parse_worktrees(&porcelain) {
        let path = PathBuf::from(&fields["worktree"]);
        if !stray_in(sweep, &path) {
            continue;
        }
        let to = sweep.container.join("worktrees").join(path.file_name().unwrap_or_default());
        let why = hold(sweep, &fields, &path, in_fleet);
        let result = if !why.is_empty() {
            Err(why)
        } else if to.exists() {
            Err(format!("{} exists", to.display()))
        } else if sweep.dry_run {
            Ok(())
        } else {
            let moved = git(sweep.main, &["worktree", "move", &path.to_string_lossy(), &to.to_string_lossy()]);
            if moved.ok { Ok(()) } else { Err(format!("worktree move failed: {}", moved.last_err_line())) }
        };
        outcomes.push(Outcome { from: path, to, result });
    }
    outcomes
}

impl Gc {
    pub(super) fn move_strays(&mut self, container: &Path, main: &Path, common: &Path, live: &[PathBuf]) {
        let code = container.parent().unwrap_or(container).to_path_buf();
        let (home, now_ms, idle_hours, dry_run) = (self.home.clone(), self.now_ms, self.options.idle_hours, self.options.dry_run);
        let sweep = Sweep { code: &code, home: &home, container, main, common, live, now_ms, idle_hours, dry_run };
        let name = container.file_name().unwrap_or_default().to_string_lossy().into_owned();
        for outcome in move_idle(&sweep, &mut |path| self.session_in(path)) {
            let (from, to) = (self.tilde(&outcome.from), self.tilde(&outcome.to));
            match outcome.result {
                Ok(()) if dry_run => println!("{name}: stray {from}: move to {to}"),
                Ok(()) => println!("{name}: stray {from}: moved to {to}"),
                Err(why) => println!("{name}: stray {from}: kept in place ({why})"),
            }
        }
    }
}

//! Where the finish call takes a draft lane (north#1240): it commits in the lane as always, then runs the
//! box's check once more with the draft's files put back as they were on the draft's base, records the draft
//! in the lane's mark as `draft-pass`, or `passes-on-base` when that check passes without the change, and
//! queues nothing, so a draft lands only through its box's launch.
use std::path::Path;
use std::process::{Command, Stdio};

use crate::lanes::draft_mark::{self, Mark};
use north_lander::land::hand_in::{Lane, Road};

pub(super) struct Draft {
    pub mark: Mark,
}

impl Road for Draft {
    fn failed(&self, root: &Path, failure: &str) {
        let mark = Mark { outcome: Some("draft-fail".to_owned()), failure: Some(failure.to_owned()), ..self.mark.clone() };
        if let Err(why) = draft_mark::write(root, &mark) {
            eprintln!("finish: draft {}: {why}", mark.name);
        }
    }

    fn check(&self, root: &Path, check: &str, head: Option<&str>) -> Option<Result<(), (String, String)>> {
        super::remote_land::check_through_broker(root, check, head)
    }

    fn writes_finish_records(&self) -> bool {
        !north_runtime::worker_sandbox::confined()
    }

    fn take(self, lane: &Lane) -> u8 {
        let root = lane.root.as_path();
        let files: Vec<String> = git(root, &["diff", "--name-only", &self.mark.base, "HEAD"]).map(|text| text.lines().map(str::to_owned).collect()).unwrap_or_default();
        let base_exit = on_base(root, &self.mark, &files);
        let outcome = if base_exit == Some(0) { "passes-on-base" } else { "draft-pass" };
        let mark = Mark { outcome: Some(outcome.to_owned()), commit: git(root, &["rev-parse", "HEAD"]), files, base_exit, failure: None, ..self.mark };
        match draft_mark::write(root, &mark) {
            Ok(()) => {
                println!("finish: ✓ draft {} recorded as {outcome} at {}; not queued: a draft lands only when its box is launched (north#1240).", mark.name, mark.commit.as_deref().unwrap_or("HEAD"));
                0
            }
            Err(why) => {
                eprintln!("finish: ✗ draft {}: {why}; committed, NOT recorded, NOT queued.", mark.name);
                1
            }
        }
    }
}

fn git(root: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git").arg("-C").arg(root).args(arguments).stdin(Stdio::null()).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn on_base(root: &Path, mark: &Mark, files: &[String]) -> Option<i32> {
    if files.is_empty() {
        return None;
    }
    let restore = |source: &str| Command::new("git").arg("-C").arg(root).args(["restore", "--source", source, "--worktree", "--"]).args(files).stdin(Stdio::null()).status().is_ok_and(|status| status.success());
    let exit = restore(&mark.base).then(|| Command::new("sh").arg("-c").arg(&mark.check).current_dir(root).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().ok().and_then(|status| status.code())).flatten();
    restore("HEAD");
    exit
}

/// `north lane land-draft LANE --item REPO#N --box TEXT` (north#1233): lands a passing draft when its box is launched.
/// It rebases the draft onto current main, runs its check there again, stamps the box's trailers and hands it to
/// safe-push, whose landing checks run on current main too; it prints `landed SHA` and exits 0, or names why not.
pub fn land(arguments: &[String]) -> u8 {
    let (lane, item, box_text) = match arguments {
        [lane, item_flag, item, box_flag, box_text] if item_flag == "--item" && box_flag == "--box" => (Path::new(lane), item, box_text),
        _ => {
            eprintln!("usage: north lane land-draft LANE --item REPO#N --box TEXT");
            return 2;
        }
    };
    match ready(lane, item, box_text) {
        Ok(()) => {}
        Err(why) => {
            eprintln!("candidate unusable: {why}");
            return 3;
        }
    }
    if let Err(error) = std::env::set_current_dir(lane) {
        eprintln!("candidate unusable: {}: {error}", lane.display());
        return 3;
    }
    let code = super::remote_land::safe_push(&["--to".to_owned(), "main".to_owned(), "--wait".to_owned()]);
    if code == 0 {
        println!("landed {}", git(lane, &["rev-parse", "HEAD"]).unwrap_or_default());
    }
    code
}

fn ready(lane: &Path, item: &str, box_text: &str) -> Result<(), String> {
    let mark = draft_mark::read(lane).ok_or_else(|| format!("{} holds no draft mark", lane.display()))?;
    match (mark.outcome.as_deref(), mark.commit.as_deref()) {
        (Some("draft-pass"), Some(commit)) if git(lane, &["rev-parse", "HEAD"]).as_deref() == Some(commit) => {}
        (outcome, _) => return Err(format!("the draft is {}, not a passing commit at the lane's head", outcome.unwrap_or("unfinished"))),
    }
    git(lane, &["fetch", "-q", "origin", "main"]).ok_or("git fetch origin main failed")?;
    if git(lane, &["rebase", "-q", "origin/main"]).is_none() {
        git(lane, &["rebase", "--abort"]);
        return Err("stale: main moved under its files and the draft no longer rebases onto it".to_owned());
    }
    let message = git(lane, &["log", "-1", "--format=%B"]).unwrap_or_default();
    if !message.lines().any(|line| line.starts_with("Box: ")) {
        let (item_trailer, box_trailer) = (format!("Item: {item}"), format!("Box: {}", box_text.trim()));
        git(lane, &["commit", "-q", "--amend", "--no-edit", "--trailer", &item_trailer, "--trailer", &box_trailer]).ok_or("cannot add the Item and Box trailers")?;
    }
    let output = Command::new("sh").arg("-c").arg(&mark.check).current_dir(lane).stdin(Stdio::null()).output().map_err(|error| format!("check `{}` did not start: {error}", mark.check))?;
    if output.status.success() {
        return Ok(());
    }
    let tail: Vec<String> = String::from_utf8_lossy(&output.stderr).lines().chain(String::from_utf8_lossy(&output.stdout).lines()).map(str::to_owned).collect::<Vec<_>>();
    let tail = tail[tail.len().saturating_sub(20)..].join("\n");
    Err(format!("failed: check `{}` exited {} on current main:\n{tail}", mark.check, output.status.code().map_or_else(|| "on a signal".to_owned(), |code| code.to_string())))
}

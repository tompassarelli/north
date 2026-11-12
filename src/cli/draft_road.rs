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

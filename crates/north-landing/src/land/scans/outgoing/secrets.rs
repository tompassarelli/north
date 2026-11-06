use std::path::Path;
use std::process::Stdio;

use crate::landing::safe_push_state::refuse;
use crate::landing::step::{Refused, Step};
use crate::platform::betterleaks;

pub fn history_secret_scan(dir: &Path, revisions: &[String]) -> Step {
    let clean = betterleaks::command("betterleaks")
        .current_dir(dir)
        .args(betterleaks::history_args(&revisions.join(" ")))
        .status()
        .is_ok_and(|status| status.success());
    if clean { Ok(()) } else { Err(refuse("safe-push: ✗ SECRET DETECTED in tag commit history — fix it; NOT pushed.")) }
}

pub fn annotation_secret_scan(dir: &Path, annotation: &[u8]) -> Step {
    let scratch = tempfile::tempdir().map_err(|_| Refused(1))?;
    std::fs::write(scratch.path().join("tag-object.txt"), annotation).map_err(|_| Refused(1))?;
    let clean = betterleaks::command("betterleaks")
        .current_dir(dir)
        .args(betterleaks::directory_args(scratch.path()))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    if clean { Ok(()) } else { Err(refuse("safe-push: ✗ SECRET DETECTED in tag annotation — fix it; NOT pushed.")) }
}

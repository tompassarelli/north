//! The newest head of main whose full test line passed in the hosted workflow `.safe-push` names under
//! `landing.full-line-workflow`: a waiting after-landing run of a landing that head descends from verifies nothing that
//! pass did not, so it holds no build-host slot (north#1537).
use std::process::Command;

use super::super::alone_pass::landings_since;
use super::{Batch, Lander, setting};

const WORKFLOW: &str = "landing.full-line-workflow";

/// The head of main's newest successful run of the full-line workflow, `None` when the repository names none or GitHub
/// does not answer.
pub(super) fn passed(lander: &Lander) -> Option<String> {
    let workflow = setting::landing(&lander.repo, "refs/remotes/origin/main", WORKFLOW);
    let workflow = workflow.trim();
    if workflow.is_empty() {
        return None;
    }
    let endpoint = format!("repos/{{owner}}/{{repo}}/actions/workflows/{workflow}/runs?branch=main&status=success&per_page=1");
    match Command::new("gh").args(["api", &endpoint, "--jq", ".workflow_runs[0].head_sha // empty"]).current_dir(&lander.worktree).output() {
        Ok(output) if output.status.success() => Some(String::from_utf8_lossy(&output.stdout).trim().to_owned()).filter(|head| !head.is_empty()),
        Ok(output) => {
            println!("north land: could not read the newest pass of {workflow} on main, so no waiting after-landing run is dropped: {}", String::from_utf8_lossy(&output.stderr).trim());
            None
        }
        Err(error) => {
            println!("north land: could not run gh to read the newest pass of {workflow} on main, so no waiting after-landing run is dropped: {error}");
            None
        }
    }
}

/// Whether the full line that passed at `passed` covers every landing of `batch`.
pub(super) fn covers(lander: &Lander, passed: &str, batch: &Batch) -> bool {
    landings_since(&lander.worktree, &batch.head, passed).is_some()
}

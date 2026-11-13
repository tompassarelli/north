//! The check a finish call runs before it commits (north#1036): a portable acceptance command joins the lane's run before queueing on the would-be head's tree, keyed as the lander's own run, so the safe-push the lane is handed to reuses it; a command that does not join runs as the repository's own check before queueing when the lane's checks hold it (north#1230), else alone, by the outside broker for a sandboxed session (`Road::check`).

use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

use super::super::enqueue::{Facts, fetch_base, git, judge_static_first};
use super::{Lane, Road};
use crate::land::slots::runner::{self, Accepted, QueueRefusal};

/// The verdict of the finish call's `check` of the would-be head in `checked` (none for a hand-in) on `lane`, and whether the road ran it and wrote its finish record itself.
pub(super) fn verdict(road: &impl Road, lane: &Lane, checked: Option<(&str, &str)>, check: Option<&str>, facts: Option<&Facts>) -> (Result<(), (String, String)>, bool) {
    let base = checked.and_then(|_| destination_base(lane, facts));
    if let Some(((check, head), base)) = checked.zip(base.as_deref()).filter(|((check, head), base)| runner::joins_queue_run(&lane.root, base, head, check)) {
        if let Err(refusal) = judge_static_first(&lane.root, base, head) {
            return (Err(("failed as a check before queueing".to_owned(), refusal)), false);
        }
        return match road.check(&lane.root, check, Some(head)) {
            Some(verdict) => (verdict, true),
            None => (in_queue_run(&lane.root, check, head, base), false),
        };
    }
    let Some(check) = check else { return (Ok(()), false) };
    let head = checked.map(|(_, head)| head);
    if let Some(ran) = head.and_then(|head| as_land_check(lane, check, head, facts)) {
        return (ran.map_err(|refusal| ("failed as a check before queueing".to_owned(), refusal)), false);
    }
    match road.check(&lane.root, check, head) {
        Some(verdict) => (verdict, true),
        None => (passes(&lane.root, check), false),
    }
}

fn as_land_check(lane: &Lane, check: &str, head: &str, facts: Option<&Facts>) -> Option<Result<(), String>> {
    git(&lane.root, &["show", &format!("{head}:.safe-push")]).ok().filter(|config| config.contains(check.trim()))?;
    let (_, base) = facts?.base().or_else(|_| fetch_base(&lane.root, &lane.destination)).ok()?;
    let commands = runner::queue_commands(&lane.root, &base, head).ok()?;
    commands.iter().any(|command| command.trim() == check.trim()).then(|| judge_static_first(&lane.root, &base, head).and_then(|()| runner::refuse_queue_checks(&lane.root, &base, head)))
}

/// The tip of the lane's destination as the finish call's own fetch left it, the base its checks before queueing are keyed on.
fn destination_base(lane: &Lane, facts: Option<&Facts>) -> Option<String> {
    facts.and_then(|facts| facts.base().ok()).or_else(|| fetch_base(&lane.root, &lane.destination).ok()).map(|(_, base)| base)
}

/// Runs the finish call's portable acceptance `check` as one more check of the lane's run before queueing on `base`, on
/// the would-be head `head`'s tree in the same slot (north#1036): the verdict is the run's, so the finish record written
/// from it says what the lander's own keyed run found, and the safe-push the lane is handed to reuses that run.
fn in_queue_run(root: &Path, check: &str, head: &str, base: &str) -> Result<(), (String, String)> {
    let judged = runner::judge_with_acceptance(root, base, head, check, None);
    if judged.passed() {
        return Ok(());
    }
    let failure = match judged.accepted {
        Accepted::Failed => "failed as one more check before queueing",
        Accepted::Passed => "passed, and a check the repository runs before queueing failed",
        Accepted::NotRun => "did not run: a check before queueing failed first",
        Accepted::Unjudged => "reached no verdict as a check before queueing",
    };
    let refusal = judged.run.err().map(QueueRefusal::into_text).unwrap_or_default();
    let output = if judged.output.is_empty() { format!("{refusal}\n") } else { judged.output };
    Err((failure.to_owned(), output))
}

fn passes(root: &Path, check: &str) -> Result<(), (String, String)> {
    if check.trim().is_empty() {
        return Err(("is empty".to_owned(), String::new()));
    }
    let unstarted = |error: io::Error| (format!("did not start: {error}"), String::new());
    let mut log = tempfile::tempfile().map_err(unstarted)?;
    let (out, err) = (log.try_clone().map_err(unstarted)?, log.try_clone().map_err(unstarted)?);
    let status = Command::new("sh").arg("-c").arg(check).current_dir(root).stdin(Stdio::null()).stdout(out).stderr(err).status().map_err(unstarted)?;
    let mut output = String::new();
    let _ = io::Seek::rewind(&mut log).and_then(|()| io::Read::read_to_string(&mut log, &mut output));
    match status.code() {
        Some(0) => {
            eprint!("{output}");
            Ok(())
        }
        Some(code) => Err((format!("exited {code}"), output)),
        None => Err(("was killed by a signal".to_owned(), output)),
    }
}

use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;

use north_base::platform::finish_record::{FinishRecords, Finished};
use north_base::platform::review_accept::{Accepts, patch_id};
use north_base::platform::trust_boundary;

use crate::test_file::is_test_file;
use crate::{Range, git, holdlist};

const SELF: &str = "review-accept";
const REVERTS: &str = "This reverts commit ";

fn changed(directory: &Path, fork: &str, head: &str) -> Option<Vec<String>> {
    git::text(directory, &["diff", "--name-only", "--no-renames", fork, head]).map(|names| names.lines().map(str::to_owned).collect())
}

/// Whether the lane's patch `patch` is, exactly, the reverse of a commit a message of the lane names with git's `This reverts commit` line.
fn reverts(directory: &Path, fork: &str, head: &str, patch: &str) -> bool {
    git::messages(directory, fork, head)
        .lines()
        .filter_map(|line| line.strip_prefix(REVERTS))
        .filter_map(|reverted| reverted.split(|c: char| !c.is_ascii_hexdigit()).next())
        .any(|reverted| patch_id(directory, reverted, &format!("{reverted}^")).is_ok_and(|reverse| reverse == patch))
}

/// Whether the finish call `finished` handed in a commit of the lane `fork..head`, or one whose patch the lane holds under another commit, as when the lander rebases it.
fn authored(directory: &Path, fork: &str, head: &str, finished: &Finished) -> bool {
    let tip = finished.head.as_str();
    if tip.is_empty() || !tip.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return false;
    }
    let in_lane = git::text(directory, &["rev-list", &format!("{fork}..{head}")]).is_some_and(|commits| commits.lines().any(|commit| commit == tip));
    in_lane || git::text(directory, &["cherry", head, tip, fork]).is_some_and(|lines| lines.lines().any(|line| line.starts_with('-')))
}

/// The patch id of `fork..head` and the distinct reviewer sessions that accepted it, leaving out every session that handed in a commit of the lane.
pub(crate) fn reviewers(directory: &Path, fork: &str, head: &str, accepts: &Accepts, finished: &[Finished]) -> Result<(String, BTreeSet<String>), String> {
    let patch = patch_id(directory, fork, head)?;
    let mut accepted = accepts.sessions(&patch)?;
    for author in finished.iter().filter(|finished| authored(directory, fork, head, finished)) {
        accepted.remove(&author.session);
    }
    Ok((patch, accepted))
}

/// `Err` is a lane that changes a `trust_boundary_paths` file and holds fewer accepts than it needs: the accepts of two distinct reviewer sessions for its patch id, or one for a change of test files alone or an exact revert.
pub fn check(directory: &Path, base: &str, head: &str, accepts: &Accepts, finished: &[Finished]) -> Result<(), String> {
    let fork = git::merge_base(directory, base, head);
    let files = changed(directory, &fork, head).ok_or_else(|| format!("git could not diff {fork}..{head}"))?;
    let crossing = trust_boundary::crossing(&files).ok_or("harness/defaults.toml lists no trust_boundary_paths, so no change can be told to cross one")?;
    if crossing.is_empty() {
        return Ok(());
    }
    let (patch, accepted) = reviewers(directory, &fork, head, accepts, finished)?;
    let needed = if files.iter().all(is_test_file) || reverts(directory, &fork, head, &patch) { 1 } else { 2 };
    if accepted.len() >= needed {
        return Ok(());
    }
    let by = accepted.iter().map(String::as_str).collect::<Vec<_>>().join(", ");
    Err(format!("the lane changes {} of trust_boundary_paths and holds {} of {needed} review accepts of its patch {patch} from distinct reviewer sessions that did not author the lane ({}); a reviewer's `north runtime accept {head}` records one", crossing.join(", "), accepted.len(), if by.is_empty() { "none" } else { &by }))
}

pub fn scan(directory: &Path, range: &str, state: &Path, accepts: &Accepts, finished: &FinishRecords, out: &mut impl Write) -> u8 {
    let Some(Range { base, head }) = Range::parse(range) else {
        let _ = writeln!(out, "{SELF}: expected BASE..HEAD, got {range}");
        return 2;
    };
    let outcome = check(directory, base, head, accepts, &finished.all());
    let enforce = holdlist::enforcing(directory, SELF, [&git::merge_base(directory, base, head), head]);
    if !enforce {
        holdlist::record(state, SELF, head, outcome.as_ref().err().map(String::as_str));
    }
    match outcome {
        Ok(()) => 0,
        Err(refusal) => {
            let verb = if enforce { "refused" } else { "would refuse (shadow)" };
            let _ = writeln!(out, "{SELF}: {verb}: {refusal}");
            u8::from(enforce)
        }
    }
}

#[cfg(test)]
#[path = "review_accept_tests.rs"]
mod tests;

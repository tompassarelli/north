use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;
use std::sync::LazyLock;

use north_base::foundation::offload::ONE_LANE_ENV;
use north_base::platform::finish_record::{FinishRecords, Finished};
use north_base::platform::harness::DEFAULTS;
use north_base::platform::review_accept::{Accepts, patch_id};
use north_base::platform::trust_boundary;
use regex::Regex;
use toml_edit::DocumentMut;

use crate::{Range, git, holdlist};

const SELF: &str = "review-accept";
const DELETIONS: &str = "review-accept-deletions";
const REVERTS: [&str; 2] = ["This reverts commit ", "Reverts: "];

fn changed(directory: &Path, fork: &str, head: &str) -> Option<Vec<String>> {
    git::text(directory, &["diff", "--name-only", "--no-renames", fork, head]).map(|names| names.lines().map(str::to_owned).collect())
}

fn lists_at(directory: &Path, base: &str) -> Result<DocumentMut, String> {
    let text = git::text(directory, &["show", &format!("{base}:{DEFAULTS}")]).ok_or_else(|| format!("{DEFAULTS} is not at {base}, so no change can be told to cross a trust boundary"))?;
    text.parse::<DocumentMut>().map_err(|error| format!("{DEFAULTS} at {base} does not parse: {error}"))
}

/// The commits on main below `fork` whose exact reverse is the lane's patch `patch`, named by git's `This reverts commit` line or
/// the lander's and sheriff's `Reverts:` trailers: one named commit, or the range from the first named to the last, as the lander
/// names each commit of a reverted lane oldest first. A named commit of the lane itself is no revert of main and counts for nothing.
pub(crate) fn reverted(directory: &Path, fork: &str, head: &str, patch: &str) -> Option<Vec<String>> {
    let messages = git::messages(directory, fork, head);
    let named: Vec<&str> = messages
        .lines()
        .filter_map(|line| REVERTS.iter().find_map(|prefix| line.strip_prefix(prefix)))
        .filter_map(|reverted| reverted.split(|c: char| !c.is_ascii_hexdigit()).next())
        .filter(|reverted| !reverted.is_empty() && git::text(directory, &["merge-base", "--is-ancestor", reverted, fork]).is_some())
        .collect();
    let reverse = |newest: &str, oldest: &str| patch_id(directory, newest, &format!("{oldest}^")).is_ok_and(|reverse| reverse == patch);
    if let Some(one) = named.iter().find(|reverted| reverse(reverted, reverted)) {
        return Some(vec![(*one).to_owned()]);
    }
    named.first().zip(named.last()).filter(|(oldest, newest)| reverse(newest, oldest)).map(|_| named.iter().map(|reverted| (*reverted).to_owned()).collect())
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

/// `Err` is a lane that changes a `trust_boundary_paths` file of the harness/defaults.toml at its landing `base` and holds no accept of its patch id from a reviewer session that did not author the lane.
pub fn check(directory: &Path, base: &str, head: &str, accepts: &Accepts, finished: &[Finished]) -> Result<(), String> {
    check_listed(directory, base, base, head, accepts, finished)
}

fn check_listed(directory: &Path, listed_at: &str, base: &str, head: &str, accepts: &Accepts, finished: &[Finished]) -> Result<(), String> {
    let fork = git::merge_base(directory, base, head);
    let files = changed(directory, &fork, head).ok_or_else(|| format!("git could not diff {fork}..{head}"))?;
    let lists = lists_at(directory, listed_at)?;
    let crossing = trust_boundary::crossing(&lists, &files).ok_or_else(|| format!("{DEFAULTS} at {listed_at} lists no trust_boundary_paths, so no change can be told to cross one"))?;
    let unreviewed = reverts_unreviewed(directory, &fork, head, accepts, finished);
    let crossing: Vec<String> = crossing.into_iter().filter(|_| !unreviewed).map(str::to_owned).collect();
    if crossing.is_empty() {
        return Ok(());
    }
    let (patch, accepted) = reviewers(directory, &fork, head, accepts, finished)?;
    if !accepted.is_empty() {
        return Ok(());
    }
    Err(format!("the lane changes {} of trust_boundary_paths and holds no review accept of its patch {patch} from a reviewer session that did not author the lane; a reviewer's `north runtime accept {head}` records one", crossing.join(", ")))
}

/// Whether the lane `fork..head` is an exact revert of commits that each landed unreviewed across a trust-boundary path, so the revert removes only content no reviewer read and needs no accept of its own.
fn reverts_unreviewed(directory: &Path, fork: &str, head: &str, accepts: &Accepts, finished: &[Finished]) -> bool {
    let gate = gate_commit(directory);
    patch_id(directory, fork, head).ok().and_then(|patch| reverted(directory, fork, head, &patch)).is_some_and(|commits| commits.iter().all(|commit| unreviewed_landing(directory, gate.as_deref(), commit, accepts, finished)))
}

/// The commit that first gave `harness/defaults.toml` its `trust_boundary_paths` list, from which the review gate reads trust boundaries.
fn gate_commit(directory: &Path) -> Option<String> {
    git::text(directory, &["log", "--reverse", "--format=%H", "-S[trust_boundary_paths]", "--", DEFAULTS])?.lines().next().map(str::to_owned)
}

/// Whether `commit` landed after `gate`, crossed a trust-boundary path at its own landing, and holds no reviewer accept of its patch.
fn unreviewed_landing(directory: &Path, gate: Option<&str>, commit: &str, accepts: &Accepts, finished: &[Finished]) -> bool {
    let parent = format!("{commit}^");
    let crossed = changed(directory, &parent, commit).zip(lists_at(directory, &parent).ok()).is_some_and(|(files, lists)| trust_boundary::crossing(&lists, &files).is_some_and(|crossing| !crossing.is_empty()));
    let after_gate = gate.is_some_and(|gate| git::text(directory, &["merge-base", "--is-ancestor", gate, commit]).is_some());
    let unaccepted = reviewers(directory, &parent, commit, accepts, finished).is_ok_and(|(_, accepted)| accepted.is_empty());
    [crossed, after_gate, unaccepted].into_iter().all(std::convert::identity)
}

/// The test functions the lane `fork..head` removes from each Rust file it changes, by name: those its base defines and its head does not, whether it deletes or renames them.
fn deleted_tests(directory: &Path, fork: &str, head: &str, files: &[String]) -> Vec<String> {
    files
        .iter()
        .filter(|file| file.ends_with(".rs"))
        .flat_map(|file| test_functions(&source_at(directory, fork, file)).difference(&test_functions(&source_at(directory, head, file))).cloned().collect::<Vec<_>>())
        .collect()
}

/// The refusal of the shadow gate `DELETIONS` for the lane `base..head`: the test functions it removes, unless it is an exact revert of main or holds a reviewer accept of its patch from a session that did not author it.
fn deleted_refusal(directory: &Path, base: &str, head: &str, accepts: &Accepts, finished: &[Finished]) -> Result<Option<String>, String> {
    let fork = git::merge_base(directory, base, head);
    let files = changed(directory, &fork, head).ok_or_else(|| format!("git could not diff {fork}..{head}"))?;
    let deleted = deleted_tests(directory, &fork, head, &files);
    let reverting = patch_id(directory, &fork, head).ok().and_then(|patch| reverted(directory, &fork, head, &patch));
    let (patch, accepted) = reviewers(directory, &fork, head, accepts, finished)?;
    Ok(Some(deleted).filter(|deleted| !deleted.is_empty()).filter(|_| reverting.is_none()).filter(|_| accepted.is_empty()).map(|deleted| format!("the lane deletes test functions {} and holds no review accept of its patch {patch} from a reviewer session that did not author the lane", deleted.join(", "))))
}

fn source_at(directory: &Path, revision: &str, file: &str) -> String {
    git::text(directory, &["show", &format!("{revision}:{file}")]).unwrap_or_default()
}

static TEST_FUNCTION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"#\[(?:[[:alnum:]_]+::)?test\](?:\s*(?:#\[[^\]]*\]|//[^\n]*))*\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([[:alnum:]_]+)").expect("test function pattern"));

/// The names of the test functions in `source`: each `fn` that a `#[test]` or `#[...::test]` attribute marks, with the attributes and doc comments between.
fn test_functions(source: &str) -> BTreeSet<String> {
    TEST_FUNCTION.captures_iter(source).map(|found| found[1].to_owned()).collect()
}

pub(crate) fn each_commit(directory: &Path, base: &str, head: &str, judge: impl Fn(&str, &str) -> Result<(), String>) -> Result<(), String> {
    let Err(refusal) = judge(base, head) else { return Ok(()) };
    if std::env::var(ONE_LANE_ENV).is_ok_and(|flag| flag == "1") {
        return Err(refusal);
    }
    let fork = git::merge_base(directory, base, head);
    let listed = git::text(directory, &["rev-list", "--first-parent", "--reverse", &format!("{fork}..{head}")]).unwrap_or_default();
    let commits: Vec<&str> = listed.lines().collect();
    if commits.len() < 2 {
        return Err(refusal);
    }
    for commit in commits {
        judge(&format!("{commit}^"), commit).map_err(|why| format!("{}: {why}", &commit[..commit.len().min(10)]))?;
    }
    Ok(())
}

pub fn scan(directory: &Path, range: &str, state: &Path, accepts: &Accepts, finished: &FinishRecords, out: &mut impl Write) -> u8 {
    let Some(Range { base, head }) = Range::parse(range) else {
        let _ = writeln!(out, "{SELF}: expected BASE..HEAD, got {range}");
        return 2;
    };
    let finished = finished.all();
    let outcome = each_commit(directory, base, head, |from, to| check_listed(directory, base, from, to, accepts, &finished));
    let fork = git::merge_base(directory, base, head);
    let deletions = deleted_refusal(directory, base, head, accepts, &finished);
    holdlist::record(state, DELETIONS, (head, patch_id(directory, &fork, head).ok().as_deref()), deletions.as_ref().ok().and_then(|found| found.as_deref()));
    let enforce = holdlist::enforcing(directory, SELF, [base, head]);
    if !enforce {
        holdlist::record(state, SELF, (head, patch_id(directory, &fork, head).ok().as_deref()), outcome.as_ref().err().map(String::as_str));
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

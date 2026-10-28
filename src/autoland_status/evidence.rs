use crate::platform::process;
use std::time::Duration;

pub(crate) fn git_output(arguments: &[&str]) -> Option<String> {
    process::run_within("git", arguments, Duration::from_secs(30))
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
}

fn patch_ids(arguments: &[&str]) -> Option<String> {
    let patches = git_output(arguments)?;
    process::run_with_input_within(
        "git",
        &["patch-id", "--stable"],
        Some(patches.as_bytes()),
        Duration::from_secs(30),
    )
    .filter(|output| output.status.success())
    .and_then(|output| String::from_utf8(output.stdout).ok())
}

pub(crate) fn landed_commit(candidate: &str, destination: &str) -> Option<String> {
    if process::run_within(
        "git",
        &["merge-base", "--is-ancestor", candidate, destination],
        Duration::from_secs(30),
    )
    .is_some_and(|output| output.status.success())
    {
        return git_output(&["rev-parse", "--verify", &format!("{candidate}^{{commit}}")])
            .map(|sha| sha.trim().to_owned());
    }
    let base = git_output(&["merge-base", candidate, destination])?;
    let base = base.trim();
    let lane = range_patch_ids(&format!("{base}..{candidate}"))?;
    let landed = range_patch_ids(&format!("{base}..{destination}"))?;
    let landed_for = |patch: &str| {
        landed
            .iter()
            .find(|(other, _)| other == patch)
            .map(|(_, sha)| sha.clone())
    };
    let tip = landed_for(&lane.first()?.0);
    lane.iter()
        .all(|(patch, _)| landed_for(patch).is_some())
        .then_some(tip)
        .flatten()
}

fn range_patch_ids(range: &str) -> Option<Vec<(String, String)>> {
    let ids = patch_ids(&[
        "log",
        "--no-ext-diff",
        "--no-color",
        "--binary",
        "--format=commit %H",
        "-p",
        "--no-merges",
        range,
    ])?;
    Some(
        ids.lines()
            .filter_map(|line| {
                let mut fields = line.split_whitespace();
                Some((fields.next()?.to_owned(), fields.next()?.to_owned()))
            })
            .collect(),
    )
}

pub(super) fn landed_by_subject(candidate: &str, destination: &str) -> Option<String> {
    let base = git_output(&["merge-base", candidate, destination])?;
    let lane = git_output(&[
        "log",
        "--no-merges",
        "--format=%s",
        &format!("{}..{candidate}", base.trim()),
    ])?;
    let landed = git_output(&[
        "log",
        "--no-merges",
        "--format=%H%x09%s",
        &format!("{}..{destination}", base.trim()),
    ])?;
    let landed: Vec<(&str, &str)> = landed
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .collect();
    let landed_for = |subject: &str| {
        landed
            .iter()
            .find(|(_, other)| *other == subject)
            .map(|(sha, _)| (*sha).to_owned())
    };
    let tip = landed_for(lane.lines().next()?);
    lane.lines()
        .all(|subject| landed_for(subject).is_some())
        .then_some(tip)
        .flatten()
}

pub(super) fn branch_gone_from_origin(branch: &str) -> bool {
    git_output(&["ls-remote", "origin", &format!("refs/heads/{branch}")])
        .is_some_and(|listing| listing.trim().is_empty())
}

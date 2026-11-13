use std::path::Path;

use super::candidate::git;
use north_base::land_ticket::Phase;

/// Names, after a conflict verdict's files, the newest landing on `base` since the lane forked that touched any of them.
pub fn cite(worktree: &Path, base: &str, head: &str, verdict: &str) -> String {
    let Some((_, listed)) = verdict.split_once(' ').filter(|_| Phase::decided(verdict) == Phase::Conflict) else { return verdict.to_owned() };
    let Ok(fork) = git(worktree, &["merge-base", base, head]) else { return verdict.to_owned() };
    let range = format!("{fork}..{base}");
    let mut arguments = vec!["--literal-pathspecs", "log", "-1", "--format=%h %s", &range, "--"];
    arguments.extend(listed.split(','));
    match git(worktree, &arguments) {
        Ok(landing) if !landing.is_empty() => format!("{verdict} after landing {landing}"),
        _ => verdict.to_owned(),
    }
}

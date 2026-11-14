use std::path::Path;

use north_lander::land::candidate::git;

/// A reviewer's standing hold on a commit, which keeps every head containing it from going live.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewHold {
    pub commit: String,
    pub session: String,
    pub why: String,
}

/// The first of `holds` on a commit main holds and the lander runtime `lander` lacks, which keeps the runtime off main on purpose.
pub(super) fn standing_over(checkout: &Path, lander: &str, holds: &[ReviewHold]) -> Option<ReviewHold> {
    let sha = Path::new(lander).file_name()?.to_str()?;
    let contains = |revision: &str, commit: &str| git(checkout, &["merge-base", "--is-ancestor", commit, revision]).is_ok();
    holds.iter().find(|hold| contains("refs/remotes/origin/main", &hold.commit) && !contains(sha, &hold.commit)).cloned()
}

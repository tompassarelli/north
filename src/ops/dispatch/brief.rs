//! The brief a dispatched worker starts from: the issue's Item, Category and facets, read from its
//! body or defaulted, and the generic Done when every dispatched box shares.
use super::issues::{Issue, OWNER};
use super::facets::{FACETS, declared, facet, native};
use super::tier::{Route, Tier};

pub(crate) const HAIKU_WAIT: &str = " Run any command that may take over a minute in the background and wait for its exit with Monitor; never end your turn while a run you started is still going.";
pub(crate) const SAFETY_LINE: &str = "Run only build, test, git and file commands inside your clone and /tmp. Never kill, pkill or signal a process, never use sudo, and use no network beyond git clone, git fetch and package installs. If a command is blocked or would need approval, do not retry it or work around it; stop and report `Blocked:` with the command.";

/// The brief for one ready issue; its first block, before the blank line, is what placement and
/// routing read.
pub(crate) fn brief(issue: &Issue, launched_by: &str, route: &Route) -> String {
    let reference = issue.reference();
    let mut head = vec![format!("Item: {reference} {}", issue.title), format!("Repository: {OWNER}/{}", issue.repo)];
    head.extend(FACETS.iter().map(|(key, _)| format!("{key}: {}", facet(issue, key))));
    head.push(format!("Tier: {}", route.tier.name()));
    head.extend(route.arm.then(|| format!("Arm: {}", route.tier.name())));
    head.extend(route.follows.as_ref().map(|follows| format!("Follows: {follows}")));
    let needs = declared(issue, "Needs").map(str::to_owned).or_else(|| native(issue).then(|| "native".to_owned()));
    head.extend(needs.map(|needs| format!("Needs: {needs}")));
    head.push(format!(
        "Done when: every Done-when box of {reference} is ticked with its evidence, the change is on origin/main (`git merge-base --is-ancestor SHA origin/main`), and {reference} is closed with the verified SHA."
    ));
    head.push(format!("ETA: {}", declared(issue, "ETA").unwrap_or("45 min (uncalibrated)")));
    head.push(format!("Files: as {reference} names them"));
    format!(
        "{}\n\n{launched_by} staffed this box because a worker slot was free and {reference} was ready: open, unticked, unheld and unblocked. \
         Read it with `gh issue view {} -R {OWNER}/{}` and work its unticked Done-when boxes; each box's evidence is an observation outside you. \
         Claim it with `threads claim {reference}` first, and stop with `Not done:` if someone else holds it. \
         Load the skills the issue's work needs, edit in a worktree, land through safe-push, tick each box with its evidence and close the issue with the SHA on origin/main. \
         End with Done:, Not done: or Split:, and Stop: with its reason.{}",
        head.join("\n"),
        issue.number,
        issue.repo,
        if route.tier == Tier::Haiku { HAIKU_WAIT } else { "" },
    )
}

pub(crate) const AUTHOR_NAME: &str = "tompassarelli";
pub(crate) const AUTHOR_EMAIL: &str = "6378995+tompassarelli@users.noreply.github.com";
pub(crate) const HANDBACK: &str = "git format-patch origin/main..HEAD --stdout | gzip -9 | base64 -w 76 > /tmp/handback.b64, then one Bash call per 300-line part i of n: echo BEGIN-HANDBACK part i/n; sed -n <first>,<last>p /tmp/handback.b64; echo END-HANDBACK part i/n";

/// The brief a cloud session starts from: the box brief, the cloud safety line, and the last steps
/// that land it by pushing `branch`, with the patch handback only when the push is refused.
pub(crate) fn cloud(brief: &str, repo: &str, branch: &str) -> String {
    let (closes, after) = if super::cloud::autoland(repo) {
        (" and ends with `Closes #<number>`", format!("Report `Done: queued {branch}`; Autoland lands it once its checks pass, and no courier runs."))
    } else {
        ("", format!("Report `Done: pushed {branch}`; a courier off the cloud host lands it through safe-push."))
    };
    format!(
        "{brief}\n\nCloud safety line: {SAFETY_LINE} The one push allowed is `git push origin HEAD:refs/heads/{branch}`.\n\n\
         You run in Anthropic's cloud and land by pushing a branch, so these last steps replace the brief's claim, worktree, safe-push, tick and close steps. \
         Run the repository's checks as its AGENTS.md names them and fix what fails. \
         Rebase onto a fresh `git fetch origin main` and rerun the checks if main moved. \
         Run `git config user.name {AUTHOR_NAME}` and `git config user.email {AUTHOR_EMAIL}`, fold all your work into one commit with `git reset --soft origin/main`, \
         and commit it with a message that names the Item's issue{closes} and carries no `Claude-Session:` or `Co-Authored-By:` trailer, \
         because an earlier commit's private email or session URL makes the deny-list refuse the branch. \
         If the push below is refused, hand the work back instead: {HANDBACK}; then report `Not done:` with the refusal and `Stop: landing`. \
         Last, push with `git push origin HEAD:refs/heads/{branch}`. {after}"
    )
}
